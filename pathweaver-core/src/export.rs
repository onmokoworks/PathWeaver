//! Export of the drawing as a structured scene (folders → groups, paths with
//! their real curves and line settings) and its SVG form. The PDF writer
//! (`pdf.rs`) draws the same scene.
//!
//! Where things come from, one function each, so new paint or geometry kinds
//! plug in at one place:
//! - `fill_paint` / `edge_paint`: what a path is painted with;
//! - `centerline` / `contour_curves`: its geometry as curves;
//! - `line_style`: whether its line can stay a line (uniform width).
use std::sync::Arc;

use kurbo::{Affine, BezPath, PathEl, Point};

use super::model::{Cap, Document, Join, Stroke};
use super::outline;

/// A paint: a colour, or a gradient (`color` is then its solid fallback,
/// the first stop), with an overall opacity.
#[derive(Clone, Debug, PartialEq)]
pub struct Paint {
    pub color: [u8; 3],
    pub opacity: f64,
    pub gradient: Option<super::paint::Gradient>,
}
/// A line drawn along a path with a uniform width.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub paint: Paint,
    pub width: f64,
    pub cap: Cap,
    pub join: Join,
    pub miter_limit: f64,
}
/// One filled and/or stroked path (nonzero fill rule).
#[derive(Clone, Debug)]
pub struct Shape {
    pub path: BezPath,
    pub fill: Option<Paint>,
    pub line: Option<Line>,
}
#[derive(Clone, Debug)]
pub enum Source {
    /// A data URL (placed images).
    Data(String),
    /// A bitmap from the appearance graph.
    Bitmap(Arc<super::raster::Image>),
}
/// A bitmap: pixel space 0..width × 0..height, placed by `matrix`.
#[derive(Clone, Debug)]
pub struct Picture {
    pub source: Source,
    pub width: f64,
    pub height: f64,
    pub matrix: Affine,
    pub opacity: f64,
}
/// Name, id and (for text) the characters of what an item came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub text: Option<String>,
    /// A layer folder (`<g inkscape:groupmode="layer">`).
    pub layer: bool,
}
#[derive(Clone, Debug)]
pub enum Item {
    Group(Meta, Vec<Item>),
    Shape(Option<Meta>, Shape),
    Picture(Option<Meta>, Picture),
}

/// How lines are written.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// 輪郭にする: every line becomes its filled outline (as curves).
    pub outline: bool,
}

/// How closely outlines made of many short segments are fitted with curves.
pub const FIT_ACCURACY: f64 = 0.05;

pub fn fill_paint(s: &Stroke) -> Option<Paint> {
    s.fill.map(|color| Paint { color, opacity: s.fill_opacity.clamp(0., 1.), gradient: s.fill_gradient.clone() })
}
pub fn edge_paint(s: &Stroke) -> Option<Paint> {
    s.edge.map(|color| Paint { color, opacity: s.edge_opacity.clamp(0., 1.), gradient: s.edge_gradient.clone() })
}

/// The path's own curve (rounded corners applied), as drawn by the pen.
pub fn centerline(s: &Stroke) -> BezPath {
    let rounded;
    let s = if s.corners.is_empty() {
        s
    } else {
        rounded = super::corners::rounded(s);
        &rounded
    };
    let mut p = BezPath::new();
    if s.nodes.len() < 2 {
        return p;
    }
    p.move_to(s.nodes[0].p);
    for i in 0..s.segment_count() {
        let c = s.cubic(i);
        if straight(c) {
            p.line_to(c.p3);
        } else {
            p.curve_to(c.p1, c.p2, c.p3);
        }
    }
    if s.closed {
        p.close_path();
    }
    p
}

/// A cubic whose handles lie on its chord (between its ends) is a line.
fn straight(c: kurbo::CubicBez) -> bool {
    let chord = c.p3 - c.p0;
    let len2 = chord.hypot2();
    if len2 < 1e-18 {
        return c.p1 == c.p0 && c.p2 == c.p3;
    }
    [c.p1, c.p2].iter().all(|q| {
        let v = *q - c.p0;
        (v.cross(chord)).abs() <= 1e-9 * len2 && v.dot(chord) >= -1e-9 * len2 && v.dot(chord) <= len2 * (1. + 1e-9)
    })
}

/// Closed rings (many short straight segments) as curves within `accuracy`;
/// sharp turns stay corners.
pub fn contour_curves(rings: &[Vec<Point>], accuracy: f64) -> BezPath {
    super::curve_fit::fit_rings(rings, accuracy)
}
/// Rings exactly as given (straight segments).
pub fn polygon_path(rings: &[Vec<Point>]) -> BezPath {
    let mut out = BezPath::new();
    for r in rings.iter().filter(|r| r.len() > 2) {
        out.move_to(r[0]);
        for p in &r[1..] {
            out.line_to(*p);
        }
        out.close_path();
    }
    out
}
/// The line width when the whole profile is one width with straight cuts
/// (what SVG / PDF lines can draw); `None` for a variable-width line.
pub fn uniform_width(s: &Stroke) -> Option<f64> {
    if s.contours.is_some() {
        return Some(s.edge_width);
    }
    let mut widths = vec![s.start_width];
    if !s.closed {
        widths.push(s.end_width);
    }
    widths.extend(s.widths.iter().filter(|k| !k.interpolated).map(|k| k.width));
    let w = s.start_width.left + s.start_width.right;
    let eps = 1e-6 * w.max(1.);
    if widths.iter().any(|x| (x.left - w / 2.).abs() > eps || (x.right - w / 2.).abs() > eps || x.angle.abs() > 1e-6) {
        return None;
    }
    // A cut turned away from square (a locked cap angle) is not a line cap.
    if !s.closed && s.nodes.len() >= 2 {
        let last = s.segment_count().saturating_sub(1);
        for (end, locked, i, t) in [(false, s.start_cap_locked, 0, 0.), (true, s.end_cap_locked, last, 1.)] {
            if locked {
                let d = outline::cap_direction(s, end);
                let n = outline::normal(s.tangent(i, t));
                if (d.x * n.y - d.y * n.x).abs() > 1e-6 {
                    return None;
                }
            }
        }
    }
    Some(w)
}
/// The path's line as a uniform line, if it can be one.
pub fn line_style(s: &Stroke) -> Option<Line> {
    let paint = edge_paint(s)?;
    let width = uniform_width(s)?;
    let contours = s.contours.is_some();
    Some(Line {
        paint,
        width,
        cap: if contours || s.closed { Cap::Butt } else { common_cap(s)? },
        join: s.join,
        miter_limit: s.miter_limit.max(1.),
    })
}
/// Both ends must share one cap to be a line cap.
fn common_cap(s: &Stroke) -> Option<Cap> {
    (s.start_cap == s.end_cap).then_some(s.start_cap)
}

fn meta(s: &Stroke) -> Meta {
    Meta {
        id: format!("pw-{}", s.id),
        name: s.name.clone(),
        text: s.text.as_ref().map(|t| t.text.clone()),
        layer: false,
    }
}

/// The geometry the fill (and a uniform line) follows.
fn shape_path(s: &Stroke) -> BezPath {
    match &s.contours {
        Some(c) => contour_curves(c, FIT_ACCURACY),
        None => centerline(s),
    }
}

/// The line's filled outline (rings), as the canvas paints it.
fn edge_outline(s: &Stroke) -> Result<Vec<Vec<Point>>, String> {
    let mut only = s.clone();
    only.fill = None;
    only.appearance = None;
    Ok(outline::paint_layers(&only)?.into_iter().flat_map(|l| l.outline.contours).collect())
}

/// What one path exports as.
pub fn stroke_items(doc: &Document, s: &Stroke, options: Options) -> Result<Option<Item>, String> {
    if let Some(img) = &s.image {
        // 下絵 (tracing reference) is never exported. 抜き: the caller hands in
        // the keyed copy next to the original.
        if img.reference || img.opacity <= 0. {
            return Ok(None);
        }
        let keyed = img.knockout.and_then(|k| doc.assets.get(&super::model::knockout_asset(&img.asset, k)));
        let Some(href) = keyed.or_else(|| doc.assets.get(&img.asset)) else { return Ok(None) };
        return Ok(Some(Item::Picture(
            Some(meta(s)),
            Picture {
                source: Source::Data(href.clone()),
                width: img.width,
                height: img.height,
                matrix: Affine::new(img.matrix),
                opacity: img.opacity.clamp(0., 1.),
            },
        )));
    }
    if s.appearance.is_some() {
        // The rendered result of the appearance graph, layer by layer.
        let mut items = vec![];
        for layer in outline::paint_layers(s)? {
            if layer.opacity <= 0. {
                continue;
            }
            let opacity = layer.opacity.clamp(0., 1.);
            if let Some(img) = &layer.image {
                items.push(Item::Picture(None, bitmap(img.clone(), opacity)));
            } else if !layer.outline.contours.is_empty() {
                items.push(Item::Shape(
                    None,
                    Shape { path: polygon_path(&layer.outline.contours), fill: Some(Paint { color: layer.color, opacity, gradient: layer.gradient.clone() }), line: None },
                ));
            }
        }
        return Ok((!items.is_empty()).then(|| Item::Group(meta(s), items)));
    }
    let fill = fill_paint(s);
    let has_geometry = s.contours.as_ref().map_or(s.nodes.len() >= 2, |c| c.iter().any(|r| r.len() > 2));
    if !has_geometry {
        return Ok(None);
    }
    let uniform = line_style(s);
    // a zero-width line paints nothing either way
    let empty_line = uniform.as_ref().is_some_and(|l| l.width <= 0.);
    let line = uniform.filter(|_| !options.outline && !empty_line);
    let mut items = vec![];
    if fill.is_some() || line.is_some() {
        items.push(Shape { path: shape_path(s), fill, line: line.clone() });
    }
    if s.edge.is_some() && line.is_none() && !empty_line {
        // Variable width (or 輪郭にする): the line's outline, as curves.
        let rings = edge_outline(s)?;
        if !rings.is_empty() {
            items.push(Shape { path: contour_curves(&rings, FIT_ACCURACY), fill: edge_paint(s), line: None });
        }
    }
    Ok(match items.len() {
        0 => None,
        1 => Some(Item::Shape(Some(meta(s)), items.pop().unwrap())),
        _ => Some(Item::Group(meta(s), items.into_iter().map(|x| Item::Shape(None, x)).collect())),
    })
}
fn bitmap(img: Arc<super::raster::Image>, opacity: f64) -> Picture {
    Picture {
        width: img.width as f64,
        height: img.height as f64,
        matrix: Affine::translate((img.x0, img.y0)) * Affine::scale(1. / img.scale),
        source: Source::Bitmap(img),
        opacity,
    }
}

/// Enclosing folders of a path, outermost first.
fn folder_chain(doc: &Document, s: &Stroke) -> Vec<u64> {
    let mut chain = vec![];
    let mut at = s.folder;
    while let Some(id) = at {
        if chain.len() > 64 || chain.contains(&id) {
            break;
        }
        let Some(f) = doc.folders.iter().find(|f| f.id == id) else { break };
        chain.push(id);
        at = f.parent;
    }
    chain.reverse();
    chain
}

/// The whole drawing as items: shown paths in stacking order, folders as
/// nested groups (a folder split by paths of other folders in the stack
/// appears as several groups, so the stacking stays exact). Guides, hidden
/// paths and hidden folders are left out.
pub fn scene(doc: &Document, options: Options) -> Result<Vec<Item>, String> {
    let mut stack: Vec<(Option<u64>, Meta, Vec<Item>)> = vec![(None, Meta::default(), vec![])];
    let mut seen = std::collections::HashMap::<u64, usize>::new();
    fn close(stack: &mut Vec<(Option<u64>, Meta, Vec<Item>)>) {
        let (_, meta, items) = stack.pop().unwrap();
        if !items.is_empty() {
            stack.last_mut().unwrap().2.push(Item::Group(meta, items));
        }
    }
    for s in doc.strokes.iter().filter(|s| doc.shown(s) && !s.guide) {
        let Some(item) = stroke_items(doc, s, options)? else { continue };
        let chain = folder_chain(doc, s);
        let mut depth = 0;
        while depth + 1 < stack.len() && depth < chain.len() && stack[depth + 1].0 == Some(chain[depth]) {
            depth += 1;
        }
        while stack.len() > depth + 1 {
            close(&mut stack);
        }
        for &id in &chain[depth..] {
            let f = doc.folders.iter().find(|f| f.id == id).unwrap();
            let n = seen.entry(id).or_insert(0);
            *n += 1;
            let meta = Meta {
                id: if *n == 1 { format!("pw-folder-{id}") } else { format!("pw-folder-{id}-{n}") },
                name: f.name.clone(),
                text: None,
                layer: true,
            };
            stack.push((Some(id), meta, vec![]));
        }
        stack.last_mut().unwrap().2.push(item);
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    Ok(stack.pop().unwrap().2)
}

/// Bounds of everything painted (effects included), as the canvas shows it.
pub fn painted_bounds(doc: &Document) -> Result<Option<(Point, Point)>, String> {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |p: Point| {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    };
    for s in doc.strokes.iter().filter(|s| doc.shown(s) && !s.guide) {
        if let Some(img) = &s.image {
            let keyed = img.knockout.and_then(|k| doc.assets.get(&super::model::knockout_asset(&img.asset, k)));
            if img.reference || img.opacity <= 0. || keyed.or_else(|| doc.assets.get(&img.asset)).is_none() {
                continue;
            }
            img.rect().into_iter().for_each(&mut add);
            continue;
        }
        for layer in outline::paint_layers(s)? {
            if layer.opacity <= 0. {
                continue;
            }
            if let Some(img) = &layer.image {
                img.rect().into_iter().for_each(&mut add);
            } else {
                layer.outline.contours.iter().flatten().copied().for_each(&mut add);
            }
        }
    }
    Ok(min.x.is_finite().then_some((min, max)))
}

/// The frame to export: the given one, or what is painted plus `margin`.
pub fn resolve_frame(doc: &Document, frame: Option<[f64; 4]>, margin: f64) -> Result<[f64; 4], String> {
    let f = match frame {
        Some(f) => f,
        None => {
            let (min, max) = painted_bounds(doc)?.unwrap_or((Point::ZERO, Point::new(100., 100.)));
            let m = margin.max(0.);
            [min.x - m, min.y - m, max.x - min.x + 2. * m, max.y - min.y + 2. * m]
        }
    };
    if !(f.iter().all(|v| v.is_finite()) && f[2] > 0. && f[3] > 0.) {
        return Err("書き出す範囲が空です".into());
    }
    Ok(f)
}

// ---- SVG ----

/// A number for SVG / PDF: up to 3 decimals, no trailing zeros.
pub fn num(v: f64) -> String {
    let mut s = format!("{:.3}", if v.is_finite() { v } else { 0. });
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\n' => o.push_str("&#10;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}
fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}
pub fn path_data(p: &BezPath) -> String {
    let mut d = String::new();
    let mut at = Point::ZERO;
    for e in p.elements() {
        if !d.is_empty() {
            d.push(' ');
        }
        match *e {
            PathEl::MoveTo(a) => {
                d.push_str(&format!("M{} {}", num(a.x), num(a.y)));
                at = a;
            }
            PathEl::LineTo(a) => {
                d.push_str(&format!("L{} {}", num(a.x), num(a.y)));
                at = a;
            }
            PathEl::QuadTo(a, b) => {
                let (c1, c2) = (at + (a - at) * (2. / 3.), b + (a - b) * (2. / 3.));
                d.push_str(&format!("C{} {} {} {} {} {}", num(c1.x), num(c1.y), num(c2.x), num(c2.y), num(b.x), num(b.y)));
                at = b;
            }
            PathEl::CurveTo(a, b, c) => {
                d.push_str(&format!("C{} {} {} {} {} {}", num(a.x), num(a.y), num(b.x), num(b.y), num(c.x), num(c.y)));
                at = c;
            }
            PathEl::ClosePath => d.push('Z'),
        }
    }
    d
}
fn meta_attrs(m: &Meta) -> String {
    let mut a = format!(" id=\"{}\"", esc(&m.id));
    if !m.name.is_empty() {
        a.push_str(&format!(" data-name=\"{}\"", esc(&m.name)));
        if m.layer {
            a.push_str(&format!(" inkscape:groupmode=\"layer\" inkscape:label=\"{}\"", esc(&m.name)));
        }
    }
    if let Some(t) = &m.text {
        a.push_str(&format!(" data-text=\"{}\"", esc(t)));
    }
    a
}
/// `fill`/`stroke` value for a paint; a gradient's element goes to `defs`.
fn paint_ref(p: &Paint, defs: &mut String, ids: &mut usize) -> String {
    match &p.gradient {
        Some(g) => {
            *ids += 1;
            let id = format!("pw-gradient-{ids}");
            defs.push_str(&super::paint::gradient_svg(g, &id));
            format!("url(#{id})")
        }
        None => hex(p.color),
    }
}
fn shape_svg(meta: Option<&Meta>, s: &Shape, ids: &mut usize) -> String {
    let mut defs = String::new();
    let mut a = String::from("<path");
    if let Some(m) = meta {
        a.push_str(&meta_attrs(m));
    }
    match &s.fill {
        Some(p) => {
            a.push_str(&format!(" fill=\"{}\"", paint_ref(p, &mut defs, ids)));
            if p.opacity < 1. {
                a.push_str(&format!(" fill-opacity=\"{}\"", num(p.opacity)));
            }
        }
        None => a.push_str(" fill=\"none\""),
    }
    if let Some(l) = &s.line {
        a.push_str(&format!(" stroke=\"{}\" stroke-width=\"{}\"", paint_ref(&l.paint, &mut defs, ids), num(l.width)));
        if l.paint.opacity < 1. {
            a.push_str(&format!(" stroke-opacity=\"{}\"", num(l.paint.opacity)));
        }
        a.push_str(match l.cap {
            Cap::Butt => " stroke-linecap=\"butt\"",
            Cap::Round => " stroke-linecap=\"round\"",
            Cap::Square => " stroke-linecap=\"square\"",
        });
        a.push_str(match l.join {
            Join::Miter => " stroke-linejoin=\"miter\"",
            Join::Round => " stroke-linejoin=\"round\"",
            Join::Bevel => " stroke-linejoin=\"bevel\"",
        });
        if l.join == Join::Miter {
            a.push_str(&format!(" stroke-miterlimit=\"{}\"", num(l.miter_limit)));
        }
    }
    a.push_str(&format!(" d=\"{}\"", path_data(&s.path)));
    if let Some(t) = meta.and_then(|m| m.text.as_ref()) {
        a.push_str(&format!("><title>{}</title></path>", esc(t)));
    } else {
        a.push_str("/>");
    }
    if defs.is_empty() { a } else { format!("<defs>{defs}</defs>{a}") }
}
fn picture_svg(meta: Option<&Meta>, p: &Picture) -> String {
    let href = match &p.source {
        Source::Data(s) => s.as_str(),
        Source::Bitmap(img) => img.data_url(),
    };
    let m = p.matrix.as_coeffs();
    let mut a = String::from("<image");
    if let Some(me) = meta {
        a.push_str(&meta_attrs(me));
    }
    a.push_str(&format!(
        " width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {})\"",
        num(p.width), num(p.height),
        format_args!("{:.6}", m[0]), format_args!("{:.6}", m[1]), format_args!("{:.6}", m[2]), format_args!("{:.6}", m[3]), num(m[4]), num(m[5])
    ));
    if p.opacity < 1. {
        a.push_str(&format!(" opacity=\"{}\"", num(p.opacity)));
    }
    a.push_str(&format!(" href=\"{}\"/>", esc(href)));
    a
}
fn items_svg(items: &[Item], depth: usize, out: &mut String, ids: &mut usize) {
    for item in items {
        out.push_str(&"  ".repeat(depth));
        match item {
            Item::Group(m, children) => {
                out.push_str(&format!("<g{}>\n", meta_attrs(m)));
                items_svg(children, depth + 1, out, ids);
                out.push_str(&"  ".repeat(depth));
                out.push_str("</g>");
            }
            Item::Shape(m, s) => out.push_str(&shape_svg(m.as_ref(), s, ids)),
            Item::Picture(m, p) => out.push_str(&picture_svg(m.as_ref(), p)),
        }
        out.push('\n');
    }
}

/// SVG of the drawing seen through `frame` (an artboard: x, y, w, h; `None` =
/// everything painted plus `margin`), at its size in units, optionally over
/// a solid `background`. Folders are groups (layers in Inkscape), lines keep
/// their curves and settings unless `options.outline`.
pub fn svg(doc: &Document, frame: Option<[f64; 4]>, background: Option<[u8; 3]>, margin: f64, options: Options) -> Result<String, String> {
    let [x, y, w, h] = resolve_frame(doc, frame, margin)?;
    let mut body = String::new();
    if let Some([r, g, b]) = background {
        body.push_str(&format!("  <rect x=\"{x:.4}\" y=\"{y:.4}\" width=\"{w:.4}\" height=\"{h:.4}\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>\n"));
    }
    items_svg(&scene(doc, options)?, 1, &mut body, &mut 0);
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\" width=\"{w:.4}\" height=\"{h:.4}\" viewBox=\"{x:.4} {y:.4} {w:.4} {h:.4}\">\n{body}</svg>\n"
    ))
}

#[cfg(test)]
#[path = "export_tests.rs"]
pub(crate) mod tests;
