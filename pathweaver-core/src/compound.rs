//! Compound paths (Illustrator's compound path).
//!
//! A compound path is not a new kind of object: it is ordinary paths tied by
//! the same `Stroke::compound` id. Every member keeps its anchors and handles,
//! so the direct-selection tool, the pen, corners, transforms and undo work on
//! a member exactly as on any path. What makes them one shape is painting:
//! the members' centerlines are filled together by `fill_rule` (holes where a
//! member lies inside another) with the first member's look — fill, line,
//! appearance graph. The editor keeps those settings the same on every member.
//!
//! Polygon results (Pathfinder, old flattened outlines, traced text) come back
//! as members through `fit_ring`: sharp turns stay corners, the smooth runs
//! between them are fitted with as few cubic segments as the accuracy allows.
use crate::{
    corners,
    model::{Document, FillRule, Node, Stroke},
    outline,
};
use kurbo::{
    simplify::SimplifyBezPath, BezPath, CubicBez, PathEl, Point, Vec2,
};
use std::borrow::Cow;
use std::collections::HashMap;

/// The ring a member adds to its compound's fill: its centerline (live
/// corners applied), closed even when the path itself is open.
pub fn ring(s: &Stroke) -> Vec<Point> {
    let c = if s.corners.is_empty() { Cow::Borrowed(s) } else { Cow::Owned(corners::rounded(s)) };
    let mut pts: Vec<Point> = outline::samples(&c).into_iter().map(|p| p.p).collect();
    if pts.len() > 1 && pts[0].distance(*pts.last().unwrap()) < 1e-9 {
        pts.pop();
    }
    pts
}

/// What a compound paints: the first member's look over the members' rings
/// combined by its fill rule. The result keeps the first member's own nodes,
/// so its centerline (the selection outline) is still that member's.
pub fn shape(members: &[&Stroke]) -> Stroke {
    let head = members[0];
    let mut s = head.clone();
    let rings: Vec<Vec<Point>> = members.iter().map(|m| ring(m)).filter(|r| r.len() > 2).collect();
    s.contours = Some(outline::fill_rule_contours(&rings, head.fill_rule == FillRule::EvenOdd));
    s.compound = None;
    s
}

/// Members of each compound in stacking order.
pub fn members(doc: &Document) -> HashMap<u64, Vec<&Stroke>> {
    let mut m: HashMap<u64, Vec<&Stroke>> = HashMap::new();
    for s in &doc.strokes {
        if let Some(c) = s.compound {
            m.entry(c).or_default().push(s);
        }
    }
    m
}

/// What the document paints, back to front: shown paths, with each compound
/// collapsed into one shape at its first shown member's place.
pub fn painted(doc: &Document) -> Vec<Cow<'_, Stroke>> {
    let mut groups: HashMap<u64, Vec<&Stroke>> = HashMap::new();
    for s in doc.strokes.iter().filter(|s| doc.shown(s) && !s.guide) {
        if let Some(c) = s.compound {
            groups.entry(c).or_default().push(s);
        }
    }
    let mut out = vec![];
    for s in doc.strokes.iter().filter(|s| doc.shown(s) && !s.guide) {
        match s.compound {
            None => out.push(Cow::Borrowed(s)),
            Some(c) => {
                if let Some(list) = groups.remove(&c) {
                    out.push(Cow::Owned(shape(&list)));
                }
            }
        }
    }
    out
}

/// An anchor of a fitted ring (no id yet).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct FitAnchor {
    pub p: Point,
    pub incoming: Vec2,
    pub outgoing: Vec2,
    pub smooth: bool,
}

/// A turn sharper than this (between neighbouring polygon edges) stays a corner.
const CORNER_TURN: f64 = 32. * std::f64::consts::PI / 180.;

fn unit(v: Vec2) -> Vec2 {
    let l = v.hypot();
    if l > 1e-12 { v / l } else { Vec2::ZERO }
}
fn turn(a: Point, b: Point, c: Point) -> f64 {
    let (u, v) = (b - a, c - b);
    u.cross(v).atan2(u.dot(v)).abs()
}

/// A smooth curve through the points (tangent at each point along its
/// neighbours, handles a third of each edge), as cubics. `ends`: the
/// tangents at the first and last point (open runs between corners).
fn spline(q: &[Point], closed: bool) -> BezPath {
    let n = q.len();
    let tangent = |i: usize| -> Vec2 {
        if closed {
            let t = unit(q[(i + 1) % n] - q[(i + n - 1) % n]);
            if t != Vec2::ZERO { return t; }
            return unit(q[(i + 1) % n] - q[i]);
        }
        let t = if i == 0 {
            q[1] - q[0]
        } else if i == n - 1 {
            q[n - 1] - q[n - 2]
        } else {
            q[i + 1] - q[i - 1]
        };
        unit(t)
    };
    let mut path = BezPath::new();
    path.move_to(q[0]);
    let count = if closed { n } else { n - 1 };
    for i in 0..count {
        let (a, b) = (q[i], q[(i + 1) % n]);
        let l = a.distance(b) / 3.;
        path.curve_to(a + tangent(i) * l, b - tangent((i + 1) % n) * l, b);
    }
    path
}

/// Fit a smooth run with few cubics (Fréchet error about `accuracy`).
fn fit_run(q: &[Point], closed: bool, accuracy: f64) -> Vec<CubicBez> {
    // A straight run is one line.
    if !closed {
        let (a, b) = (q[0], *q.last().unwrap());
        let d = unit(b - a);
        if d != Vec2::ZERO && q.iter().all(|p| (*p - a).cross(d).abs() <= accuracy * 0.5) {
            return vec![CubicBez::new(a, a, b, b)];
        }
    }
    let source = SimplifyBezPath::new(spline(q, closed));
    // The optimizing fit gives the fewest anchors; it is slower, so very
    // long runs take the subdividing one.
    let fitted = if q.len() <= 4000 {
        kurbo::fit_to_bezpath_opt(&source, accuracy)
    } else {
        kurbo::fit_to_bezpath(&source, accuracy)
    };
    let mut out = vec![];
    let mut last = q[0];
    for el in fitted.elements() {
        match *el {
            PathEl::MoveTo(p) => last = p,
            PathEl::LineTo(p) => { out.push(CubicBez::new(last, last, p, p)); last = p; }
            PathEl::QuadTo(c, p) => {
                out.push(CubicBez::new(last, last + (c - last) * (2. / 3.), p + (c - p) * (2. / 3.), p));
                last = p;
            }
            PathEl::CurveTo(a, b, p) => { out.push(CubicBez::new(last, a, b, p)); last = p; }
            PathEl::ClosePath => {}
        }
    }
    // The fit may end a hair away from the run's end; anchors must meet.
    if let Some(c) = out.last_mut() {
        let end = if closed { q[0] } else { *q.last().unwrap() };
        let d = end - c.p3;
        c.p3 = end;
        c.p2 += d;
    }
    if let Some(c) = out.first_mut() {
        let d = q[0] - c.p0;
        c.p0 = q[0];
        c.p1 += d;
    }
    out
}

/// A closed polygon as a closed Bézier path: corners where the polygon turns
/// sharply, smooth fitted curves between them.
pub fn fit_ring(ring: &[Point], accuracy: f64) -> Vec<FitAnchor> {
    let accuracy = if accuracy.is_finite() && accuracy > 0. { accuracy } else { 0.1 };
    // Drop repeated points (and the repeated start).
    let eps = (accuracy * 0.05).max(1e-9);
    let mut q: Vec<Point> = vec![];
    for p in ring {
        if q.last().map_or(true, |l: &Point| l.distance(*p) > eps) {
            q.push(*p);
        }
    }
    while q.len() > 1 && q[0].distance(*q.last().unwrap()) <= eps {
        q.pop();
    }
    let n = q.len();
    if n < 3 {
        return q.iter().map(|p| FitAnchor { p: *p, incoming: Vec2::ZERO, outgoing: Vec2::ZERO, smooth: false }).collect();
    }
    let corners: Vec<usize> = (0..n).filter(|&i| turn(q[(i + n - 1) % n], q[i], q[(i + 1) % n]) > CORNER_TURN).collect();
    let mut segs: Vec<CubicBez> = vec![];
    let mut corner_at_start = vec![];
    if corners.is_empty() {
        segs = fit_run(&q, true, accuracy);
        corner_at_start.resize(segs.len(), false);
    } else {
        for (k, &c) in corners.iter().enumerate() {
            let next = corners[(k + 1) % corners.len()];
            let mut run = vec![];
            let mut i = c;
            loop {
                run.push(q[i]);
                if i == next && run.len() > 1 { break; }
                i = (i + 1) % n;
                if i == next {
                    run.push(q[i]);
                    break;
                }
            }
            let fitted = fit_run(&run, false, accuracy);
            for (j, _) in fitted.iter().enumerate() {
                corner_at_start.push(j == 0);
            }
            segs.extend(fitted);
        }
    }
    let m = segs.len();
    let mut out: Vec<FitAnchor> = segs
        .iter()
        .map(|c| FitAnchor { p: c.p0, incoming: Vec2::ZERO, outgoing: c.p1 - c.p0, smooth: false })
        .collect();
    for (k, c) in segs.iter().enumerate() {
        out[(k + 1) % m].incoming = c.p2 - c.p3;
    }
    for (k, a) in out.iter_mut().enumerate() {
        let (i, o) = (a.incoming, a.outgoing);
        let tiny = i.hypot() < 1e-9 || o.hypot() < 1e-9;
        a.smooth = !corner_at_start[k] && !tiny && i.cross(o).abs() <= 1e-3 * i.hypot() * o.hypot() && i.dot(o) < 0.;
        if !corner_at_start[k] && !tiny && !a.smooth {
            // A fitted join is G1 up to rounding: make it exactly smooth.
            let d = unit(o - i);
            a.incoming = d * -i.hypot();
            a.outgoing = d * o.hypot();
            a.smooth = true;
        }
    }
    out
}

/// Paths made from closed polygons (one per ring) in the look of `template`.
/// More than one ring makes them a compound path with `rule`. The first path
/// takes `first_id` when given.
pub fn paths_from_rings(
    d: &mut Document,
    rings: &[Vec<Point>],
    template: &Stroke,
    rule: FillRule,
    first_id: Option<u64>,
    accuracy: f64,
) -> Vec<Stroke> {
    let fitted: Vec<Vec<FitAnchor>> = rings.iter().map(|r| fit_ring(r, accuracy)).filter(|a| a.len() > 1).collect();
    let compound = if fitted.len() > 1 { Some(d.id()) } else { None };
    let mut out = vec![];
    for (k, anchors) in fitted.into_iter().enumerate() {
        let id = match (k, first_id) {
            (0, Some(id)) => id,
            _ => d.id(),
        };
        let mut s = styled(template, id, if k == 0 { template.name.clone() } else { format!("{} {}", template.name, k + 1) });
        s.closed = true;
        s.compound = compound;
        s.fill_rule = if compound.is_some() { rule } else { FillRule::NonZero };
        s.nodes = anchors
            .into_iter()
            .map(|a| Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: d.id(),
                p: a.p,
                incoming: a.incoming,
                outgoing: a.outgoing,
                smooth: a.smooth,
            })
            .collect();
        out.push(s);
    }
    out
}

/// A fresh path with the look (paint, line, appearance, place) of `t`.
fn styled(t: &Stroke, id: u64, name: String) -> Stroke {
    let mut s = Stroke::new(id, name);
    s.fill = t.fill;
    s.edge = t.edge;
    s.join = t.join;
    s.miter_limit = t.miter_limit;
    s.appearance = t.appearance.clone();
    s.folder = t.folder;
    s.visible = t.visible;
    s.set_line_width(t.edge_width.clamp(0., 2000.));
    s.edge_width = t.edge_width;
    s
}

/// Copy the look shared by a compound's members from `from` onto `to`.
fn share_look(from: &Stroke, to: &mut Stroke) {
    to.fill = from.fill;
    to.edge = from.edge;
    to.edge_width = from.edge_width;
    to.join = from.join;
    to.miter_limit = from.miter_limit;
    to.appearance = from.appearance.clone();
    to.fill_rule = from.fill_rule;
    to.folder = from.folder;
    to.visible = from.visible;
}

/// Polygon accuracy for converting old flattened outlines.
pub const ACCURACY: f64 = 0.1;

/// Object → Compound Path → Make (⌘8): the selected paths (whole compounds
/// when any member is selected) become one compound path, in the place and
/// with the look of the front-most one. Old flattened shapes (contours) are
/// turned into editable paths on the way, so a single one of them can be
/// "made" just to make it editable. Returns the ids of the members (a single
/// resulting path is left a plain path).
pub fn make(doc: &mut Document, ids: &[u64]) -> Result<Vec<u64>, String> {
    let mut next = doc.clone();
    let d = &mut next;
    let touched: Vec<u64> = d.strokes.iter().filter(|s| ids.contains(&s.id)).filter_map(|s| s.compound).collect();
    let picked = |s: &Stroke| ids.contains(&s.id) || s.compound.is_some_and(|c| touched.contains(&c));
    let order: Vec<usize> = (0..d.strokes.len()).filter(|&i| picked(&d.strokes[i])).collect();
    if order.is_empty() {
        return Err("複合パスにするパスを選んでください".into());
    }
    for &i in &order {
        let s = &d.strokes[i];
        if s.image.is_some() {
            return Err("配置画像は複合パスにできません".into());
        }
        if s.text.is_some() {
            return Err("文字は先にアウトライン化してください（⇧⌘O）".into());
        }
        if s.guide {
            return Err("ガイドは複合パスにできません".into());
        }
        if s.blend_of.is_some() || d.blends.iter().any(|b| b.a == s.id || b.b == s.id) {
            return Err("ブレンドに使われているパスは複合パスにできません".into());
        }
    }
    let legacy = order.iter().any(|&i| d.strokes[i].contours.is_some());
    let compounds: Vec<u64> = {
        let mut v: Vec<u64> = order.iter().filter_map(|&i| d.strokes[i].compound).collect();
        v.dedup();
        v
    };
    let units = order.iter().filter(|&&i| d.strokes[i].compound.is_none()).count() + compounds.len();
    if units < 2 && !legacy {
        return Err("複合パスにするパスを2つ以上選んでください".into());
    }
    let front = *order.last().unwrap();
    let template = d.strokes[front].clone();
    // A compound already in the selection keeps its fill rule (text needs
    // nonzero); otherwise any path inside another becomes a hole.
    let rule = order.iter().rev().find_map(|&i| d.strokes[i].compound.map(|_| d.strokes[i].fill_rule)).unwrap_or(FillRule::EvenOdd);
    let id = d.id();
    let mut members = vec![];
    for &i in &order {
        let s = d.strokes[i].clone();
        if let Some(rings) = &s.contours {
            let mut t = s.clone();
            t.fill = template.fill;
            t.edge = template.edge;
            members.extend(paths_from_rings(d, rings, &t, rule, Some(s.id), ACCURACY));
        } else {
            members.push(s);
        }
    }
    let head_name = if template.compound.is_some() {
        d.strokes.iter().find(|s| s.compound == template.compound).map(|s| s.name.clone()).unwrap_or_default()
    } else if units == 1 {
        template.name.clone()
    } else {
        "複合パス".into()
    };
    let mut look = template.clone();
    look.fill_rule = rule;
    if template.contours.is_some() {
        // An old flattened shape has no line profile; its line is edge_width.
        look.edge_width = template.edge_width;
    }
    for (k, m) in members.iter_mut().enumerate() {
        share_look(&look, m);
        m.compound = Some(id);
        m.closed = m.closed || m.nodes.len() > 2;
        if k == 0 {
            m.name = head_name.clone();
        }
    }
    members.retain(|m| m.nodes.len() > 1);
    if members.is_empty() {
        return Err("複合パスにできる形がありません".into());
    }
    if members.len() == 1 {
        members[0].compound = None;
        members[0].fill_rule = FillRule::NonZero;
    }
    let out = members.iter().map(|m| m.id).collect();
    let at = (0..front).filter(|&i| !picked(&d.strokes[i])).count();
    let mut rest: Vec<Stroke> = d.strokes.iter().filter(|s| !picked(s)).cloned().collect();
    rest.splice(at..at, members);
    d.strokes = rest;
    next.validate()?;
    *doc = next;
    Ok(out)
}

/// Object → Compound Path → Release (⌥⇧⌘8): the members of the selected
/// compounds become separate paths again (each keeps the shared look).
/// Returns their ids.
pub fn release(doc: &mut Document, ids: &[u64]) -> Result<Vec<u64>, String> {
    let touched: Vec<u64> = doc.strokes.iter().filter(|s| ids.contains(&s.id)).filter_map(|s| s.compound).collect();
    if touched.is_empty() {
        return Err("解除する複合パスを選んでください".into());
    }
    let mut next = doc.clone();
    let mut out = vec![];
    for s in next.strokes.iter_mut().filter(|s| s.compound.is_some_and(|c| touched.contains(&c))) {
        s.compound = None;
        s.fill_rule = FillRule::NonZero;
        out.push(s.id);
    }
    next.validate()?;
    *doc = next;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(c: Point, r: f64, n: usize) -> Vec<Point> {
        (0..n).map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            c + Vec2::new(a.cos(), a.sin()) * r
        }).collect()
    }
    fn square(x: f64, y: f64, w: f64, step: f64) -> Vec<Point> {
        // dense, like the core's flattened outlines
        let mut v = vec![];
        let corners = [Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + w), Point::new(x, y + w)];
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let n = (a.distance(b) / step).ceil() as usize;
            for i in 0..n { v.push(a.lerp(b, i as f64 / n as f64)); }
        }
        v
    }
    fn dev(anchors: &[FitAnchor], ring: &[Point]) -> f64 {
        // farthest polygon point from the fitted curve
        let mut s = Stroke::new(1, String::new());
        s.closed = true;
        s.nodes = anchors.iter().enumerate().map(|(i, a)| Node {
            exact_incoming: false, exact_outgoing: false, id: i as u64 + 1, p: a.p, incoming: a.incoming, outgoing: a.outgoing, smooth: a.smooth,
        }).collect();
        let pts: Vec<Point> = outline::samples(&s).into_iter().map(|p| p.p).collect();
        let seg = |p: Point, a: Point, b: Point| {
            let v = b - a;
            let t = ((p - a).dot(v) / v.hypot2().max(1e-12)).clamp(0., 1.);
            p.distance(a + v * t)
        };
        ring.iter()
            .map(|p| (0..pts.len()).map(|i| seg(*p, pts[i], pts[(i + 1) % pts.len()])).fold(f64::INFINITY, f64::min))
            .fold(0., f64::max)
    }

    #[test]
    fn dense_square_keeps_four_sharp_corners() {
        let ring = square(0., 0., 100., 1.);
        let a = fit_ring(&ring, 0.1);
        assert_eq!(a.len(), 4, "{a:?}");
        assert!(a.iter().all(|a| !a.smooth && a.incoming == Vec2::ZERO && a.outgoing == Vec2::ZERO));
    }

    #[test]
    fn dense_circle_becomes_a_few_smooth_anchors() {
        let ring = circle(Point::new(50., 50.), 40., 400);
        let a = fit_ring(&ring, 0.1);
        assert!(a.len() >= 2 && a.len() <= 8, "{} anchors", a.len());
        assert!(a.iter().all(|a| a.smooth));
        assert!(dev(&a, &ring) < 0.3, "deviation {}", dev(&a, &ring));
    }

    #[test]
    fn mixed_corner_and_arc() {
        // a D shape: straight side + half circle
        let mut ring = vec![];
        for i in 0..=200 {
            let a = -std::f64::consts::FRAC_PI_2 + i as f64 / 200. * std::f64::consts::PI;
            ring.push(Point::new(a.cos() * 50., a.sin() * 50.));
        }
        for i in 1..100 {
            ring.push(Point::new(0., 50. - i as f64));
        }
        let a = fit_ring(&ring, 0.1);
        assert!(a.len() <= 6, "{} anchors", a.len());
        assert_eq!(a.iter().filter(|a| !a.smooth).count(), 2);
        assert!(dev(&a, &ring) < 0.3, "deviation {}", dev(&a, &ring));
    }

    fn doc_with(rings: &[Vec<Point>]) -> Document {
        let mut d = Document::default();
        for r in rings {
            let mut s = Stroke::new(d.id(), "形".into());
            s.closed = true;
            s.fill = Some([10, 20, 30]);
            s.edge = None;
            s.nodes = r.iter().map(|p| Node { exact_incoming: false, exact_outgoing: false, id: 0, p: *p, incoming: Vec2::ZERO, outgoing: Vec2::ZERO, smooth: false }).collect();
            for n in &mut s.nodes { n.id = d.id(); }
            d.strokes.push(s);
        }
        d
    }
    fn filled(d: &Document, p: Point) -> bool {
        let painted = painted(d);
        painted.iter().any(|s| {
            let o = outline::paint(s).unwrap();
            let mut w = 0;
            for c in &o.contours {
                for k in 0..c.len() {
                    let (a, b) = (c[k], c[(k + 1) % c.len()]);
                    if (a.y <= p.y) != (b.y <= p.y) {
                        let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                        if x > p.x { w += if b.y > a.y { 1 } else { -1 }; }
                    }
                }
            }
            w != 0
        })
    }

    #[test]
    fn make_cuts_a_hole_and_release_gives_it_back() {
        let outer = vec![Point::new(0., 0.), Point::new(100., 0.), Point::new(100., 100.), Point::new(0., 100.)];
        let inner = vec![Point::new(30., 30.), Point::new(70., 30.), Point::new(70., 70.), Point::new(30., 70.)];
        let mut d = doc_with(&[outer, inner]);
        let ids: Vec<u64> = d.strokes.iter().map(|s| s.id).collect();
        d.strokes[1].fill = Some([200, 0, 0]);
        assert!(filled(&d, Point::new(50., 50.)));
        let c = make(&mut d, &ids[1..]).unwrap_err();
        assert!(c.contains("2つ以上"));
        let made = make(&mut d, &ids).unwrap();
        assert_eq!(made.len(), 2);
        let c = d.strokes[0].compound.unwrap();
        assert!(d.strokes.iter().all(|s| s.compound == Some(c)));
        // the front-most path's look, even-odd: the inner square is a hole
        assert!(d.strokes.iter().all(|s| s.fill == Some([200, 0, 0]) && s.fill_rule == FillRule::EvenOdd));
        assert!(!filled(&d, Point::new(50., 50.)));
        assert!(filled(&d, Point::new(10., 50.)));
        // members stay ordinary editable paths
        d.validate().unwrap();
        let svg = outline::svg(&d).unwrap();
        assert_eq!(svg.matches("<path").count(), 1, "one shape: {svg}");
        // round trip
        let back: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        release(&mut d, &[ids[0]]).unwrap();
        assert!(d.strokes.iter().all(|s| s.compound.is_none()));
        assert!(filled(&d, Point::new(50., 50.)));
    }

    #[test]
    fn pathfinder_takes_a_compound_whole_and_keeps_its_hole() {
        let outer = vec![Point::new(0., 0.), Point::new(100., 0.), Point::new(100., 100.), Point::new(0., 100.)];
        let inner = vec![Point::new(30., 30.), Point::new(70., 30.), Point::new(70., 70.), Point::new(30., 70.)];
        let side = vec![Point::new(90., 40.), Point::new(150., 40.), Point::new(150., 60.), Point::new(90., 60.)];
        let mut d = doc_with(&[outer, inner, side]);
        let ids: Vec<u64> = d.strokes.iter().map(|s| s.id).collect();
        make(&mut d, &ids[..2]).unwrap();
        // only one member named: the whole compound takes part
        let out = crate::editing::pathfinder(&mut d, &[ids[0], ids[2]], crate::editing::PathfinderOp::Unite).unwrap();
        assert_eq!(out.len(), 2, "outline + hole");
        assert!(d.strokes.iter().all(|s| s.contours.is_none() && s.compound.is_some()));
        assert!(!filled(&d, Point::new(50., 50.)), "the hole survives");
        assert!(filled(&d, Point::new(140., 50.)));
        // a transform moves every member (and so the shape) together
        let moved: Vec<Stroke> = d.strokes.iter().map(|s| s.transformed(kurbo::Affine::translate((1000., 0.)), true)).collect();
        d.strokes = moved;
        assert!(!filled(&d, Point::new(1050., 50.)) && filled(&d, Point::new(1010., 50.)));
        d.validate().unwrap();
    }

    #[test]
    fn appearance_runs_on_the_whole_compound() {
        let outer = vec![Point::new(0., 0.), Point::new(100., 0.), Point::new(100., 100.), Point::new(0., 100.)];
        let inner = vec![Point::new(30., 30.), Point::new(70., 30.), Point::new(70., 70.), Point::new(30., 70.)];
        let mut d = doc_with(&[outer, inner]);
        let ids: Vec<u64> = d.strokes.iter().map(|s| s.id).collect();
        make(&mut d, &ids).unwrap();
        let g = crate::appearance::Graph::default();
        for s in &mut d.strokes {
            s.appearance = Some(g.clone());
        }
        let p = painted(&d);
        assert_eq!(p.len(), 1);
        let layers = outline::paint_layers(&p[0]).unwrap();
        assert!(!layers.is_empty());
        assert!(!filled(&d, Point::new(50., 50.)));
    }

    #[test]
    fn old_flattened_shape_becomes_editable() {
        let mut d = Document::default();
        let mut s = Stroke::new(d.id(), "古い形".into());
        s.fill = Some([1, 2, 3]);
        s.edge = None;
        let mut hole = circle(Point::new(50., 50.), 20., 200);
        hole.reverse();
        s.contours = Some(vec![square(0., 0., 100., 1.5), hole]);
        d.strokes.push(s);
        let id = d.strokes[0].id;
        assert!(filled(&d, Point::new(10., 10.)) && !filled(&d, Point::new(50., 50.)));
        make(&mut d, &[id]).unwrap();
        let c = d.strokes[0].compound.unwrap();
        assert_eq!(d.strokes.len(), 2);
        assert_eq!(d.strokes[0].id, id, "the first member keeps the id");
        assert!(d.strokes.iter().all(|s| s.contours.is_none() && s.compound == Some(c)));
        assert_eq!(d.strokes[0].nodes.len(), 4);
        assert!(d.strokes[1].nodes.len() <= 8);
        assert!(filled(&d, Point::new(10., 10.)) && !filled(&d, Point::new(50., 50.)));
    }

    #[test]
    fn old_files_without_compound_fields_still_load() {
        let d = doc_with(&[square(0., 0., 10., 5.)]);
        let mut v = serde_json::to_value(&d).unwrap();
        let s = &mut v["strokes"][0];
        assert!(s.get("compound").is_none() && s.get("fill_rule").is_none());
        s.as_object_mut().unwrap().remove("compound");
        let back: Document = serde_json::from_value(v).unwrap();
        assert_eq!(back.strokes[0].compound, None);
        assert_eq!(back.strokes[0].fill_rule, FillRule::NonZero);
    }
}
