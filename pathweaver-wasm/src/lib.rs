//! JSON bridge between the web front end and `pathweaver-core`.
//!
//! The front end owns UI state and the undo history; every geometric or
//! document-structure decision is delegated here so the Rust core stays the
//! single source of truth for curves, widths, effects and validation.
//!
//! ABI: `pw_alloc(len) -> ptr`, write UTF-8 JSON `{"op": "...", ...}`, then
//! `pw_call(ptr, len) -> packed` where `packed = (out_ptr << 32) | out_len`.
//! The output is `{"ok": value}` or `{"err": "message"}`. Free both buffers
//! with `pw_free(ptr, len)`.

use kurbo::{Affine, Point, Vec2};
use pathweaver_core::{
    appearance::{self, Graph, Kind, Node as GraphNode, Parameter},
    corners,
    editing::{self, Anchor, AnchorKind, MergeTarget},
    model::{Corner, CornerKind, Document, Stroke, WidthTarget},
    outline, snapping,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize, Clone, Copy)]
struct A {
    stroke: u64,
    node: u64,
}
impl From<A> for Anchor {
    fn from(a: A) -> Self {
        Anchor { stroke: a.stroke, node: a.node }
    }
}
fn anchor_json(a: Anchor) -> Value {
    json!({"stroke": a.stroke, "node": a.node})
}

fn field<T: for<'de> Deserialize<'de>>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("引数 {key} が不正です: {e}"))
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn path_d(contours: &[Vec<Point>]) -> String {
    let mut d = String::new();
    for c in contours.iter().filter(|c| c.len() > 1) {
        for (i, p) in c.iter().enumerate() {
            d.push_str(if i == 0 { "M" } else { "L" });
            d.push_str(&format!("{:.3} {:.3}", p.x, p.y));
        }
        d.push('Z');
    }
    d
}

fn polyline_d(points: impl Iterator<Item = Point>) -> String {
    let mut d = String::new();
    for (i, p) in points.enumerate() {
        d.push_str(if i == 0 { "M" } else { "L" });
        d.push_str(&format!("{:.3} {:.3}", p.x, p.y));
    }
    d
}

fn target_json(t: WidthTarget) -> Value {
    match t {
        WidthTarget::Start => json!("start"),
        WidthTarget::End => json!("end"),
        WidthTarget::Key(id) => json!(id),
    }
}
fn parse_target(v: &Value) -> Result<WidthTarget, String> {
    match v {
        Value::String(s) if s == "start" => Ok(WidthTarget::Start),
        Value::String(s) if s == "end" => Ok(WidthTarget::End),
        Value::Number(n) => n.as_u64().map(WidthTarget::Key).ok_or("幅点IDが不正です".into()),
        _ => Err("幅点の指定が不正です".into()),
    }
}

/// Paint without the appearance graph: the "original" layers a graph receives.
fn original_layers(s: &Stroke) -> Result<Vec<appearance::Layer>, String> {
    let mut plain = s.clone();
    plain.appearance = None;
    Ok(outline::contours_only(|| outline::paint_layers(&plain))?
        .into_iter()
        .map(|l| appearance::Layer { color: l.color, opacity: l.opacity, contours: l.outline.contours, image: None })
        .collect())
}

fn render(s: &Stroke) -> Value {
    let center = if s.corners.is_empty() { s.clone() } else { corners::rounded(s) };
    let centerline = polyline_d(outline::samples(&center).into_iter().map(|p| p.p));
    // A guide is only its line: nothing painted, picked by its line.
    if s.guide {
        return json!({"layers": [], "hit": "", "center": centerline});
    }
    // A placed image: drawn by the canvas from the document's asset (the core
    // never sees the pixels here), picked by its rectangle.
    if let Some(img) = &s.image {
        let d = path_d(&[img.rect()]);
        return json!({
            "layers": [{"color": "#000000", "opacity": img.opacity, "d": d, "placed": {
                "asset": img.asset, "w": img.width, "h": img.height, "matrix": img.matrix, "reference": img.reference, "knockout": img.knockout,
            }}],
            "hit": d, "center": "",
        });
    }
    // Contours only: no triangulation and no union. Hit testing uses the
    // layers' own rings (non-zero winding), which covers the same area.
    match outline::contours_only(|| outline::paint_layers(s)) {
        Ok(layers) => {
            let ds: Vec<String> = layers.iter().map(|l| path_d(&l.outline.contours)).collect();
            json!({
                "layers": layers.iter().zip(&ds).map(|(l, d)| match &l.image {
                    // Bitmap from the raster stage: drawn as an image over its rectangle.
                    Some(img) => json!({
                        "color": hex(l.color), "opacity": l.opacity, "d": d,
                        "image": {"href": img.data_url(), "x": img.x0, "y": img.y0, "w": img.x1() - img.x0, "h": img.y1() - img.y0},
                    }),
                    None => json!({"color": hex(l.color), "opacity": l.opacity, "d": d}),
                }).collect::<Vec<_>>(),
                "hit": ds.concat(),
                "center": centerline,
            })
        }
        Err(e) => json!({"layers": [], "hit": "", "center": centerline, "error": e}),
    }
}

fn width_handles(s: &Stroke) -> Vec<Value> {
    s.width_targets()
        .into_iter()
        .filter_map(|t| {
            let (c, d) = outline::width_location(s, t)?;
            let w = s.width(t)?;
            Some(json!({
                "target": target_json(t), "center": c, "dir": d,
                "left": w.left, "right": w.right, "linked": w.linked, "angle": w.angle,
                "interpolated": matches!(t, WidthTarget::Key(id) if s.widths.iter().any(|k| k.id == id && k.interpolated)),
            }))
        })
        .collect()
}

/// Fix every width point at its current value. Points created as "follow the
/// profile" would otherwise move when a neighbouring width is edited, which
/// reads as the widths before/after being linked.
fn freeze_widths(s: &mut Stroke) {
    let ids: Vec<u64> = s.widths.iter().filter(|k| k.interpolated).map(|k| k.id).collect();
    for id in ids {
        s.width_mut(WidthTarget::Key(id));
    }
}

fn find_stroke<'a>(doc: &'a mut Document, id: u64) -> Result<&'a mut Stroke, String> {
    doc.strokes.iter_mut().find(|s| s.id == id).ok_or_else(|| "パスが見つかりません".to_string())
}

fn parameter_key(v: &Value) -> Result<Parameter, String> {
    serde_json::from_value(v.clone()).map_err(|_| "パラメーター名が不正です".into())
}

const KINDS: [Kind; 32] = [
    Kind::Input, Kind::Output, Kind::Reroute, Kind::Value, Kind::Math, Kind::Measure,
    Kind::Rough, Kind::Zigzag, Kind::Transform, Kind::Fill, Kind::Stack, Kind::Pathfinder,
    Kind::Boolean, Kind::Offset, Kind::Round, Kind::Band, Kind::Pucker, Kind::Simplify,
    Kind::Streak, Kind::Dissolve, Kind::Halftone, Kind::Mezzotint, Kind::Opacity, Kind::Shadow, Kind::Glow, Kind::Blur,
    Kind::Rasterize, Kind::RasterBlur, Kind::Threshold, Kind::Noise, Kind::Trace, Kind::Image,
];

fn node_info(n: &GraphNode) -> Value {
    json!({
        "label": n.kind.label(),
        "inputs": n.kind.inputs(),
        "number": n.kind.is_number(),
        "params": n.parameters().iter().map(|p| json!({
            "key": p.key, "label": p.label, "min": p.min, "max": p.max, "soft": p.soft.map(|(a, b)| [a, b]), "value": n.number(p.key),
        })).collect::<Vec<_>>(),
    })
}

pub fn dispatch(op: &str, a: &Value) -> Result<Value, String> {
    Ok(match op {
        "newDocument" => serde_json::to_value(Document::default()).unwrap(),
        "example" => serde_json::to_value(Document::example()).unwrap(),
        "load" => {
            let text: String = field(a, "text")?;
            let mut doc: Document = serde_json::from_str(&text).map_err(|e| format!("読み込めません: {e}"))?;
            doc.upgrade()?;
            doc.validate()?;
            serde_json::to_value(doc).unwrap()
        }
        "validate" => {
            let doc: Document = field(a, "doc")?;
            doc.validate()?;
            Value::Null
        }
        "save" => {
            let doc: Document = field(a, "doc")?;
            doc.validate()?;
            json!(serde_json::to_string_pretty(&doc).unwrap())
        }
        "svg" => {
            let doc: Document = field(a, "doc")?;
            json!(outline::svg(&doc)?)
        }
        "render" => render(&field::<Stroke>(a, "stroke")?),
        "widthHandles" => json!(width_handles(&field::<Stroke>(a, "stroke")?)),
        "nearest" => {
            let s: Stroke = field(a, "stroke")?;
            let p: Point = field(a, "p")?;
            match outline::nearest(&s, p) {
                Some((i, t, d)) => json!({"segment": i, "t": t, "distance": d, "point": s.eval(i, t), "tangent": s.tangent(i, t)}),
                None => Value::Null,
            }
        }
        "frames" => {
            // Snap frames for the given anchors, in request order.
            let doc: Document = field(a, "doc")?;
            let anchors: Vec<A> = field(a, "anchors")?;
            json!(anchors.iter().filter_map(|at| {
                let s = doc.strokes.iter().find(|s| s.id == at.stroke)?;
                let f = snapping::frame(s, at.node)?;
                Some(json!({"origin": f.origin, "tangent": f.tangent}))
            }).collect::<Vec<_>>())
        }
        "snap" | "snapTranslation" => {
            let doc: Document = field(a, "doc")?;
            let frames: Vec<Value> = field(a, "frames")?;
            let frames: Vec<snapping::Frame> = frames
                .iter()
                .filter_map(|f| {
                    Some(snapping::Frame {
                        origin: serde_json::from_value(f.get("origin")?.clone()).ok()?,
                        tangent: serde_json::from_value(f.get("tangent")?.clone()).ok()?,
                    })
                })
                .collect();
            let zoom: f64 = field(a, "zoom")?;
            let (p, guide) = if op == "snap" {
                snapping::snap(&doc, field(a, "p")?, &frames, zoom)
            } else {
                snapping::translation(&doc, field(a, "p")?, field(a, "start")?, field(a, "origin")?, &frames, zoom, false)
            };
            json!({"p": p, "guide": guide.map(|g| json!({
                "origin": g.origin, "direction": g.direction, "kind": format!("{:?}", g.kind),
            }))})
        }
        "edit" => {
            let mut doc: Document = field(a, "doc")?;
            let kind: String = field(a, "kind")?;
            let result = edit(&mut doc, &kind, a)?;
            json!({"doc": doc, "result": result})
        }
        "cornerWidgets" => {
            let s: Stroke = field(a, "stroke")?;
            let zoom: f64 = field(a, "zoom")?;
            json!((0..s.nodes.len()).filter_map(|i| {
                let (p, dir, _) = corners::widget(&s, i, zoom)?;
                let id = s.nodes[i].id;
                let c = s.corners.iter().find(|c| c.node == id);
                let kind = c.map_or(CornerKind::Circular, |c| c.kind);
                Some(json!({
                    "node": id, "p": p, "dir": dir, "radius": c.map_or(0., |c| c.radius),
                    "kind": kind, "limit": corners::radius_limit(&s, i, kind),
                }))
            }).collect::<Vec<_>>())
        }
        "kinds" => json!(KINDS.iter().map(|k| json!({
            "kind": k, "label": k.label(), "inputs": k.inputs(), "number": k.is_number(),
        })).collect::<Vec<_>>()),
        "nodeNew" => {
            let kind: Kind = field(a, "kind")?;
            let id: u64 = field(a, "id")?;
            let position: [f32; 2] = field(a, "position")?;
            serde_json::to_value(GraphNode::new(id, kind, position)).unwrap()
        }
        "nodeInfo" => node_info(&field::<GraphNode>(a, "node")?),
        "nodeSetNumber" => {
            let mut n: GraphNode = field(a, "node")?;
            n.set_number(parameter_key(a.get("key").unwrap_or(&Value::Null))?, field(a, "value")?);
            serde_json::to_value(n).unwrap()
        }
        "graphDefault" => serde_json::to_value(Graph::default()).unwrap(),
        "graphValidate" => {
            field::<Graph>(a, "graph")?.validate()?;
            Value::Null
        }
        "graphConnect" => {
            let mut g: Graph = field(a, "graph")?;
            let from: u64 = field(a, "from")?;
            let to: u64 = field(a, "to")?;
            match a.get("param") {
                Some(p) if !p.is_null() => g.connect_parameter(from, to, parameter_key(p)?)?,
                _ => g.connect(from, to, field(a, "port")?)?,
            }
            g.validate()?;
            serde_json::to_value(g).unwrap()
        }
        "graphRemove" => {
            let mut g: Graph = field(a, "graph")?;
            for id in field::<Vec<u64>>(a, "ids")? {
                g.remove(id);
            }
            serde_json::to_value(g).unwrap()
        }
        "parameterValues" => {
            let s: Stroke = field(a, "stroke")?;
            let Some(g) = &s.appearance else { return Ok(json!([])) };
            let values = appearance::parameter_values(g, &original_layers(&s)?)?;
            json!(values.into_iter().map(|((node, key), v)| json!({"node": node, "key": key, "value": v})).collect::<Vec<_>>())
        }
        // Fallback text outlines (browsers without the Mac host): the text
        // drawn by the page into a PNG, traced back into contours.
        "traceText" => {
            let png: String = field(a, "png")?;
            let png = png.trim_start_matches("data:image/png;base64,");
            let scale: f64 = field(a, "scale")?;
            let img = pathweaver_core::raster::picture(png, 0., 0., 0.)?;
            let w = img.width as f64;
            let img = pathweaver_core::raster::picture(png, 0., 0., w * scale)?;
            let layer = pathweaver_core::raster::trace(&img, 0.5, 0.25 * scale, 1, 0.5 * scale * scale)?;
            json!(layer.contours)
        }
        // Rings from an imported shape, made into normalized contours under
        // the shape's fill rule (SVG nonzero / evenodd).
        "normalizeRings" => {
            let rings: Vec<Vec<Point>> = field(a, "rings")?;
            let even: bool = field(a, "evenodd").unwrap_or(false);
            json!(outline::fill_rule_contours(&rings, even))
        }
        // Test hook: proves the host survives a panic inside the core.
        "debugPanic" => panic!("debugPanic requested"),
        _ => return Err(format!("未知の操作: {op}")),
    })
}

fn edit(doc: &mut Document, kind: &str, a: &Value) -> Result<Value, String> {
    let anchors = || -> Result<Vec<Anchor>, String> {
        Ok(field::<Vec<A>>(a, "anchors")?.into_iter().map(Anchor::from).collect())
    };
    Ok(match kind {
        "insertAnchor" => anchor_json(editing::insert_anchor(doc, field(a, "stroke")?, field(a, "segment")?, field(a, "t")?)?),
        "straighten" => {
            editing::straighten(doc, field(a, "stroke")?, field(a, "segmentNode")?)?;
            Value::Null
        }
        "anchorKind" => {
            let k = match field::<String>(a, "anchorKind")?.as_str() {
                "corner" => AnchorKind::Corner,
                "smooth" => AnchorKind::Smooth,
                _ => AnchorKind::Independent,
            };
            editing::set_anchor_kind(doc, &anchors()?, k)?;
            Value::Null
        }
        "separate" => {
            let at: A = field(a, "anchor")?;
            json!(editing::separate(doc, at.into())?.into_iter().map(anchor_json).collect::<Vec<_>>())
        }
        "join" => {
            let v = anchors()?;
            if v.len() != 2 {
                return Err("端点を2つ選択してください".into());
            }
            anchor_json(editing::join(doc, v[0], v[1])?)
        }
        "merge" => {
            let target = match field::<String>(a, "target")?.as_str() {
                "first" => MergeTarget::First,
                "last" => MergeTarget::Last,
                _ => MergeTarget::Center,
            };
            anchor_json(editing::merge_selected(doc, &anchors()?, target)?)
        }
        "deleteAnchors" => {
            editing::delete_anchors(doc, &anchors()?)?;
            Value::Null
        }
        "reverse" => {
            let s = find_stroke(doc, field(a, "stroke")?)?;
            *s = editing::reversed(s);
            doc.validate()?;
            Value::Null
        }
        "union" => json!(editing::union(doc, &field::<Vec<u64>>(a, "ids")?)?),
        "pathfinder" => json!(editing::pathfinder(doc, &field::<Vec<u64>>(a, "ids")?, field::<editing::PathfinderOp>(a, "pathfinder")?)?),
        "transform" => {
            // Affine on selected paths; widths scale with the shape.
            let ids: Vec<u64> = field(a, "ids")?;
            let m: [f64; 6] = field(a, "matrix")?;
            let m = Affine::new(m);
            for s in doc.strokes.iter_mut().filter(|s| ids.contains(&s.id)) {
                *s = s.transformed(m, true);
            }
            Value::Null
        }
        "insertWidth" => {
            let id = doc.id();
            let s = find_stroke(doc, field(a, "stroke")?)?;
            freeze_widths(s);
            if !outline::insert_width_key(s, id, field(a, "segment")?, field(a, "t")?) {
                return Err("幅点を追加できません".into());
            }
            freeze_widths(s); // the new point keeps its width when others change
            json!(id)
        }
        "setWidth" => {
            let s = find_stroke(doc, field(a, "stroke")?)?;
            freeze_widths(s);
            let target = parse_target(a.get("target").unwrap_or(&Value::Null))?;
            let w = s.width_mut(target).ok_or("幅点が見つかりません")?;
            if let Some(v) = a.get("left").and_then(Value::as_f64) {
                w.left = v.clamp(0., 1000.);
            }
            if let Some(v) = a.get("right").and_then(Value::as_f64) {
                w.right = v.clamp(0., 1000.);
            }
            if let Some(v) = a.get("linked").and_then(Value::as_bool) {
                w.linked = v;
            }
            if let Some(v) = a.get("angle").and_then(Value::as_f64) {
                w.angle = v.clamp(-89., 89.);
            }
            Value::Null
        }
        "deleteWidth" => {
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let id: u64 = field(a, "key")?;
            s.widths.retain(|k| k.id != id);
            Value::Null
        }
        "slideWidth" => {
            // Move a width point along the centerline to the point nearest `p`.
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let id: u64 = field(a, "key")?;
            let p: Point = field(a, "p")?;
            let (i, t, _) = outline::nearest(s, p).ok_or("パスが短すぎます")?;
            let segment = s.nodes[i].id;
            let k = s.widths.iter_mut().find(|k| k.id == id).ok_or("幅点が見つかりません")?;
            k.segment = segment;
            k.t = t.clamp(0.001, 0.999);
            doc.validate()?;
            Value::Null
        }
        "capAngle" => {
            // Lock (angle in degrees, world: 0 = horizontal) or unlock (null) an end cut.
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let end = field::<String>(a, "end")? == "end";
            let angle: Option<f64> = field(a, "angle")?;
            let (value, locked) = if end {
                (&mut s.end_angle, &mut s.end_cap_locked)
            } else {
                (&mut s.start_angle, &mut s.start_cap_locked)
            };
            match angle {
                Some(v) if v.is_finite() => {
                    *value = v.rem_euclid(180.);
                    *locked = true;
                }
                _ => *locked = false,
            }
            Value::Null
        }
        "capAngles" => {
            let s = find_stroke(doc, field(a, "stroke")?)?;
            if s.nodes.len() < 2 || s.closed {
                return Ok(Value::Null);
            }
            json!({
                "start": outline::cap_angle(s, false), "startLocked": s.start_cap_locked,
                "end": outline::cap_angle(s, true), "endLocked": s.end_cap_locked,
            })
        }
        "setCorner" => {
            // Live corner radius on anchors (0 removes it), clamped to what fits.
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let nodes: Vec<u64> = field(a, "nodes")?;
            let radius: f64 = field(a, "radius")?;
            let kind: Option<CornerKind> = field(a, "cornerKind").ok();
            for id in nodes {
                let Some(i) = s.nodes.iter().position(|n| n.id == id) else { continue };
                if !corners::eligible(s, i) {
                    continue;
                }
                let k = kind.or_else(|| s.corners.iter().find(|c| c.node == id).map(|c| c.kind)).unwrap_or_default();
                let r = radius.max(0.).min(corners::radius_limit(s, i, k));
                s.corners.retain(|c| c.node != id);
                if r > 1e-6 {
                    s.corners.push(Corner {node: id, radius: r, kind: k});
                }
            }
            doc.validate()?;
            Value::Null
        }
        "dragCorner" => {
            // The canvas widget sits 18 px inside the corner; dragging it along
            // the bisector sets the radius (same geometry as corners::widget).
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let id: u64 = field(a, "node")?;
            let p: Point = field(a, "p")?;
            let zoom: f64 = field(a, "zoom")?;
            let i = s.nodes.iter().position(|n| n.id == id).ok_or("アンカーが見つかりません")?;
            let (_, dir, factor) = corners::widget(s, i, zoom).ok_or("この点は角丸にできません")?;
            let kind = s.corners.iter().find(|c| c.node == id).map(|c| c.kind).unwrap_or_default();
            let along = (p - s.nodes[i].p).dot(dir) - 18. / zoom;
            let r = (along / factor).max(0.).min(corners::radius_limit(s, i, kind));
            s.corners.retain(|c| c.node != id);
            if r > 1e-6 {
                s.corners.push(Corner {node: id, radius: r, kind});
            }
            json!(r)
        }
        "lineWidth" => {
            find_stroke(doc, field(a, "stroke")?)?.resize_line_width(field(a, "value")?)?;
            Value::Null
        }
        "moveWidthHandle" => {
            // Same rule as the first PathWeaver (e4c9924): a normal drag adds
            // the same amount to both sides relative to the width at drag start
            // (`base`), keeping any asymmetry; `independent` (Option) changes
            // only the dragged side. The `linked` flag does not affect drags.
            let s = find_stroke(doc, field(a, "stroke")?)?;
            let target = parse_target(a.get("target").unwrap_or(&Value::Null))?;
            let p: Point = field(a, "p")?;
            let left = field::<String>(a, "side")? == "left";
            let independent: bool = field(a, "independent").unwrap_or(false);
            freeze_widths(s);
            let (c, d) = outline::width_location(s, target).ok_or("幅点が見つかりません")?;
            let value = ((p - c).dot(d) * if left { 1. } else { -1. }).clamp(0., 1000.);
            let current = s.width(target).ok_or("幅点が見つかりません")?;
            let base = match a.get("base") {
                Some(b) if !b.is_null() => (
                    b.get("left").and_then(Value::as_f64).unwrap_or(current.left),
                    b.get("right").and_then(Value::as_f64).unwrap_or(current.right),
                ),
                _ => (current.left, current.right),
            };
            let w = s.width_mut(target).ok_or("幅点が見つかりません")?;
            w.left = base.0;
            w.right = base.1;
            if independent {
                if left { w.left = value } else { w.right = value }
            } else {
                let delta = value - if left { base.0 } else { base.1 };
                w.left = (base.0 + delta).clamp(0., 1000.);
                w.right = (base.1 + delta).clamp(0., 1000.);
            }
            json!({"left": w.left, "right": w.right})
        }
        _ => return Err(format!("未知の編集: {kind}")),
    })
}

// ---- C ABI -----------------------------------------------------------------

/// With panic=abort a panic traps the instance and its message is lost. The
/// hook copies it into a fixed buffer the host can read after the trap, then
/// the host discards this instance and starts a fresh one.
static mut PANIC_BUF: [u8; 1024] = [0; 1024];
static mut PANIC_LEN: usize = 0;

#[no_mangle]
pub extern "C" fn pw_panic_buf() -> *const u8 {
    std::ptr::addr_of!(PANIC_BUF) as *const u8
}
#[no_mangle]
pub extern "C" fn pw_panic_len_ptr() -> *const usize {
    std::ptr::addr_of!(PANIC_LEN)
}
fn install_panic_hook() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            let bytes = msg.as_bytes();
            let n = bytes.len().min(1024);
            unsafe {
                let buf = &mut *std::ptr::addr_of_mut!(PANIC_BUF);
                buf[..n].copy_from_slice(&bytes[..n]);
                *std::ptr::addr_of_mut!(PANIC_LEN) = n;
            }
        }));
    });
}

#[no_mangle]
pub extern "C" fn pw_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr`/`len` must come from `pw_alloc` or a `pw_call` result.
#[no_mangle]
pub unsafe extern "C" fn pw_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// # Safety
/// `ptr` must point to `len` initialized bytes allocated with `pw_alloc`.
#[no_mangle]
pub unsafe extern "C" fn pw_call(ptr: *const u8, len: usize) -> u64 {
    install_panic_hook();
    let input = std::slice::from_raw_parts(ptr, len);
    let out = call_json(input);
    let mut bytes = out.into_bytes();
    bytes.shrink_to_fit();
    let (p, l) = (bytes.as_mut_ptr(), bytes.len());
    std::mem::forget(bytes);
    ((p as u64) << 32) | l as u64
}

pub fn call_json(input: &[u8]) -> String {
    let parsed: Result<Value, _> = serde_json::from_slice(input);
    let result = match parsed {
        Ok(v) => {
            let op = v.get("op").and_then(Value::as_str).unwrap_or("").to_string();
            dispatch(&op, &v)
        }
        Err(e) => Err(format!("JSONが不正です: {e}")),
    };
    match result {
        Ok(v) => json!({"ok": v}).to_string(),
        Err(e) => json!({"err": e}).to_string(),
    }
}

#[allow(dead_code)]
fn _unused(_: Vec2) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(v: Value) -> Value {
        let out: Value = serde_json::from_str(&call_json(v.to_string().as_bytes())).unwrap();
        if let Some(e) = out.get("err") {
            panic!("{e}");
        }
        out["ok"].clone()
    }

    #[test]
    fn renders_example_and_round_trips() {
        let doc = call(json!({"op": "example"}));
        for s in doc["strokes"].as_array().unwrap() {
            let r = call(json!({"op": "render", "stroke": s}));
            assert!(r.get("error").is_none(), "{r}");
            assert!(!r["hit"].as_str().unwrap().is_empty());
        }
        let text = call(json!({"op": "save", "doc": doc}));
        call(json!({"op": "load", "text": text}));
    }

    #[test]
    fn insert_anchor_keeps_shape_and_adds_node() {
        let doc = call(json!({"op": "example"}));
        let s = &doc["strokes"][0];
        let id = s["id"].as_u64().unwrap();
        let before = s["nodes"].as_array().unwrap().len();
        let r = call(json!({"op": "edit", "kind": "insertAnchor", "doc": doc, "stroke": id, "segment": 0, "t": 0.5}));
        let after = r["doc"]["strokes"][0]["nodes"].as_array().unwrap().len();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn width_drag_adds_to_both_sides_and_option_changes_one() {
        let doc = call(json!({"op": "example"}));
        let id = doc["strokes"][0]["id"].as_u64().unwrap();
        // make the start asymmetric: left 10, right 30
        let doc = call(json!({"op": "edit", "kind": "setWidth", "doc": doc, "stroke": id, "target": "start", "left": 10.0, "right": 30.0, "linked": true}))["doc"].clone();
        let h = call(json!({"op": "widthHandles", "stroke": doc["strokes"][0]}))[0].clone();
        let (cx, cy, dx, dy) = (h["center"]["x"].as_f64().unwrap(), h["center"]["y"].as_f64().unwrap(), h["dir"]["x"].as_f64().unwrap(), h["dir"]["y"].as_f64().unwrap());
        let at = |v: f64| json!({"x": cx + dx * v, "y": cy + dy * v});
        let base = json!({"left": 10.0, "right": 30.0});
        // normal drag of the left edge to 25: +15 on both sides (linked flag is irrelevant)
        let r = call(json!({"op": "edit", "kind": "moveWidthHandle", "doc": doc, "stroke": id, "target": "start", "side": "left", "p": at(25.), "base": base}));
        let close = |v: &Value, l: f64, r: f64| (v["left"].as_f64().unwrap() - l).abs() < 1e-6 && (v["right"].as_f64().unwrap() - r).abs() < 1e-6;
        assert!(close(&r["result"], 25., 45.), "{}", r["result"]);
        // Option: left only
        let r = call(json!({"op": "edit", "kind": "moveWidthHandle", "doc": doc, "stroke": id, "target": "start", "side": "left", "p": at(25.), "base": base, "independent": true}));
        assert!(close(&r["result"], 25., 30.), "{}", r["result"]);
    }
    #[test]
    fn editing_one_width_never_moves_another_width_point() {
        let doc = call(json!({"op": "example"}));
        let id = doc["strokes"][0]["id"].as_u64().unwrap();
        let r = call(json!({"op": "edit", "kind": "insertWidth", "doc": doc, "stroke": id, "segment": 0, "t": 0.5}));
        let key = r["result"].as_u64().unwrap();
        let doc = r["doc"].clone();
        let width_of = |doc: &Value| {
            let hs = call(json!({"op": "widthHandles", "stroke": doc["strokes"][0]}));
            let h = hs.as_array().unwrap().iter().find(|h| h["target"] == json!(key)).unwrap().clone();
            (h["left"].as_f64().unwrap(), h["right"].as_f64().unwrap())
        };
        let before = width_of(&doc);
        for (kind, extra) in [
            ("setWidth", json!({"target": "start", "left": 80.0, "right": 80.0})),
            ("setWidth", json!({"target": "end", "left": 1.0, "right": 1.0})),
        ] {
            let mut args = json!({"op": "edit", "kind": kind, "doc": doc, "stroke": id});
            for (k, v) in extra.as_object().unwrap() { args[k] = v.clone(); }
            let r = call(args);
            let after = width_of(&r["doc"]);
            assert!((after.0 - before.0).abs() < 1e-6 && (after.1 - before.1).abs() < 1e-6, "{kind} moved the untouched point: {before:?} -> {after:?}");
        }
    }
    #[test]
    fn node_set_number_clamps() {
        let n = call(json!({"op": "nodeNew", "kind": "Rough", "id": 5, "position": [0, 0]}));
        let n = call(json!({"op": "nodeSetNumber", "node": n, "key": "Amount", "value": 99999}));
        let info = call(json!({"op": "nodeInfo", "node": n}));
        assert_eq!(info["params"][0]["value"], json!(1000.0));
    }
}
