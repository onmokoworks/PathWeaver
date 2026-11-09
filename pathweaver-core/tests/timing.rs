// Not an assertion test: prints per-kind evaluation time (cargo test -- --nocapture --ignored).
use pathweaver_core::appearance::{evaluate, Graph, Kind, Layer};
use kurbo::Point;
#[test]
#[ignore]
fn timing() {
    let ring = |x: f64, y: f64, w: f64, h: f64| vec![Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    let original = vec![
        Layer {color: [20, 20, 20], opacity: 1., contours: vec![ring(0., 0., 300., 80.)], image: None},
        Layer {color: [200, 20, 20], opacity: 1., contours: vec![ring(40., 20., 120., 120.)], image: None},
    ];
    for kind in [Kind::Rough, Kind::Zigzag, Kind::Offset, Kind::Round, Kind::Band, Kind::Pucker, Kind::Simplify, Kind::Streak,
                 Kind::Dissolve, Kind::Halftone, Kind::Mezzotint, Kind::Shadow, Kind::Glow, Kind::Blur, Kind::Boolean, Kind::Pathfinder] {
        let mut g = Graph::default();
        let n = g.add(kind, [100., 0.]).unwrap();
        g.connect(1, n, 0).unwrap();
        if kind.inputs() == 2 { g.connect(1, n, 1).unwrap(); }
        g.connect(n, 2, 0).unwrap();
        let t = std::time::Instant::now();
        let r = evaluate(&g, &original);
        let pts: usize = r.as_ref().map(|l| l.iter().flat_map(|l| l.contours.iter()).map(|c| c.len()).sum()).unwrap_or(0);
        println!("{kind:?}: {:?} ({} pts) {}", t.elapsed(), pts, r.err().unwrap_or_default());
    }
}
