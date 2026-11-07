//! Reversible corner parameters; generated Beziers are never written over anchors.
use super::model::{CornerKind, Node, Stroke};
use kurbo::{CubicBez, ParamCurve, Point, Vec2};

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn straight(s: &Stroke, i: usize) -> bool {
    let d = s.nodes[(i + 1) % s.nodes.len()].p - s.nodes[i].p;
    let l = d.hypot();
    if l < 1e-6 {
        return false;
    }
    [
        s.nodes[i].outgoing,
        -s.nodes[(i + 1) % s.nodes.len()].incoming,
    ]
    .iter()
    .all(|v| cross(*v, d).abs() < 1e-6 * l && v.dot(d) >= -1e-8 && v.dot(d) <= d.hypot2() + 1e-8)
}
fn direction(s: &Stroke, i: usize, t: f64) -> Vec2 {
    if straight(s, i) {
        (s.nodes[(i + 1) % s.nodes.len()].p - s.nodes[i].p).normalize()
    } else {
        s.tangent(i, t)
    }
}
pub fn eligible(s: &Stroke, i: usize) -> bool {
    s.nodes.len() > 2
        && (s.closed || (i > 0 && i + 1 < s.nodes.len()))
        && cross(
            direction(s, (i + s.nodes.len() - 1) % s.nodes.len(), 1.),
            direction(s, i, 0.),
        )
        .abs()
            > 1e-5
}
pub fn control(s: &Stroke, i: usize) -> Option<(Point, Vec2, f64)> {
    if !eligible(s, i) {
        return None;
    }
    let p = s.nodes[i].p;
    let a = -direction(s, (i + s.nodes.len() - 1) % s.nodes.len(), 1.);
    let b = direction(s, i, 0.);
    let bisector = a / a.hypot() + b / b.hypot();
    let dir = bisector / bisector.hypot();
    Some((p, dir, available(s, i)))
}
pub fn radius_limit(s: &Stroke, i: usize, kind: CornerKind) -> f64 {
    let Some((_, _, distance)) = control(s, i) else {
        return 0.;
    };
    let a = direction(s, (i + s.nodes.len() - 1) % s.nodes.len(), 1.);
    let b = direction(s, i, 0.);
    let turn = cross(a, b).atan2(a.dot(b)).abs();
    let factor = match kind {
        CornerKind::Clothoid => turn * integral(0., 1., turn).hypot() / (turn / 2.).cos(),
        _ => (turn / 2.).tan(),
    };
    (distance / factor.max(1e-9)).min(10000.)
}
/// Position and drag conversion share the same inner-angle geometry.
pub fn widget(s: &Stroke, i: usize, zoom: f64) -> Option<(Point, Vec2, f64)> {
    let (p, dir, _) = control(s, i)?;
    let a = -direction(s, (i + s.nodes.len() - 1) % s.nodes.len(), 1.);
    let b = direction(s, i, 0.);
    let sin_half = ((1. - a.dot(b)) * 0.5).sqrt();
    let factor = (1. / sin_half.max(1e-6) - 1.).max(0.05);
    let corner = s.corners.iter().find(|c| c.node == s.nodes[i].id);
    let kind = corner.map_or(CornerKind::Circular, |c| c.kind);
    let radius = corner
        .map_or(0., |c| c.radius)
        .min(radius_limit(s, i, kind));
    Some((p + dir * (18. / zoom + radius * factor), dir, factor))
}
fn heading(u: f64, angle: f64) -> f64 {
    angle
        * if u <= 0.5 {
            2. * u * u
        } else {
            1. - 2. * (1. - u).powi(2)
        }
}
fn integral(a: f64, b: f64, angle: f64) -> Vec2 {
    let count = 16;
    let h = (b - a) / count as f64;
    let mut result = Vec2::ZERO;
    for i in 0..=count {
        let t = heading(a + h * i as f64, angle);
        let w = if i == 0 || i == count {
            1.
        } else if i % 2 == 0 {
            2.
        } else {
            4.
        };
        result += Vec2::new(t.cos(), t.sin()) * w;
    }
    result * (h / 3.)
}
fn arc_nodes(
    p: Point,
    incoming: Vec2,
    outgoing: Vec2,
    radius: f64,
    maximum: f64,
    kind: CornerKind,
) -> Vec<Node> {
    let angle = cross(incoming, outgoing).atan2(incoming.dot(outgoing));
    let turn = angle.abs();
    let shape = integral(0., 1., angle);
    let desired = match kind {
        CornerKind::Clothoid => 2. * radius * turn * shape.hypot() / (2. * (turn / 2.).cos()),
        _ => radius * (turn / 2.).tan(),
    };
    let distance = desired.min(maximum);
    let start = p - incoming * distance;
    let end = p + outgoing * distance;
    let mut nodes = Vec::new();
    let normal = Vec2::new(-incoming.y, incoming.x);
    let point = |local: Vec2| start + incoming * local.x + normal * local.y;
    let make = |p| Node {
        exact_incoming: true,
        exact_outgoing: true,
        id: 0,
        p,
        incoming: Vec2::ZERO,
        outgoing: Vec2::ZERO,
        smooth: true,
    };
    match kind {
        CornerKind::Bevel => {
            nodes.push(make(start));
            nodes.push(make(end));
        }
        CornerKind::Circular => {
            let r = distance / (turn / 2.).tan();
            let center = start + normal * (r * angle.signum());
            let initial = start - center;
            let count = (turn / std::f64::consts::FRAC_PI_2).ceil().max(1.) as usize;
            for j in 0..=count {
                let a = angle * j as f64 / count as f64;
                let v = Vec2::new(
                    initial.x * a.cos() - initial.y * a.sin(),
                    initial.x * a.sin() + initial.y * a.cos(),
                );
                let mut n = make(center + v);
                let tangent = Vec2::new(-v.y, v.x) * angle.signum() / r;
                let h = 4. / 3. * (turn / count as f64 / 4.).tan() * r;
                if j > 0 {
                    n.incoming = -tangent * h;
                }
                if j < count {
                    n.outgoing = tangent * h;
                }
                nodes.push(n);
            }
        }
        CornerKind::Clothoid => {
            let length = (end - start).hypot() / shape.hypot();
            let count = 32;
            let mut pos = Vec2::ZERO;
            for j in 0..=count {
                let u = j as f64 / count as f64;
                if j > 0 {
                    pos += integral((j - 1) as f64 / count as f64, u, angle) * length;
                }
                let a = heading(u, angle);
                let tangent = incoming * a.cos() + normal * a.sin();
                let mut n = make(point(pos));
                if j > 0 {
                    n.incoming = -tangent * (length / count as f64 / 3.);
                }
                if j < count {
                    n.outgoing = tangent * (length / count as f64 / 3.);
                }
                nodes.push(n);
            }
        }
    }
    nodes.first_mut().unwrap().p = start;
    nodes.last_mut().unwrap().p = end;
    nodes
}
fn distance_table(s: &Stroke, i: usize) -> Vec<f64> {
    let mut d = vec![0.];
    let mut p = s.eval(i, 0.);
    for j in 1..=128 {
        let q = s.eval(i, j as f64 / 128.);
        d.push(d.last().unwrap() + p.distance(q));
        p = q;
    }
    d
}
fn available(s: &Stroke, i: usize) -> f64 {
    let reserved = |j: usize| {
        if s.corners
            .iter()
            .any(|c| c.node == s.nodes[j].id && c.radius > 0.)
        {
            0.499
        } else {
            0.98
        }
    };
    (distance_table(s, (i + s.nodes.len() - 1) % s.nodes.len())[128]
        * reserved((i + s.nodes.len() - 1) % s.nodes.len()))
    .min(distance_table(s, i)[128] * reserved((i + 1) % s.nodes.len()))
}
fn at_length(table: &[f64], distance: f64) -> f64 {
    let j = table.partition_point(|d| *d < distance).clamp(1, 128);
    ((j - 1) as f64 + (distance - table[j - 1]) / (table[j] - table[j - 1]).max(1e-12)) / 128.
}
fn parameter(s: &Stroke, i: usize, table: &[f64], distance: f64) -> f64 {
    if !straight(s, i) {
        return at_length(table, distance);
    }
    let a = s.nodes[i].p;
    let dir = (s.nodes[(i + 1) % s.nodes.len()].p - a).normalize();
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..48 {
        let t = (lo + hi) * 0.5;
        if (s.eval(i, t) - a).dot(dir) < distance {
            lo = t;
        } else {
            hi = t;
        }
    }
    (lo + hi) * 0.5
}
fn cubic(s: &Stroke, i: usize) -> CubicBez {
    s.cubic(i)
}
fn append(out: &mut Stroke, c: CubicBez, id: &mut u64) {
    if out.nodes.is_empty() {
        out.nodes.push(Node {
            exact_incoming: true,
            exact_outgoing: true,
            id: *id,
            p: c.p0,
            incoming: Vec2::ZERO,
            outgoing: Vec2::ZERO,
            smooth: true,
        });
        *id -= 1;
    }
    out.nodes.last_mut().unwrap().outgoing = c.p1 - c.p0;
    out.nodes.push(Node {
        exact_incoming: true,
        exact_outgoing: true,
        id: *id,
        p: c.p3,
        incoming: c.p2 - c.p3,
        outgoing: Vec2::ZERO,
        smooth: true,
    });
    *id -= 1;
}
pub fn rounded(s: &Stroke) -> Stroke {
    let mut out = s.clone();
    out.corners.clear();
    if !s.corners.iter().any(|c| {
        c.radius > 1e-6
            && s.nodes
                .iter()
                .position(|n| n.id == c.node)
                .is_some_and(|i| eligible(s, i))
    }) {
        return out;
    }
    let mut cuts = vec![None; s.nodes.len()];
    for (i, cut) in cuts.iter_mut().enumerate() {
        if let Some(c) = s
            .corners
            .iter()
            .find(|c| c.node == s.nodes[i].id && c.radius > 1e-6 && eligible(s, i))
        {
            let incoming = direction(s, (i + s.nodes.len() - 1) % s.nodes.len(), 1.);
            let outgoing = direction(s, i, 0.);
            let theta = cross(incoming, outgoing)
                .atan2(incoming.dot(outgoing))
                .abs();
            let factor = if c.kind == CornerKind::Clothoid {
                theta * integral(0., 1., theta).hypot() / (theta / 2.).cos()
            } else {
                (theta / 2.).tan()
            };
            let d = (c.radius * factor).min(available(s, i));
            let left = distance_table(s, (i + s.nodes.len() - 1) % s.nodes.len());
            let right = distance_table(s, i);
            *cut = Some((
                parameter(
                    s,
                    (i + s.nodes.len() - 1) % s.nodes.len(),
                    &left,
                    left[128] - d,
                ),
                parameter(s, i, &right, d),
                c.kind,
                c.radius,
                d,
            ));
        }
    }
    out.nodes.clear();
    let mut id = u64::MAX;
    for i in 0..s.segment_count() {
        let start = cuts[i].map_or(0., |c| c.1);
        let end = cuts[(i + 1) % s.nodes.len()].map_or(1., |c| c.0);
        append(&mut out, cubic(s, i).subsegment(start..end), &mut id);
        if let Some((ta, tb, kind, radius, d)) = cuts[(i + 1) % s.nodes.len()] {
            let a = s.eval(i, ta);
            let b = s.eval((i + 1) % s.segment_count(), tb);
            if straight(s, i) && straight(s, (i + 1) % s.segment_count()) {
                let mut nodes = arc_nodes(
                    s.nodes[(i + 1) % s.nodes.len()].p,
                    (s.nodes[(i + 1) % s.nodes.len()].p - s.nodes[i].p).normalize(),
                    (s.nodes[(i + 2) % s.nodes.len()].p - s.nodes[(i + 1) % s.nodes.len()].p)
                        .normalize(),
                    radius,
                    d,
                    kind,
                );
                nodes.first_mut().unwrap().p = a;
                nodes.last_mut().unwrap().p = b;
                for pair in nodes.windows(2) {
                    append(
                        &mut out,
                        CubicBez::new(
                            pair[0].p,
                            pair[0].p + pair[0].outgoing,
                            pair[1].p + pair[1].incoming,
                            pair[1].p,
                        ),
                        &mut id,
                    );
                }
            } else {
                // Curved neighbours retain their exact trimmed Beziers. Blend
                // their endpoint tangents; no silent loss of the corner setting.
                let u = direction(s, i, ta);
                let v = direction(s, (i + 1) % s.segment_count(), tb);
                let angle = cross(u, v).atan2(u.dot(v)).abs();
                let h = if kind == CornerKind::Bevel {
                    0.
                } else if angle < 1e-5 {
                    a.distance(b) / 3.
                } else {
                    a.distance(b) * 2. / 3. * (angle / 4.).tan() / (angle / 2.).sin().max(1e-6)
                };
                append(&mut out, CubicBez::new(a, a + u * h, b - v * h, b), &mut id);
            }
        }
    }
    if s.closed && out.nodes.len() > 1 {
        let last = out.nodes.pop().unwrap();
        out.nodes[0].incoming = last.incoming;
    }
    let mut center = out.clone();
    center.widths.clear();
    for key in &mut out.widths {
        if let Some(i) = s.segment_index(key.segment) {
            if let Some((j, t, _)) = super::outline::nearest(&center, s.eval(i, key.t)) {
                key.segment = center.nodes[j].id;
                key.t = t.clamp(0.001, 0.999);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Corner, Document, Width, WidthKey};
    fn fixture(kind: CornerKind) -> Stroke {
        let mut s = Stroke::new(1, "corners".into());
        for (id, p) in [
            (2, Point::new(40., 180.)),
            (3, Point::new(220., 180.)),
            (4, Point::new(220., 40.)),
        ] {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id,
                p,
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
            });
        }
        s.start_width.left = 8.;
        s.start_width.right = 8.;
        s.end_width = s.start_width;
        s.corners.push(Corner {
            node: 3,
            radius: 45.,
            kind,
        });
        s
    }
    #[test]
    fn live_corners_are_reversible_and_preserve_source_and_width_keys() {
        for kind in [
            CornerKind::Circular,
            CornerKind::Clothoid,
            CornerKind::Bevel,
        ] {
            let mut s = fixture(kind);
            s.widths.push(WidthKey {
                interpolated: false,
                id: 5,
                segment: 2,
                t: 0.8,
                width: Width::default(),
            });
            let source = s.nodes.clone();
            let generated = rounded(&s);
            assert_eq!(s.nodes, source);
            assert_eq!(generated.nodes.first().unwrap().p, source[0].p);
            assert_eq!(generated.nodes.last().unwrap().p, source[2].p);
            assert!(generated
                .segment_index(generated.widths[0].segment)
                .is_some());
            assert!(generated
                .nodes
                .iter()
                .all(|n| n.p.x.is_finite() && n.p.y.is_finite()));
            let rendered = crate::outline::build(&s).unwrap();
            assert!(!rendered.indices.is_empty());
            s.corners[0].radius = 0.;
            assert_eq!(rounded(&s).nodes, source);
            let d = Document {
                strokes: vec![s],
                next_id: 6,
                ..Document::default()
            };
            d.validate().unwrap();
            assert_eq!(
                serde_json::from_str::<Document>(&serde_json::to_string(&d).unwrap()).unwrap(),
                d
            );
        }
    }
    #[test]
    fn circle_and_clothoid_have_tangent_continuity_and_radius_limit() {
        for kind in [CornerKind::Circular, CornerKind::Clothoid] {
            let mut s = fixture(kind);
            let out = rounded(&s);
            assert!(
                out.nodes[1].outgoing.y.abs() < 1e-8,
                "{kind:?} {:?}",
                out.nodes[1]
            );
            assert!(out.nodes[out.nodes.len() - 2].incoming.x.abs() < 1e-8);
            if kind == CornerKind::Circular {
                assert!(out.nodes[1].p.distance(Point::new(175., 180.)) < 1e-8);
                assert!(out.nodes[2].p.distance(Point::new(220., 135.)) < 1e-8);
            }
            s.corners[0].radius = 10000.;
            let out = rounded(&s);
            assert!(out.nodes[1].p.x >= 220. - 140. * 0.98 - 1e-6);
        }
        // The clothoid begins with quadratic heading, unlike a circular arc.
        assert!((heading(0.01, 1.) - 0.0002).abs() < 1e-12);
        assert!((heading(0.99, 1.) - 0.9998).abs() < 1e-12);
    }
    #[test]
    fn corner_comparison_fixture() {
        if let Ok(path) = std::env::var("PATHWEAVER_CORNER_FIXTURE") {
            let mut fixtures = vec![];
            for kind in [
                CornerKind::Circular,
                CornerKind::Clothoid,
                CornerKind::Bevel,
            ] {
                let o = crate::outline::build(&fixture(kind)).unwrap();
                fixtures.push(serde_json::json!({"name":format!("{kind:?}"),"vertices":o.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":o.indices}));
            }
            for (label, radius, curve) in [
                ("Large radius", 120., false),
                ("Curved neighbour", 45., true),
            ] {
                let mut s = fixture(CornerKind::Circular);
                s.corners[0].radius = radius;
                if curve {
                    s.nodes[0].outgoing = Vec2::new(70., -60.);
                }
                let o = crate::outline::build(&s).unwrap();
                fixtures.push(serde_json::json!({"name":label,"vertices":o.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":o.indices}));
            }
            std::fs::write(path, serde_json::to_string(&fixtures).unwrap()).unwrap();
        }
    }
    #[test]
    fn neighbouring_curve_handle_does_not_remove_rounding() {
        let mut s = fixture(CornerKind::Circular);
        s.nodes[0].outgoing = Vec2::new(70., -60.);
        assert!(eligible(&s, 1));
        let source = s.nodes.clone();
        let out = rounded(&s);
        assert_eq!(s.nodes, source);
        assert!(out.nodes.len() > s.nodes.len());
        assert!(!out.nodes.iter().any(|n| n.p.distance(source[1].p) < 1e-6));
        for n in out.nodes.iter().skip(1).take(out.nodes.len() - 2) {
            assert!(cross(n.incoming, n.outgoing).abs() < 1e-4);
            assert!(n.incoming.dot(n.outgoing) <= 0.);
        }
        s.corners[0].radius = 0.;
        assert_eq!(rounded(&s).nodes, source);
    }
    #[test]
    fn isolated_corner_can_use_more_than_half_the_shorter_edge() {
        let s = fixture(CornerKind::Circular);
        assert!(radius_limit(&s, 1, CornerKind::Circular) > 130.);
    }
}
