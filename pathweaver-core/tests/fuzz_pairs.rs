// Every pair of visual node kinds, chained and fed into Booleans (all
// operations), then reconnected, must evaluate without panicking.
use pathweaver_core::appearance::{evaluate, Graph, Kind, Layer, Operation};
use kurbo::Point;

const KINDS: [Kind; 26] = [
    Kind::Rough, Kind::Zigzag, Kind::Transform, Kind::Fill, Kind::Stack, Kind::Reroute, Kind::Pathfinder,
    Kind::Boolean, Kind::Offset, Kind::Round, Kind::Band, Kind::Pucker, Kind::Simplify, Kind::Streak,
    Kind::Dissolve, Kind::Halftone, Kind::Mezzotint, Kind::Opacity, Kind::Shadow, Kind::Glow, Kind::Blur,
    Kind::Rasterize, Kind::RasterBlur, Kind::Threshold, Kind::Noise, Kind::Trace,
];

fn layers() -> Vec<Layer> {
    let ring = |x: f64, y: f64, w: f64, h: f64| vec![Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    vec![
        Layer {color: [20, 20, 20], opacity: 1., contours: vec![ring(0., 0., 300., 80.)], image: None},
        Layer {color: [200, 20, 20], opacity: 1., contours: vec![ring(40., 20., 120., 120.)], image: None},
    ]
}

fn run(graph: &Graph, label: &str, failures: &mut Vec<String>) {
    let original = layers();
    let g = graph.clone();
    let r = std::panic::catch_unwind(move || { let _ = evaluate(&g, &original); });
    if r.is_err() { failures.push(label.to_string()); }
}

#[test]
#[ignore = "slow: run with --ignored --release"]
fn no_node_combination_panics() {
    std::panic::set_hook(Box::new(|_| {}));
    let mut failures = vec![];
    for a in KINDS {
        for b in KINDS {
            for op in [Operation::Union, Operation::Difference, Operation::Intersect, Operation::Xor] {
                let mut g = Graph::default();
                let na = g.add(a, [100., 0.]).unwrap();
                let nb = g.add(b, [200., 0.]).unwrap();
                g.connect(1, na, 0).ok();
                if a.inputs() == 2 { g.connect(1, na, 1).ok(); }
                g.connect(na, nb, 0).ok();
                if b.inputs() == 2 { g.connect(1, nb, 1).ok(); }
                g.node_mut(na).unwrap().operation = op;
                g.node_mut(nb).unwrap().operation = op;
                g.connect(nb, 2, 0).ok();
                run(&g, &format!("{a:?} -> {b:?} ({op:?})"), &mut failures);
                g.connect(1, nb, 0).ok();
                if b.inputs() == 2 { g.connect(na, nb, 1).ok(); }
                run(&g, &format!("{a:?} -> {b:?} reconnected ({op:?})"), &mut failures);
            }
        }
    }
    let _ = std::panic::take_hook();
    assert!(failures.is_empty(), "{} panics, e.g. {:?}", failures.len(), &failures[..failures.len().min(12)]);
}
