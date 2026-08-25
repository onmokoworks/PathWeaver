use crate::geometry::{
    materialize_face, materialize_face_with_obstacles, stroke_visible_ring_sets,
    union_polygon_ring_sets, union_visible_ring_sets, Peg, Seg,
};
use crate::model::{resolve_peg, Document, FontRenderStyle, PegAppearance, PegRef, ScaleAnchor};
use kurbo::Point;

fn fmt(v: f64) -> String {
    format!("{:.2}", v)
}

/// 円弧を SVG の A コマンド列に変換。掃引が大きい場合は分割する
fn appearance_point(center: Point, radius: f64, angle: f64, appearance: PegAppearance) -> Point {
    crate::geometry::on_peg(
        &Peg {
            center,
            radius,
            shape: appearance.shape,
            rounding: appearance.rounding,
        },
        angle,
    )
}

fn ring_to_path_data(ring: &[Point], d: &mut String) {
    let Some(first) = ring.first() else { return };
    d.push_str(&format!("M {} {} ", fmt(first.x), fmt(first.y)));
    for point in &ring[1..] {
        d.push_str(&format!("L {} {} ", fmt(point.x), fmt(point.y)));
    }
    d.push_str("Z ");
}

fn flatten_segs(segs: &[Seg], appearance: PegAppearance, pts: &mut Vec<Point>) {
    for seg in segs {
        match seg {
            crate::geometry::Seg::Line { from, to } => {
                if pts.is_empty() {
                    pts.push(*from);
                }
                pts.push(*to);
            }
            crate::geometry::Seg::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let n = ((end - start).abs() * radius / 2.0).ceil() as usize;
                for i in 0..=n.max(4).min(160) {
                    pts.push(appearance_point(
                        *center,
                        *radius,
                        start + (end - start) * i as f64 / n.max(4).min(160) as f64,
                        appearance,
                    ));
                }
            }
        }
    }
}

fn export_pegs(doc: &Document, canvas_center: Point, segs: &[Seg]) -> Vec<(PegRef, Peg)> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for seg in segs {
        match *seg {
            Seg::Line { from, to } => {
                for p in [from, to] {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
            }
            Seg::Arc { center, radius, .. } => {
                min_x = min_x.min(center.x - radius);
                min_y = min_y.min(center.y - radius);
                max_x = max_x.max(center.x + radius);
                max_y = max_y.max(center.y + radius);
            }
        }
    }
    let origin = match doc.grid.scale_anchor {
        ScaleAnchor::TopLeft => Point::new(110.0, 110.0),
        ScaleAnchor::Center => Point::new(
            canvas_center.x - 3.0 * doc.grid.spacing,
            canvas_center.y - 2.0 * doc.grid.spacing,
        ),
    };
    let spacing = doc.grid.spacing.max(1.0);
    let col_min = ((min_x - origin.x) / spacing).floor() as i32 - 2;
    let col_max = ((max_x - origin.x) / spacing).ceil() as i32 + 2;
    let row_min = ((min_y - origin.y) / spacing).floor() as i32 - 2;
    let row_max = ((max_y - origin.y) / spacing).ceil() as i32 + 2;
    let mut pegs = Vec::new();
    for row in row_min..=row_max {
        for col in col_min..=col_max {
            let grid_ref = PegRef::Grid { row, col };
            if !doc.disabled_pegs.contains(&grid_ref) {
                if let Some(peg) = resolve_peg(
                    &doc.grid,
                    &doc.custom_pegs,
                    canvas_center,
                    doc.peg_appearance,
                    &grid_ref,
                ) {
                    pegs.push((grid_ref, peg));
                }
            }
            let sub_ref = PegRef::Sub { row, col };
            if doc.grid.subs_enabled && !doc.disabled_pegs.contains(&sub_ref) {
                if let Some(peg) = resolve_peg(
                    &doc.grid,
                    &doc.custom_pegs,
                    canvas_center,
                    doc.peg_appearance,
                    &sub_ref,
                ) {
                    pegs.push((sub_ref, peg));
                }
            }
        }
    }
    for custom in &doc.custom_pegs {
        let peg_ref = PegRef::Custom { id: custom.id };
        if let Some(peg) = resolve_peg(
            &doc.grid,
            &doc.custom_pegs,
            canvas_center,
            doc.peg_appearance,
            &peg_ref,
        ) {
            pegs.push((peg_ref, peg));
        }
    }
    pegs
}

pub fn document_to_svg(doc: &Document, canvas_center: Point) -> String {
    document_to_svg_styled(doc, canvas_center, FontRenderStyle::Fill, 3.0)
}

pub fn document_to_svg_styled(
    doc: &Document,
    canvas_center: Point,
    render_style: FontRenderStyle,
    thread_width: f64,
) -> String {
    // 全面の外接矩形から viewBox を決める
    let resolver = |r: &PegRef| {
        resolve_peg(
            &doc.grid,
            &doc.custom_pegs,
            canvas_center,
            doc.peg_appearance,
            r,
        )
    };
    let mut face_ring_sets: Vec<Vec<Vec<Point>>> = Vec::new();
    let path_appearance = PegAppearance {
        shape: doc.peg_appearance.shape,
        rounding: doc.peg_appearance.rounding + doc.grid.catch,
    };

    for face in doc.faces.iter().filter(|face| face.visible) {
        // マージ済みレイヤーは焼き込みリングへベベルを適用する。
        if let Some(baked) = &face.baked {
            let rings: Vec<Vec<Point>> = baked
                .rings
                .iter()
                .map(|ring| crate::geometry::bevel_ring(ring, baked.bevel))
                .filter(|ring| ring.len() >= 3)
                .collect();
            if !rings.is_empty() {
                face_ring_sets.push(rings);
            }
            continue;
        }
        if let Some(base_segs) = materialize_face(face, &resolver) {
            let obstacles = export_pegs(doc, canvas_center, &base_segs);
            let obstacles: Vec<_> = obstacles
                .into_iter()
                .filter(|(peg_ref, _)| !face.ignored_pegs.contains(peg_ref))
                .collect();
            let Some(segs) = materialize_face_with_obstacles(face, &resolver, &obstacles) else {
                continue;
            };
            let mut face_pts = Vec::new();
            flatten_segs(&segs, path_appearance, &mut face_pts);
            if face_pts.is_empty() {
                continue;
            }
            face_ring_sets.push(vec![face_pts]);
        }
    }

    // SVG/TTFの輪郭規則に任せず、キャンバスで見えている各面を一度
    // 三角形化してから加算的にunionする。自己交差や別面の重なりが
    // 逆向き輪郭として相殺されることを防ぐ。
    let rings = match render_style {
        FontRenderStyle::Thread => stroke_visible_ring_sets(&face_ring_sets, thread_width),
        FontRenderStyle::Fill => union_visible_ring_sets(&face_ring_sets),
        FontRenderStyle::FillAndThread => {
            let fill = union_visible_ring_sets(&face_ring_sets);
            let thread = stroke_visible_ring_sets(&face_ring_sets, thread_width);
            union_polygon_ring_sets(&[fill, thread])
        }
    };
    let mut pts: Vec<Point> = Vec::new();
    let mut path_data = String::new();
    for ring in &rings {
        ring_to_path_data(ring, &mut path_data);
        pts.extend(ring.iter().copied());
    }

    let (min_x, min_y, max_x, max_y) = pts.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(ax, ay, bx, by), p| (ax.min(p.x), ay.min(p.y), bx.max(p.x), by.max(p.y)),
    );
    let (min_x, min_y, max_x, max_y) = if pts.is_empty() {
        (0.0, 0.0, 100.0, 100.0)
    } else {
        (min_x - 20.0, min_y - 20.0, max_x + 20.0, max_y + 20.0)
    };

    format!(
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\">\n",
            "  <path d=\"{}\" fill=\"#000000\" fill-rule=\"nonzero\"/>\n",
            "</svg>\n"
        ),
        fmt(min_x),
        fmt(min_y),
        fmt(max_x - min_x),
        fmt(max_y - min_y),
        path_data.trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Face;

    #[test]
    fn svg_contains_face_path_when_materialized() {
        let doc = Document {
            custom_pegs: vec![crate::model::CustomPeg {
                id: 42,
                center: Point::new(100.0, 80.0),
                radius: 50.0,
                visible: true,
            }],
            faces: vec![Face {
                start: crate::model::StartAnchor {
                    peg: PegRef::Custom { id: 42 },
                    port: crate::model::Port::Right,
                },
                elements: vec![crate::model::Element::Wrap {
                    peg: PegRef::Custom { id: 42 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: std::f64::consts::PI,
                }],
                visible: true,
                name: String::new(),
                baked: None,
                ignored_pegs: Vec::new(),
            }],
            ..Document::default()
        };
        let svg = document_to_svg(&doc, Point::new(0.0, 0.0));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<path"));
        assert!(svg.contains("Z"));
    }

    #[test]
    fn svg_unions_overlapping_faces_instead_of_knocking_out_overlap() {
        let square = |x: f64| Face {
            start: crate::model::StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: crate::model::Port::Top,
            },
            elements: Vec::new(),
            visible: true,
            name: String::new(),
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(x, 0.0),
                    Point::new(x + 100.0, 0.0),
                    Point::new(x + 100.0, 100.0),
                    Point::new(x, 100.0),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        };
        let doc = Document {
            faces: vec![square(0.0), square(50.0)],
            ..Document::default()
        };
        let svg = document_to_svg(&doc, Point::new(0.0, 0.0));
        assert_eq!(
            svg.matches("M ").count(),
            1,
            "overlap should export as one unioned contour"
        );
    }

    #[test]
    fn thread_style_exports_a_real_outlined_stroke() {
        let face = Face {
            start: crate::model::StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: crate::model::Port::Top,
            },
            elements: Vec::new(),
            visible: true,
            name: String::new(),
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(0.0, 0.0),
                    Point::new(100.0, 100.0),
                    Point::new(0.0, 100.0),
                    Point::new(100.0, 0.0),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        };
        let doc = Document {
            faces: vec![face],
            ..Document::default()
        };
        let fill = document_to_svg(&doc, Point::new(0.0, 0.0));
        let thread =
            document_to_svg_styled(&doc, Point::new(0.0, 0.0), FontRenderStyle::Thread, 6.0);
        assert!(thread.contains("<path"));
        assert!(thread.contains("Z"));
        assert_ne!(thread, fill);
    }
}
