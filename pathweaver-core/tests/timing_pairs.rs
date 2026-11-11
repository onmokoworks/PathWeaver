use pathweaver_core::appearance::{evaluate, Graph, Kind, Layer, Operation};
use kurbo::Point;
#[test]
#[ignore]
fn timing_pairs() {
    std::panic::set_hook(Box::new(|i| eprintln!("PANIC {i}")));
    let ring = |x: f64, y: f64, w: f64, h: f64| vec![Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    let original = vec![
        Layer {color: [20, 20, 20], opacity: 1., contours: vec![ring(0., 0., 300., 80.)], image: None, gradient: None},
        Layer {color: [200, 20, 20], opacity: 1., contours: vec![ring(40., 20., 120., 120.)], image: None, gradient: None},
    ];
    let heavy: Vec<Kind> = match std::env::var("PW_HEAVY").as_deref() { Ok("mezzotint") => vec![Kind::Mezzotint], _ => vec![Kind::Mezzotint, Kind::Halftone, Kind::Dissolve, Kind::Glow, Kind::Blur, Kind::Rough] };
    let all = [Kind::Rough, Kind::Zigzag, Kind::Transform, Kind::Offset, Kind::Round, Kind::Band, Kind::Pucker, Kind::Simplify, Kind::Streak,
               Kind::Dissolve, Kind::Halftone, Kind::Mezzotint, Kind::Shadow, Kind::Glow, Kind::Blur, Kind::Boolean, Kind::Pathfinder, Kind::Stack];
    for a in heavy { for b in all { for op in [Operation::Union, Operation::Intersect] {
        for wiring in 0..3 {
            let mut g = Graph::default();
            let na = g.add(a, [0., 0.]).unwrap();
            let nb = g.add(b, [0., 0.]).unwrap();
            g.node_mut(nb).unwrap().operation = op;
            g.connect(1, na, 0).unwrap();
            match wiring {
                0 => { g.connect(na, nb, 0).ok(); if b.inputs() == 2 { g.connect(1, nb, 1).ok(); } }
                1 => { g.connect(1, nb, 0).ok(); if b.inputs() == 2 { g.connect(na, nb, 1).ok(); } else { continue } }
                _ => { g.connect(na, nb, 0).ok(); if b.inputs() == 2 { g.connect(na, nb, 1).ok(); } else { continue } }
            }
            g.connect(nb, 2, 0).ok();
            let t = std::time::Instant::now();
            let o = original.clone();
            let r = std::panic::catch_unwind(move || evaluate(&g, &o).map(|l| l.iter().flat_map(|l| l.contours.iter()).map(|c| c.len()).sum::<usize>()));
            let el = t.elapsed();
            if r.is_err() || el.as_millis() > std::env::var("PW_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(100) { println!("{a:?}->{b:?} {op:?} w{wiring}: {el:?} {:?}", r.map(|x| x.map_err(|e| e.chars().take(40).collect::<String>()))); }
        }
    }}}
}
