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
    Intersection,
    Grid,
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
    // Explicit ruler guides take priority. Solve X and Y independently so
    // crossing guides snap to their intersection, not just whichever came first.
    if doc.snap_guides {
        let closest = |vertical: bool| {
            doc.guides
                .iter()
                .filter(|g| g.vertical == vertical)
                .filter_map(|g| {
                    let distance = (if vertical { point.x } else { point.y } - g.position).abs();
                    (distance < tolerance).then_some((distance, g.position))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|v| v.1)
        };
        let x = closest(true);
        let y = closest(false);
        if x.is_some() || y.is_some() {
            let grid = |v: f64| {
                if doc.snap_grid {
                    (v / doc.grid_spacing).round() * doc.grid_spacing
                } else {
                    v
                }
            };
            result = Point::new(
                x.unwrap_or_else(|| grid(point.x)),
                y.unwrap_or_else(|| grid(point.y)),
            );
            return (
                result,
                Some(Guide {
                    origin: result,
                    direction: if x.is_some() {
                        Vec2::new(0., 1.)
                    } else {
                        Vec2::new(1., 0.)
                    },
                    kind: if x.is_some() && y.is_some() {
                        Kind::Intersection
                    } else {
                        Kind::Guide
                    },
                }),
            );
        }
    }
    // Directional constraints win over the grid, so grid rounding never breaks tangency.
    if guide.is_none() && doc.snap_grid {
        let g = doc.grid_spacing;
        result = Point::new((point.x / g).round() * g, (point.y / g).round() * g);
        guide = Some(Guide {
            origin: result,
            direction: Vec2::ZERO,
            kind: Kind::Grid,
        });
    }
    (result, guide)
}

/// Snap a fixed drag reference while preserving the pointer's grab offset.
pub fn translation(
    doc: &Document,
    pointer: Point,
    start: Point,
    origin: Point,
    frames: &[Frame],
    zoom: f64,
    bypass: bool,
) -> (Point, Option<Guide>) {
    let desired = origin + (pointer - start);
    if bypass {
        (desired, None)
    } else {
        snap(doc, desired, frames, zoom)
    }
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
    #[test]
    fn guides_snap_both_axes_and_respect_zoom_and_toggle() {
        use crate::model::Guide as RulerGuide;
        let mut d = Document {
            guides: vec![
                RulerGuide {
                    vertical: true,
                    position: 42.,
                },
                RulerGuide {
                    vertical: false,
                    position: -18.,
                },
            ],
            ..Default::default()
        };
        let (p, g) = snap(&d, Point::new(44., -15.), &[], 1.);
        assert_eq!(p, Point::new(42., -18.));
        assert_eq!(g.unwrap().kind, Kind::Intersection);
        assert_eq!(
            snap(&d, Point::new(47., 100.), &[], 2.).0,
            Point::new(47., 100.)
        );
        d.snap_guides = false;
        assert_eq!(
            snap(&d, Point::new(44., -15.), &[], 1.).0,
            Point::new(44., -15.)
        );
    }
    #[test]
    fn grid_uses_spacing_negative_coordinates_and_free_guide_axis() {
        let mut d = Document {
            snap_grid: true,
            grid_spacing: 20.,
            ..Default::default()
        };
        let (p, g) = snap(&d, Point::new(29., -32.), &[], 0.5);
        assert_eq!(p, Point::new(20., -40.));
        assert_eq!(g.unwrap().kind, Kind::Grid);
        d.guides.push(crate::model::Guide {
            vertical: true,
            position: 27.,
        });
        assert_eq!(
            snap(&d, Point::new(29., -32.), &[], 1.).0,
            Point::new(27., -40.)
        );
    }
    #[test]
    fn translation_uses_total_drag_distance_and_keeps_grab_offset() {
        let d = Document {
            snap_grid: true,
            grid_spacing: 20.,
            ..Default::default()
        };
        let start = Point::new(100., 100.);
        let origin = Point::new(20., 40.);
        let mut last = origin;
        let mut total = Vec2::ZERO;
        for i in 1..=30 {
            let (p, _) = translation(
                &d,
                start + Vec2::new(i as f64, 0.),
                start,
                origin,
                &[],
                1.,
                false,
            );
            total += p - last;
            last = p;
        }
        assert_eq!(total, Vec2::new(40., 0.));
        let (p, g) = translation(&d, Point::new(113., 105.), start, origin, &[], 1., true);
        assert_eq!(p, Point::new(33., 45.));
        assert!(g.is_none());
    }
}
