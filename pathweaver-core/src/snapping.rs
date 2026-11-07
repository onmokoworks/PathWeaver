//! Stable tangent/normal constraints. Frames are captured at gesture start.
use crate::model::{Document, Stroke};
use kurbo::{Point, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub origin: Point,
    pub tangent: Vec2,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Tangent,
    Normal,
    Guide,
}
#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub origin: Point,
    pub direction: Vec2,
    pub kind: Kind,
}

pub fn frame(stroke: &Stroke, node: u64) -> Option<Frame> {
    let i = stroke.nodes.iter().position(|n| n.id == node)?;
    let n = &stroke.nodes[i];
    let tangent = if n.outgoing.hypot() > 1e-8 {
        n.outgoing / n.outgoing.hypot()
    } else if n.incoming.hypot() > 1e-8 {
        -n.incoming / n.incoming.hypot()
    } else if i < stroke.segment_count() {
        stroke.tangent(i, 0.)
    } else if i > 0 {
        stroke.tangent(i - 1, 1.)
    } else {
        Vec2::new(1., 0.)
    };
    Some(Frame {
        origin: n.p,
        tangent,
    })
}

pub fn snap(doc: &Document, point: Point, frames: &[Frame], zoom: f64) -> (Point, Option<Guide>) {
    let tolerance = 8. / zoom.max(0.01);
    let mut best = tolerance;
    let mut result = point;
    let mut guide = None;
    let mut consider = |origin: Point, direction: Vec2, kind| {
        let projected = origin + direction * (point - origin).dot(direction);
        let distance = projected.distance(point);
        if distance < best {
            best = distance;
            result = projected;
            guide = Some(Guide {
                origin,
                direction,
                kind,
            });
        }
    };
    for f in frames {
        if point.distance(f.origin) < tolerance {
            continue;
        }
        if doc.snap_tangent {
            consider(f.origin, f.tangent, Kind::Tangent);
        }
        if doc.snap_normal {
            consider(f.origin, Vec2::new(-f.tangent.y, f.tangent.x), Kind::Normal);
        }
    }
    if doc.snap_guides {
        for g in &doc.guides {
            consider(
                if g.vertical {
                    Point::new(g.position, 0.)
                } else {
                    Point::new(0., g.position)
                },
                if g.vertical {
                    Vec2::new(0., 1.)
                } else {
                    Vec2::new(1., 0.)
                },
                Kind::Guide,
            );
        }
    }
    // Directional constraints win over the grid, so grid rounding never breaks tangency.
    if guide.is_none() && doc.snap_grid {
        let g = doc.grid_spacing;
        result = Point::new((point.x / g).round() * g, (point.y / g).round() * g);
    }
    (result, guide)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projects_to_tangent_and_normal_in_screen_tolerance() {
        let mut d = Document {
            snap_tangent: true,
            snap_normal: true,
            ..Default::default()
        };
        let frames = [Frame {
            origin: Point::ZERO,
            tangent: Vec2::new(0.6, 0.8),
        }];
        let (p, g) = snap(&d, Point::new(61., 80.), &frames, 1.);
        assert!(p.to_vec2().cross(frames[0].tangent).abs() < 1e-8);
        assert_eq!(g.unwrap().kind, Kind::Tangent);
        assert_eq!(
            snap(&d, Point::new(-80., 61.), &frames, 1.).1.unwrap().kind,
            Kind::Normal
        );
        assert!(snap(&d, Point::new(68., 80.), &frames, 2.).1.is_none());
        d.snap_grid = true;
        assert_eq!(snap(&d, Point::new(61., 80.), &frames, 1.).0, p);
    }
    #[test]
    fn origin_and_disabled_constraints_do_not_jump() {
        let d = Document::default();
        let f = [Frame {
            origin: Point::ZERO,
            tangent: Vec2::new(1., 0.),
        }];
        let p = Point::new(100., 3.);
        assert_eq!(snap(&d, p, &f, 1.).0, p);
        let d = Document {
            snap_tangent: true,
            ..d
        };
        assert_eq!(snap(&d, Point::new(2., 3.), &f, 1.).0, Point::new(2., 3.));
    }
}
