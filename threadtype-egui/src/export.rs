use crate::geometry::{flatten_segs, materialize_face, reverse_segs, signed_area, Peg};
use crate::model::{resolve_peg, Document, Face, PegRef};
use kurbo::Point;
use std::f64::consts::PI;

fn fmt(v: f64) -> String {
    format!("{:.2}", v)
}

/// 円弧を SVG の A コマンド列に変換。掃引が大きい場合は分割する
fn arc_to_svg(center: Point, radius: f64, start: f64, end: f64, d: &mut String) {
    let sweep = end - start;
    let steps = ((sweep.abs() / (PI * 0.999)).ceil() as usize).max(1);
    let flag = if sweep > 0.0 { 1 } else { 0 };
    for i in 1..=steps {
        let a = start + sweep * (i as f64 / steps as f64);
        let p = Point::new(center.x + radius * a.cos(), center.y + radius * a.sin());
        d.push_str(&format!(
            "A {} {} 0 0 {} {} {} ",
            fmt(radius),
            fmt(radius),
            flag,
            fmt(p.x),
            fmt(p.y)
        ));
    }
}

fn segs_to_path_data(segs: &[crate::geometry::Seg], d: &mut String) {
    let start = match segs.first() {
        Some(crate::geometry::Seg::Line { from, .. }) => *from,
        Some(crate::geometry::Seg::Arc { center, radius, start, .. }) => {
            Point::new(center.x + radius * start.cos(), center.y + radius * start.sin())
        }
        None => return,
    };
    d.push_str(&format!("M {} {} ", fmt(start.x), fmt(start.y)));
    for seg in segs {
        match *seg {
            crate::geometry::Seg::Line { to, .. } => {
                d.push_str(&format!("L {} {} ", fmt(to.x), fmt(to.y)))
            }
            crate::geometry::Seg::Arc { center, radius, start, end } => {
                arc_to_svg(center, radius, start, end, d)
            }
        }
    }
    d.push_str("Z ");
}

fn flatten_face(face: &Face, resolve: &impl Fn(&PegRef) -> Option<Peg>, pts: &mut Vec<Point>) {
    if let Some(segs) = materialize_face(face, resolve) {
        flatten_segs(&segs, pts);
    }
}

pub fn document_to_svg(doc: &Document, canvas_center: Point) -> String {
    // 全面の外接矩形から viewBox を決める
    let resolver = |r: &PegRef| resolve_peg(&doc.grid, &doc.custom_pegs, canvas_center, r);
    let mut pts: Vec<Point> = Vec::new();
    let mut path_data = String::new();

    for face in doc.faces.iter().filter(|face| face.visible) {
        let mut face_pts = Vec::new();
        flatten_face(face, &resolver, &mut face_pts);
        if face_pts.is_empty() {
            continue;
        }
        let mut d = String::new();
        if let Some(segs) = materialize_face(face, &resolver) {
            // NonZero 塗りで面の重なりが常に合体になるよう向きを正へ揃える
            let segs = if signed_area(&face_pts) < 0.0 {
                reverse_segs(&segs)
            } else {
                segs
            };
            segs_to_path_data(&segs, &mut d);
            path_data.push_str(&d);
        }
        pts.extend(face_pts);
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
                    exit_offset: 0.0, sweep_hint: std::f64::consts::PI,
                }],
                visible: true,
            name: String::new(),
            }],
            ..Document::default()
        };
        let svg = document_to_svg(&doc, Point::new(0.0, 0.0));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<path"));
        assert!(svg.contains("Z"));
    }
}
