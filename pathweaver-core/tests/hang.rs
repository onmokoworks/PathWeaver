use pathweaver_core::appearance::{evaluate, Graph, Kind, Layer};
use kurbo::Point;
#[test]
#[ignore]
fn rough_band() {
    let ring = |x: f64, y: f64, w: f64, h: f64| vec![Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    let original = vec![
        Layer {color: [20, 20, 20], opacity: 1., contours: vec![ring(0., 0., 300., 80.)], image: None, gradient: None},
        Layer {color: [200, 20, 20], opacity: 1., contours: vec![ring(40., 20., 120., 120.)], image: None, gradient: None},
    ];
    for b in [Kind::Band, Kind::Pucker, Kind::Simplify, Kind::Streak, Kind::Dissolve, Kind::Halftone, Kind::Mezzotint, Kind::Glow, Kind::Blur] {
        let mut g = Graph::default();
        let na = g.add(Kind::Rough, [0., 0.]).unwrap();
        let nb = g.add(b, [0., 0.]).unwrap();
        g.connect(1, na, 0).unwrap(); g.connect(na, nb, 0).unwrap(); g.connect(nb, 2, 0).unwrap();
        let t = std::time::Instant::now();
        let r = evaluate(&g, &original);
        println!("Rough->{b:?}: {:?} {:?}", t.elapsed(), r.map(|l| l.iter().flat_map(|l| l.contours.iter()).map(|c| c.len()).sum::<usize>()));
    }
}
