//! Placed images and live text: placement follows transforms, export keeps
//! images (but never a tracing reference), imported rings honour fill rules.
use kurbo::{Affine, Point};
use pathweaver_core::model::{Document, Knockout, Placed, Stroke, Text};
use pathweaver_core::outline;

fn image_doc(reference: bool) -> Document {
    let mut d = Document::default();
    let mut s = Stroke::new(d.id(), "photo.png".into());
    s.edge = None;
    s.fill = None;
    let img = Placed { asset: "a1".into(), width: 40., height: 20., matrix: [1., 0., 0., 1., 10., 10.], opacity: 0.5, reference, knockout: None };
    s.contours = Some(vec![img.rect()]);
    s.image = Some(img);
    d.strokes.push(s);
    d.assets.insert("a1".into(), "data:image/png;base64,AAAA".into());
    d
}

#[test]
fn placement_follows_transforms() {
    let d = image_doc(false);
    let moved = d.strokes[0].transformed(Affine::translate((5., 0.)) * Affine::scale(2.), true);
    let img = moved.image.as_ref().unwrap();
    assert_eq!(img.matrix, [2., 0., 0., 2., 25., 20.]);
    // The pick rectangle moves with it.
    assert_eq!(moved.contours.as_ref().unwrap()[0], img.rect());
    let mut t = Stroke::new(1, "t".into());
    t.text = Some(Text { text: "A".into(), font: "Helvetica".into(), size: 12., line_height: 1.2, matrix: [1., 0., 0., 1., 0., 0.], ..Default::default() });
    let r = t.transformed(Affine::rotate(std::f64::consts::FRAC_PI_2), true);
    let m = r.text.unwrap().matrix;
    assert!((m[1] - 1.).abs() < 1e-9 && (m[2] + 1.).abs() < 1e-9);
}

#[test]
fn svg_keeps_images_but_not_references() {
    let d = image_doc(false);
    d.validate().unwrap();
    let svg = outline::svg(&d).unwrap();
    assert!(svg.contains("<image") && svg.contains("matrix(1.000000 0.000000 0.000000 1.000000 10.0000 10.0000)"));
    assert!(svg.contains("opacity=\"0.500000\""));
    let r = image_doc(true);
    assert!(!outline::svg(&r).unwrap().contains("<image"));
}

#[test]
fn svg_uses_the_keyed_copy_when_knocked_out() {
    let mut d = image_doc(false);
    d.strokes[0].image.as_mut().unwrap().knockout = Some(Knockout::Black);
    // without the keyed copy the original is exported
    assert!(outline::svg(&d).unwrap().contains("base64,AAAA"));
    d.assets.insert("a1#black".into(), "data:image/png;base64,KEYED".into());
    let svg = outline::svg(&d).unwrap();
    assert!(svg.contains("base64,KEYED") && !svg.contains("base64,AAAA"));
    // the field round-trips as a lowercase word and is left out when unset
    let json = serde_json::to_string(&d.strokes[0]).unwrap();
    assert!(json.contains("\"knockout\":\"black\""));
    let back: Stroke = serde_json::from_str(&json).unwrap();
    assert_eq!(back.image.unwrap().knockout, Some(Knockout::Black));
    assert!(!serde_json::to_string(&image_doc(false).strokes[0]).unwrap().contains("knockout"));
}

#[test]
fn missing_asset_is_rejected_only_with_assets_present() {
    let mut d = image_doc(false);
    d.strokes[0].image.as_mut().unwrap().asset = "zz".into();
    assert!(d.validate().is_err());
    d.assets.clear(); // the web side strips assets from edit calls
    d.validate().unwrap();
}

#[test]
fn even_odd_rings_keep_their_hole() {
    let sq = |a: f64, b: f64| vec![Point::new(a, a), Point::new(b, a), Point::new(b, b), Point::new(a, b)];
    // Both rings wound the same way: nonzero fills the middle, evenodd cuts it.
    let rings = vec![sq(0., 100.), sq(25., 75.)];
    let area = |cs: &[Vec<Point>]| cs.iter().map(|c| {
        let mut a = 0.;
        for i in 0..c.len() { let (p, q) = (c[i], c[(i + 1) % c.len()]); a += p.x * q.y - q.x * p.y; }
        a / 2.
    }).sum::<f64>().abs();
    assert!((area(&outline::fill_rule_contours(&rings, false)) - 10000.).abs() < 1.);
    assert!((area(&outline::fill_rule_contours(&rings, true)) - 7500.).abs() < 1.);
}

#[test]
fn svg_frames_an_artboard_at_its_size() {
    use pathweaver_core::model::Artboard;
    let mut d = image_doc(false);
    let svg = outline::svg_frame(&d, Some([0., 0., 200., 100.]), Some([255, 255, 255]), 0.).unwrap();
    assert!(svg.contains("width=\"200.0000\" height=\"100.0000\" viewBox=\"0.0000 0.0000 200.0000 100.0000\""));
    assert!(svg.contains("fill=\"#ffffff\""));
    assert!(outline::svg_frame(&d, Some([0., 0., 0., 10.]), None, 0.).is_err());
    // artboards round-trip and are validated
    let id = d.id();
    d.artboards.push(Artboard { id, name: "A4".into(), x: 0., y: 0., w: 595., h: 842. });
    d.validate().unwrap();
    let back: pathweaver_core::model::Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(back.artboards, d.artboards);
    d.artboards[0].w = -1.;
    assert!(d.validate().is_err());
}
