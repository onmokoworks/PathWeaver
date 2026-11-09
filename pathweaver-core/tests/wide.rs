//! A curve made very wide (wider than its bends) folds its outline over
//! itself; that used to take seconds in the browser and froze the editor.
use pathweaver_core::model::Stroke;
use std::time::Instant;

const CURVE: &str = r#"{"id": 1, "name": "curve", "visible": true, "closed": false, "nodes": [{"id": 2, "p": {"x": 0, "y": 100}, "incoming": {"x": -100, "y": 100}, "outgoing": {"x": 100, "y": -100}, "smooth": true, "exact_outgoing": false, "exact_incoming": false}, {"id": 3, "p": {"x": 300, "y": 100}, "incoming": {"x": -100, "y": -100}, "outgoing": {"x": 100, "y": 100}, "smooth": true, "exact_outgoing": false, "exact_incoming": false}, {"id": 4, "p": {"x": 500, "y": 100}, "incoming": {"x": 0, "y": 0}, "outgoing": {"x": 0, "y": 0}, "smooth": false}], "widths": [], "start_width": {"left": 12, "right": 12, "linked": true, "angle": 0}, "end_width": {"left": 12, "right": 12, "linked": true, "angle": 0}, "start_angle": 90, "end_angle": 90, "fill": null, "edge": [32, 33, 36], "edge_width": 24, "frame_angle": 0, "corners": [], "join": "Miter", "miter_limit": 10, "contours": null, "appearance": null}"#;

#[test]
fn very_wide_curves_stay_fast() {
    for w in [300.0f64, 1000., 5000.] {
        let mut s: Stroke = serde_json::from_str(CURVE).unwrap();
        for k in [&mut s.start_width, &mut s.end_width] {
            k.left = w / 2.;
            k.right = w / 2.;
        }
        let t = Instant::now();
        let r = pathweaver_core::outline::contours_only(|| pathweaver_core::outline::paint_layers(&s));
        assert!(r.is_ok());
        // generous even for debug builds; it was ~0.8 s in release before
        assert!(t.elapsed().as_secs_f64() < 2.0, "w={w}: {:?}", t.elapsed());
    }
}

/// 角を丸く (in by r, out by r) must not leave dots: the inward offset left
/// zero-area slivers that the outward offset grew into small circles.
#[test]
fn round_leaves_no_dots() {
    use pathweaver_core::appearance_effects::{offset_contours, OffsetJoin};
    let s: Stroke = serde_json::from_str(CURVE).unwrap();
    let o = pathweaver_core::outline::build(&s).unwrap();
    let eroded = offset_contours(&o.contours, -3., OffsetJoin::Round).unwrap();
    assert_eq!(eroded.len(), 1, "slivers left: {:?}", eroded.iter().map(Vec::len).collect::<Vec<_>>());
}
