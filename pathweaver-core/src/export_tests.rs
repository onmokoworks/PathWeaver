use super::*;
use crate::model::{Corner, CornerKind, Folder, Node, Placed, Text, WidthKey};
use kurbo::{ParamCurve, PathSeg, Vec2};

fn node(id: u64, x: f64, y: f64, out: Vec2, inc: Vec2) -> Node {
    Node { exact_incoming: false, exact_outgoing: false, id, p: Point::new(x, y), incoming: inc, outgoing: out, smooth: false }
}
/// An S curve from (0,0) to (200,0) with a uniform 10-wide line.
pub(crate) fn curve(d: &mut Document) -> Stroke {
    let mut s = Stroke::new(d.id(), "曲線".into());
    let (a, b) = (d.id(), d.id());
    s.nodes = vec![node(a, 0., 0., Vec2::new(60., -80.), Vec2::ZERO), node(b, 200., 0., Vec2::ZERO, Vec2::new(-60., 80.))];
    s.edge = Some([10, 20, 30]);
    s.set_line_width(10.);
    s
}
pub(crate) fn square(d: &mut Document, x: f64, y: f64, size: f64) -> Stroke {
    let mut s = Stroke::new(d.id(), "四角".into());
    s.nodes = [(x, y), (x + size, y), (x + size, y + size), (x, y + size)]
        .iter()
        .map(|&(x, y)| node(d.id(), x, y, Vec2::ZERO, Vec2::ZERO))
        .collect();
    s.closed = true;
    s.fill = Some([200, 30, 30]);
    s.edge = None;
    s
}
fn tags(svg: &str, tag: &str) -> Vec<String> {
    svg.match_indices(&format!("<{tag} "))
        .map(|(i, _)| svg[i..i + svg[i..].find('>').unwrap() + 1].to_string())
        .collect()
}
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}
/// Distance from `p` to the nearest edge of the rings.
fn distance(rings: &[Vec<Point>], p: Point) -> f64 {
    rings
        .iter()
        .flat_map(|r| r.iter().zip(r.iter().cycle().skip(1)).take(r.len()))
        .map(|(a, b)| {
            let v = *b - *a;
            let t = ((p - *a).dot(v) / v.hypot2().max(1e-12)).clamp(0., 1.);
            p.distance(a.lerp(*b, t))
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn uniform_line_keeps_its_bezier_and_settings() {
    let mut d = Document::default();
    let mut s = curve(&mut d);
    s.start_cap = Cap::Round;
    s.end_cap = Cap::Round;
    s.join = Join::Bevel;
    d.strokes.push(s);
    let out = svg(&d, Some([-20., -100., 240., 200.]), None, 0., Options::default()).unwrap();
    let paths = tags(&out, "path");
    assert_eq!(paths.len(), 1, "{out}");
    let p = &paths[0];
    assert_eq!(attr(p, "d"), Some("M0 0 C60 -80 140 80 200 0"));
    assert_eq!(attr(p, "fill"), Some("none"));
    assert_eq!(attr(p, "stroke"), Some("#0a141e"));
    assert_eq!(attr(p, "stroke-width"), Some("10"));
    assert_eq!(attr(p, "stroke-linecap"), Some("round"));
    assert_eq!(attr(p, "stroke-linejoin"), Some("bevel"));
    assert_eq!(attr(p, "data-name"), Some("曲線"));
    assert!(out.contains("viewBox=\"-20.0000 -100.0000 240.0000 200.0000\""));
    // 輪郭にする: the same line as a filled outline
    let out = svg(&d, None, None, 0., Options { outline: true }).unwrap();
    let p = &tags(&out, "path")[0];
    assert_eq!(attr(p, "stroke"), None);
    assert_eq!(attr(p, "fill"), Some("#0a141e"));
    assert!(attr(p, "d").unwrap().contains('C'));
}

#[test]
fn closed_fill_and_line_is_one_path_and_rounded_corners_are_curves() {
    let mut d = Document::default();
    let mut s = square(&mut d, 0., 0., 100.);
    s.edge = Some([0, 0, 0]);
    s.set_line_width(4.);
    s.miter_limit = 4.;
    let corner = s.nodes[1].id;
    d.strokes.push(s.clone());
    let out = svg(&d, None, Some([255, 255, 255]), 16., Options::default()).unwrap();
    let p = &tags(&out, "path")[0];
    assert_eq!(attr(p, "d"), Some("M0 0 L100 0 L100 100 L0 100 L0 0 Z"));
    assert_eq!(attr(p, "fill"), Some("#c81e1e"));
    assert_eq!(attr(p, "stroke-miterlimit"), Some("4"));
    assert!(out.contains("<rect"), "background");
    // the painted frame includes the line's half width and the margin
    assert!(out.contains("viewBox=\"-18.0000 -18.0000 136.0000 136.0000\""), "{out}");
    d.strokes[0].corners.push(Corner { node: corner, radius: 20., kind: CornerKind::Circular });
    let out = svg(&d, None, None, 16., Options::default()).unwrap();
    let p = attr(&tags(&out, "path")[0], "d").unwrap().to_string();
    assert_eq!(p.matches('C').count(), 1, "{p}");
    assert!(p.contains("L80 0"), "{p}");
}

#[test]
fn variable_width_line_is_its_outline_fitted_with_few_curves() {
    let mut d = Document::default();
    let mut s = curve(&mut d);
    s.end_width.left = 20.;
    s.end_width.right = 20.;
    let segment = s.nodes[0].id;
    s.widths.push(WidthKey { interpolated: false, id: d.id(), segment, t: 0.5, width: crate::model::Width { left: 2., right: 2., linked: true, angle: 0. } });
    s.fill = Some([1, 2, 3]);
    assert!(uniform_width(&s).is_none());
    d.strokes.push(s.clone());
    let item = stroke_items(&d, &s, Options::default()).unwrap().unwrap();
    let Item::Group(meta, parts) = &item else { panic!("fill and outline: a group") };
    assert_eq!(meta.name, "曲線");
    assert_eq!(parts.len(), 2);
    let Item::Shape(_, outline) = &parts[1] else { panic!() };
    assert!(outline.line.is_none());
    let rings = edge_outline(&s).unwrap();
    let polyline: usize = rings.iter().map(|r| r.len()).sum();
    let segments = outline.path.segments().count();
    assert!(segments * 5 < polyline, "{segments} curves for {polyline} points");
    // the curves stay on the outline
    for seg in outline.path.segments() {
        for i in 0..=8 {
            let p = seg.eval(i as f64 / 8.);
            assert!(distance(&rings, p) < 0.3, "{p:?} is {} off", distance(&rings, p));
        }
        assert!(matches!(seg, PathSeg::Cubic(_) | PathSeg::Line(_)));
    }
    let out = svg(&d, None, None, 0., Options::default()).unwrap();
    assert!(out.contains(&format!("<g id=\"pw-{}\" data-name=\"曲線\">", s.id)), "{out}");
}

#[test]
fn locked_cap_angle_and_mixed_caps_fall_back_to_outline() {
    let mut d = Document::default();
    let mut s = curve(&mut d);
    assert!(line_style(&s).is_some());
    s.start_cap_locked = true;
    s.start_angle = 90.;
    assert!(line_style(&s).is_none(), "vertical cut on a slanted start");
    s.start_cap_locked = false;
    s.end_cap = Cap::Square;
    assert!(line_style(&s).is_none());
    s.start_cap = Cap::Square;
    assert_eq!(line_style(&s).unwrap().cap, Cap::Square);
}

#[test]
fn folders_are_nested_groups_in_stacking_order_without_hidden_things() {
    let mut d = Document::default();
    let (outer, inner, hidden) = (d.id(), d.id(), d.id());
    d.folders = vec![
        Folder { id: outer, name: "外 & <中>".into(), visible: true, open: true, parent: None, locked: false },
        Folder { id: inner, name: "中".into(), visible: true, open: true, parent: Some(outer), locked: false },
        Folder { id: hidden, name: "隠し".into(), visible: false, open: true, parent: None, locked: false },
    ];
    let mut a = square(&mut d, 0., 0., 10.);
    a.folder = Some(inner);
    let mut b = square(&mut d, 20., 0., 10.);
    b.folder = Some(outer);
    let c = square(&mut d, 40., 0., 10.);
    let mut e = square(&mut d, 60., 0., 10.);
    e.folder = Some(inner);
    let mut f = square(&mut d, 80., 0., 10.);
    f.folder = Some(hidden);
    let mut g = square(&mut d, 100., 0., 10.);
    g.visible = false;
    let mut guide = square(&mut d, 120., 0., 10.);
    guide.guide = true;
    let ids = [a.id, b.id, c.id, e.id];
    d.strokes = vec![a, b, c, e, f, g, guide];
    let out = svg(&d, None, None, 0., Options::default()).unwrap();
    let order: Vec<_> = out
        .match_indices(" id=\"")
        .map(|(i, _)| out[i + 5..i + 5 + out[i + 5..].find('"').unwrap()].to_string())
        .collect();
    assert_eq!(
        order,
        vec![
            format!("pw-folder-{outer}"),
            format!("pw-folder-{inner}"),
            format!("pw-{}", ids[0]),
            format!("pw-{}", ids[1]),
            format!("pw-{}", ids[2]),
            format!("pw-folder-{outer}-2"),
            format!("pw-folder-{inner}-2"),
            format!("pw-{}", ids[3]),
        ]
    );
    assert!(out.contains("inkscape:label=\"外 &amp; &lt;中&gt;\""));
    assert_eq!(out.matches("<g ").count(), out.matches("</g>").count());
    assert_eq!(tags(&out, "path").len(), 4);
}

#[test]
fn text_images_and_appearance() {
    let mut d = Document::default();
    let mut t = square(&mut d, 0., 0., 10.);
    t.contours = Some(vec![(0..64)
        .map(|i| {
            let a = i as f64 / 64. * std::f64::consts::TAU;
            Point::new(50. + 30. * a.cos(), 50. + 30. * a.sin())
        })
        .collect()]);
    t.nodes.clear();
    t.text = Some(Text { text: "あ\"<".into(), font: "Hiragino Sans".into(), size: 20., line_height: 1.2, matrix: [1., 0., 0., 1., 0., 0.], ..Default::default() });
    let mut img = square(&mut d, 0., 0., 10.);
    img.image = Some(Placed { asset: "a".into(), width: 4., height: 2., matrix: [2., 0., 0., 2., 5., 6.], opacity: 0.5, reference: false, knockout: Some(crate::model::Knockout::Black) });
    let mut reference = img.clone();
    reference.id = d.id();
    reference.image.as_mut().unwrap().reference = true;
    let mut rough = square(&mut d, 0., 0., 50.);
    rough.appearance = Some(crate::appearance::Graph::rough());
    d.assets.insert("a".into(), "data:image/png;base64,AAAA".into());
    d.assets.insert("a#black".into(), "data:image/png;base64,KEYED".into());
    d.strokes = vec![t, img, reference, rough];
    let out = svg(&d, Some([0., 0., 100., 100.]), None, 0., Options::default()).unwrap();
    let text = &tags(&out, "path")[0];
    assert_eq!(attr(text, "data-text"), Some("あ&quot;&lt;"));
    assert!(out.contains("<title>あ&quot;&lt;</title></path>"));
    // a circle of 64 points: a handful of curves
    assert!(attr(text, "d").unwrap().matches('C').count() <= 12, "{text}");
    let images = tags(&out, "image");
    assert_eq!(images.len(), 1, "the 下絵 is left out");
    assert_eq!(attr(&images[0], "href"), Some("data:image/png;base64,KEYED"));
    assert_eq!(attr(&images[0], "opacity"), Some("0.5"));
    assert!(attr(&images[0], "transform").unwrap().ends_with(" 5 6)"));
    // the appearance result is grouped under the path's name
    assert!(out.contains(&format!("<g id=\"pw-{}\" data-name=\"四角\">", d.strokes[3].id)), "{out}");
}

#[test]
fn num_is_short() {
    assert_eq!(num(1.5), "1.5");
    assert_eq!(num(2.0), "2");
    assert_eq!(num(-0.0001), "0");
    assert_eq!(num(f64::NAN), "0");
    assert_eq!(num(12.3456), "12.346");
}

#[test]
fn gradients_and_opacity_are_written() {
    use crate::paint::{Gradient, GradientStop};
    let mut d = Document::default();
    let mut s = square(&mut d, 0., 0., 100.);
    let stops = vec![GradientStop { offset: 0., color: [255, 0, 0], opacity: 1. }, GradientStop { offset: 1., color: [0, 0, 255], opacity: 1. }];
    s.fill_gradient = Some(Gradient::linear(Point::new(0., 0.), Point::new(100., 0.), stops));
    let mut c = curve(&mut d);
    c.edge_opacity = 0.5;
    d.strokes = vec![s, c];
    let svg = svg(&d, None, None, 0., Options::default()).unwrap();
    assert!(svg.contains("<linearGradient id=\"pw-gradient-1\""), "{svg}");
    assert!(svg.contains("fill=\"url(#pw-gradient-1)\""));
    assert!(svg.contains("stroke-opacity=\"0.5\""));
}

#[test]
fn compound_paths_keep_curves_and_holes() {
    let mut d = Document::default();
    let mut outer = square(&mut d, 0., 0., 100.);
    let mut inner = square(&mut d, 25., 25., 50.);
    let c = d.id();
    outer.compound = Some(c);
    inner.compound = Some(c);
    outer.fill_rule = crate::model::FillRule::EvenOdd;
    inner.fill_rule = crate::model::FillRule::EvenOdd;
    d.strokes = vec![outer, inner];
    let svg = svg(&d, None, None, 0., Options::default()).unwrap();
    let paths = tags(&svg, "path");
    assert_eq!(paths.len(), 1, "{svg}");
    assert_eq!(attr(&paths[0], "fill-rule"), Some("evenodd"));
    assert_eq!(attr(&paths[0], "d").unwrap().matches('M').count(), 2);
    let pdf = crate::pdf::pdf(&d, &[[0., 0., 100., 100.]], None, Options::default()).unwrap();
    assert!(!pdf.is_empty());
}
