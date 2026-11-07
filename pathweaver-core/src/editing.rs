//! Anchor and topology edits for lettering. Operations validate a copy before committing.
use crate::{
    model::{Document, Stroke, Width, WidthKey},
    outline,
};
use kurbo::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub stroke: u64,
    pub node: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    Corner,
    Smooth,
    Independent,
}

fn transaction<T>(
    doc: &mut Document,
    f: impl FnOnce(&mut Document) -> Result<T, String>,
) -> Result<T, String> {
    let mut next = doc.clone();
    let result = f(&mut next)?;
    next.validate()?;
    *doc = next;
    Ok(result)
}
fn index(doc: &Document, at: Anchor) -> Result<(usize, usize), String> {
    let si = doc
        .strokes
        .iter()
        .position(|s| s.id == at.stroke)
        .ok_or("パスが見つかりません")?;
    let s = &doc.strokes[si];
    if s.contours.is_some() {
        return Err("輪郭化されたパスには芯の編集を適用できません".into());
    }
    let ni = s
        .nodes
        .iter()
        .position(|n| n.id == at.node)
        .ok_or("点が見つかりません")?;
    Ok((si, ni))
}
pub fn set_anchor_kind(
    doc: &mut Document,
    anchors: &[Anchor],
    kind: AnchorKind,
) -> Result<(), String> {
    transaction(doc, |d| {
        for at in anchors {
            let (si, i) = index(d, *at)?;
            let s = &mut d.strokes[si];
            let len = s.nodes.len();
            let p = s.nodes[i].p;
            let prev = if i > 0 {
                Some(s.nodes[i - 1].p)
            } else if s.closed && len > 1 {
                Some(s.nodes[len - 1].p)
            } else {
                None
            };
            let next = if i + 1 < len {
                Some(s.nodes[i + 1].p)
            } else if s.closed && len > 1 {
                Some(s.nodes[0].p)
            } else {
                None
            };
            let direction = match (prev, next) {
                (Some(a), Some(b)) => b - a,
                (Some(a), None) => p - a,
                (None, Some(b)) => b - p,
                _ => Vec2::new(1., 0.),
            };
            let direction = if direction.hypot() > 1e-9 {
                direction / direction.hypot()
            } else {
                Vec2::new(1., 0.)
            };
            let n = &mut s.nodes[i];
            match kind {
                AnchorKind::Corner => {
                    n.incoming = Vec2::ZERO;
                    n.outgoing = Vec2::ZERO;
                    n.smooth = false;
                    n.exact_incoming = false;
                    n.exact_outgoing = false;
                }
                AnchorKind::Independent => n.smooth = false,
                AnchorKind::Smooth => {
                    let a = if n.incoming.hypot() > 1e-8 {
                        n.incoming.hypot()
                    } else {
                        prev.map_or(0., |q| q.distance(p) / 3.)
                    };
                    let b = if n.outgoing.hypot() > 1e-8 {
                        n.outgoing.hypot()
                    } else {
                        next.map_or(0., |q| q.distance(p) / 3.)
                    };
                    n.incoming = -direction * a;
                    n.outgoing = direction * b;
                    n.smooth = true;
                    n.exact_incoming = false;
                    n.exact_outgoing = false;
                }
            }
        }
        Ok(())
    })
}
pub fn straighten(doc: &mut Document, stroke: u64, segment: u64) -> Result<(), String> {
    transaction(doc, |d| {
        let (si, i) = index(
            d,
            Anchor {
                stroke,
                node: segment,
            },
        )?;
        let s = &mut d.strokes[si];
        if i >= s.segment_count() {
            return Err("次の線分がありません".into());
        }
        let j = (i + 1) % s.nodes.len();
        s.nodes[i].outgoing = Vec2::ZERO;
        s.nodes[i].exact_outgoing = false;
        s.nodes[i].smooth = false;
        s.nodes[j].incoming = Vec2::ZERO;
        s.nodes[j].exact_incoming = false;
        s.nodes[j].smooth = false;
        Ok(())
    })
}
pub fn insert_anchor(
    doc: &mut Document,
    stroke: u64,
    segment: usize,
    t: f64,
) -> Result<Anchor, String> {
    transaction(doc, |d| {
        let (si, _) = index(
            d,
            Anchor {
                stroke,
                node: d
                    .strokes
                    .iter()
                    .find(|s| s.id == stroke)
                    .and_then(|s| s.nodes.get(segment))
                    .ok_or("線分がありません")?
                    .id,
            },
        )?;
        if segment >= d.strokes[si].segment_count()
            || !t.is_finite()
            || !(1e-5..=1. - 1e-5).contains(&t)
        {
            return Err("線分の途中を選択してください".into());
        }
        let node = d.id();
        d.strokes[si].split(segment, t, node);
        Ok(Anchor { stroke, node })
    })
}
fn width_at(s: &Stroke, i: usize, t: f64) -> Width {
    let samples = outline::samples(s);
    let mut w = outline::Profile::new(s, &samples).width(outline::at_distance(&samples, i, t));
    w.linked = (w.left - w.right).abs() < 1e-8;
    w
}
fn keep_keys(s: &mut Stroke) {
    let ids: Vec<_> = s
        .nodes
        .iter()
        .take(s.segment_count())
        .map(|n| n.id)
        .collect();
    s.widths.retain(|k| ids.contains(&k.segment));
    s.corners.retain(|c| s.nodes.iter().any(|n| n.id == c.node));
}
fn width_key(d: &mut Document, segment: u64, t: f64, width: Width) -> WidthKey {
    WidthKey {
        id: d.id(),
        segment,
        t,
        width,
    }
}
/// Split at an anchor; a closed path is opened at that anchor.
pub fn separate(doc: &mut Document, at: Anchor) -> Result<Vec<Anchor>, String> {
    transaction(doc, |d| {
        let (si, i) = index(d, at)?;
        let original = d.strokes[si].clone();
        let len = original.nodes.len();
        if len < 2 || (!original.closed && (i == 0 || i + 1 == len)) {
            return Err("開いたパスの途中の点、または閉じたパスの点を選択してください".into());
        }
        let width = width_at(&original, i.min(original.segment_count() - 1), 0.);
        if original.closed {
            let mut open = original.clone();
            open.closed = false;
            open.nodes.rotate_left(i);
            let mut end = open.nodes[0].clone();
            end.id = d.id();
            open.nodes.push(end);
            open.start_width = width;
            open.end_width = width;
            open.start_cap_locked = false;
            open.end_cap_locked = false;
            // Preserve the old closed seam's width when opening elsewhere.
            if i != 0 {
                open.widths
                    .push(width_key(d, original.nodes[0].id, 0., original.start_width));
            }
            open.corners.retain(|c| c.node != at.node);
            let end = open.nodes.last().unwrap().id;
            d.strokes[si] = open;
            return Ok(vec![
                at,
                Anchor {
                    stroke: at.stroke,
                    node: end,
                },
            ]);
        }
        let mut left = original.clone();
        let mut right = original.clone();
        right.id = d.id();
        right.name.push_str(" / 分離");
        left.nodes = original.nodes[..=i].to_vec();
        right.nodes = original.nodes[i..].to_vec();
        let new_id = d.id();
        right.nodes[0].id = new_id;
        right
            .widths
            .iter_mut()
            .filter(|k| k.segment == at.node)
            .for_each(|k| k.segment = new_id);
        left.end_width = width;
        right.start_width = width;
        left.end_cap_locked = false;
        right.start_cap_locked = false;
        left.corners.retain(|c| c.node != at.node);
        right.corners.retain(|c| c.node != at.node);
        keep_keys(&mut left);
        keep_keys(&mut right);
        // These are no longer used by the split paths, but keeping them would make
        // a later join unexpectedly curve its connecting segment.
        left.nodes.last_mut().unwrap().outgoing = Vec2::ZERO;
        right.nodes[0].incoming = Vec2::ZERO;
        let result = vec![
            at,
            Anchor {
                stroke: right.id,
                node: new_id,
            },
        ];
        d.strokes[si] = left;
        d.strokes.insert(si + 1, right);
        Ok(result)
    })
}
fn reverse_width(w: &mut Width) {
    std::mem::swap(&mut w.left, &mut w.right);
}
/// Reverse traversal while preserving the physical left/right ink and width-key locations.
pub fn reversed(s: &Stroke) -> Stroke {
    let mut out = s.clone();
    out.nodes.reverse();
    for n in &mut out.nodes {
        std::mem::swap(&mut n.incoming, &mut n.outgoing);
        std::mem::swap(&mut n.exact_incoming, &mut n.exact_outgoing);
    }
    for k in &mut out.widths {
        if let Some(i) = s.segment_index(k.segment) {
            k.segment = s.nodes[(i + 1) % s.nodes.len()].id;
            k.t = 1. - k.t;
            reverse_width(&mut k.width);
        }
    }
    std::mem::swap(&mut out.start_width, &mut out.end_width);
    reverse_width(&mut out.start_width);
    reverse_width(&mut out.end_width);
    std::mem::swap(&mut out.start_angle, &mut out.end_angle);
    std::mem::swap(&mut out.start_cap_locked, &mut out.end_cap_locked);
    out
}
fn join_impl(d: &mut Document, a: Anchor, b: Anchor) -> Result<Anchor, String> {
    let (ai, an) = index(d, a)?;
    let (bi, bn) = index(d, b)?;
    let is_end = |s: &Stroke, n: usize| {
        !s.closed && s.nodes.len() >= 2 && (n == 0 || n + 1 == s.nodes.len())
    };
    if a == b || !is_end(&d.strokes[ai], an) || !is_end(&d.strokes[bi], bn) {
        return Err("開いたパスの端点を2つ選択してください".into());
    }
    if ai == bi {
        let mut s = d.strokes[ai].clone();
        s.closed = true;
        let last = s.nodes.last_mut().unwrap();
        last.outgoing = Vec2::ZERO;
        last.exact_outgoing = false;
        last.smooth = false;
        let id = last.id;
        s.nodes[0].incoming = Vec2::ZERO;
        s.nodes[0].exact_incoming = false;
        s.nodes[0].smooth = false;
        s.widths.push(width_key(d, id, 0., s.end_width));
        d.strokes[ai] = s;
        return Ok(a);
    }
    let mut left = if an == 0 {
        reversed(&d.strokes[ai])
    } else {
        d.strokes[ai].clone()
    };
    let right = if bn == 0 {
        d.strokes[bi].clone()
    } else {
        reversed(&d.strokes[bi])
    };
    let seam = left.nodes.last().unwrap().id;
    let n = left.nodes.last_mut().unwrap();
    n.outgoing = Vec2::ZERO;
    n.exact_outgoing = false;
    n.smooth = false;
    let mut nodes = right.nodes.clone();
    nodes[0].incoming = Vec2::ZERO;
    nodes[0].exact_incoming = false;
    nodes[0].smooth = false;
    left.widths.push(width_key(d, seam, 0., left.end_width));
    left.widths.push(width_key(d, seam, 1., right.start_width));
    left.widths.extend(right.widths);
    left.end_width = right.end_width;
    left.end_angle = right.end_angle;
    left.end_cap_locked = right.end_cap_locked;
    left.nodes.extend(nodes);
    left.corners.extend(right.corners);
    d.strokes[ai] = left;
    d.strokes.remove(bi);
    Ok(Anchor {
        stroke: a.stroke,
        node: seam,
    })
}
/// Join preserves the first selected path's paint and effect graph.
pub fn join(doc: &mut Document, a: Anchor, b: Anchor) -> Result<Anchor, String> {
    transaction(doc, |d| join_impl(d, a, b))
}
fn merge_adjacent(d: &mut Document, stroke: u64, a: u64, b: u64) -> Result<Anchor, String> {
    let (si, ai) = index(d, Anchor { stroke, node: a })?;
    let (_, bi) = index(d, Anchor { stroke, node: b })?;
    let s = &mut d.strokes[si];
    let count = s.nodes.len();
    if count <= if s.closed { 3 } else { 2 } {
        return Err("これ以上点を減らせません".into());
    }
    let (i, j) = if (ai + 1) % count == bi && (s.closed || ai + 1 < count) {
        (ai, bi)
    } else if (bi + 1) % count == ai && (s.closed || bi + 1 < count) {
        (bi, ai)
    } else {
        return Err("隣り合う点、または別パスの端点を選択してください".into());
    };
    let a = s.nodes[i].clone();
    let b = s.nodes[j].clone();
    let p = a.p.lerp(b.p, 0.5);
    let mut merged = a.clone();
    merged.p = p;
    merged.incoming = a.p + a.incoming - p;
    merged.outgoing = b.p + b.outgoing - p;
    merged.exact_outgoing = b.exact_outgoing;
    merged.smooth = false;
    // No hidden curve should appear where an external handle was collapsed.
    if a.incoming.hypot() < 1e-9 {
        merged.incoming = Vec2::ZERO;
    }
    if b.outgoing.hypot() < 1e-9 {
        merged.outgoing = Vec2::ZERO;
    }
    s.widths.retain(|k| k.segment != a.id);
    for k in &mut s.widths {
        if k.segment == b.id {
            k.segment = a.id;
        }
    }
    s.corners.retain(|c| c.node != a.id && c.node != b.id);
    if j == 0 {
        s.nodes[0] = merged;
        s.nodes.remove(i);
    } else {
        s.nodes[i] = merged;
        s.nodes.remove(j);
    }
    keep_keys(s);
    Ok(Anchor { stroke, node: a.id })
}
pub fn merge(doc: &mut Document, a: Anchor, b: Anchor) -> Result<Anchor, String> {
    transaction(doc, |d| {
        if a.stroke != b.stroke {
            join_impl(d, a, b)?;
            return merge_adjacent(d, a.stroke, a.node, b.node);
        }
        merge_adjacent(d, a.stroke, a.node, b.node)
    })
}
pub fn delete_anchors(doc: &mut Document, anchors: &[Anchor]) -> Result<(), String> {
    transaction(doc, |d| {
        for at in anchors {
            let (si, ni) = index(d, *at)?;
            let s = &mut d.strokes[si];
            s.nodes.remove(ni);
            if s.nodes.len() < 3 {
                s.closed = false;
            }
            keep_keys(s);
        }
        d.strokes.retain(|s| !s.nodes.is_empty());
        Ok(())
    })
}
/// Merge visible outlines into one editable contour object. Paint is taken from the first path.
pub fn union(doc: &mut Document, ids: &[u64]) -> Result<u64, String> {
    transaction(doc, |d| {
        if ids.len() < 2 {
            return Err("合体するパスを2つ以上選択してください".into());
        }
        let paths: Vec<_> = ids
            .iter()
            .map(|id| {
                d.strokes
                    .iter()
                    .find(|s| s.id == *id)
                    .ok_or("パスがありません")
            })
            .collect::<Result<_, _>>()?;
        let mut contours = vec![];
        for s in &paths {
            contours.extend(outline::paint(s)?.contours);
        }
        let contours = outline::boolean_contours(
            &contours,
            &[],
            i_overlay::core::overlay_rule::OverlayRule::Subject,
        );
        if contours.is_empty() {
            return Err("合体する輪郭がありません".into());
        }
        let first = paths[0];
        let mut result = Stroke::new(first.id, format!("{} / 合体", first.name));
        result.fill = Some(first.fill.or(first.edge).unwrap_or([224; 3]));
        result.edge = None;
        result.contours = Some(contours);
        let id = result.id;
        let position = d.strokes.iter().position(|s| s.id == id).unwrap();
        d.strokes[position] = result;
        d.strokes.retain(|s| s.id == id || !ids.contains(&s.id));
        Ok(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use kurbo::Point;
    fn fixture() -> Document {
        let mut d = Document::default();
        for offset in [0., 200.] {
            let mut s = Stroke::new(d.id(), "線".into());
            for x in [0., 60., 120.] {
                s.nodes.push(Node {
                    id: d.id(),
                    p: Point::new(x + offset, 0.),
                    incoming: Vec2::new(-12., 10.),
                    outgoing: Vec2::new(15., -12.),
                    smooth: true,
                    exact_incoming: false,
                    exact_outgoing: false,
                });
            }
            s.start_width.left = 10.;
            s.end_width.left = 30.;
            let segment = s.nodes[1].id;
            s.widths.push(width_key(
                &mut d,
                segment,
                0.5,
                Width {
                    left: 42.,
                    right: 15.,
                    linked: false,
                    angle: 12.,
                },
            ));
            s.appearance = Some(crate::appearance::Graph::rough());
            d.strokes.push(s);
        }
        d
    }
    fn at(d: &Document, s: usize, n: usize) -> Anchor {
        Anchor {
            stroke: d.strokes[s].id,
            node: d.strokes[s].nodes[n].id,
        }
    }
    #[test]
    fn anchor_conversion_and_line_change_only_intended_handles() {
        let mut d = fixture();
        let a = at(&d, 0, 1);
        let old = d.strokes[0].nodes[0].incoming;
        set_anchor_kind(&mut d, &[a], AnchorKind::Corner).unwrap();
        assert_eq!(d.strokes[0].nodes[1].outgoing, Vec2::ZERO);
        set_anchor_kind(&mut d, &[a], AnchorKind::Smooth).unwrap();
        let n = &d.strokes[0].nodes[1];
        assert!(n.incoming.cross(n.outgoing).abs() < 1e-8);
        let seg = d.strokes[0].nodes[0].id;
        straighten(&mut d, a.stroke, seg).unwrap();
        assert_eq!(d.strokes[0].nodes[0].incoming, old);
        for t in [0.1, 0.5, 0.9] {
            assert!(d.strokes[0].eval(0, t).y.abs() < 1e-8);
        }
    }
    #[test]
    fn separation_keeps_curve_keys_effects_and_unique_ids() {
        let mut d = fixture();
        let original = d.strokes[0].clone();
        let a = at(&d, 0, 1);
        let result = separate(&mut d, a).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(d.strokes.len(), 3);
        assert_eq!(d.strokes[0].cubic(0), original.cubic(0));
        assert_eq!(d.strokes[1].cubic(0), original.cubic(1));
        assert_eq!(d.strokes[1].appearance, original.appearance);
        assert_eq!(d.strokes[0].end_width, d.strokes[1].start_width);
        assert_eq!(d.strokes[1].widths[0].segment, result[1].node);
        d.validate().unwrap();
    }
    #[test]
    fn closed_path_opens_without_losing_its_last_segment() {
        let mut d = fixture();
        d.strokes[0].closed = true;
        let old = d.strokes[0].clone();
        let a = at(&d, 0, 1);
        separate(&mut d, a).unwrap();
        let s = &d.strokes[0];
        assert!(!s.closed);
        assert_eq!(s.segment_count(), 3);
        for i in 0..3 {
            assert_eq!(s.cubic(i), old.cubic((i + 1) % 3));
        }
        assert_eq!(s.nodes.first().unwrap().p, s.nodes.last().unwrap().p);
        d.validate().unwrap();
    }
    #[test]
    fn reversal_preserves_curve_and_asymmetric_width_positions() {
        let d = fixture();
        let s = &d.strokes[0];
        let r = reversed(s);
        for i in 0..s.segment_count() {
            for t in [0.1, 0.4, 0.9] {
                assert!(
                    r.eval(i, t)
                        .distance(s.eval(s.segment_count() - 1 - i, 1. - t))
                        < 1e-8
                );
            }
        }
        assert_eq!(r.start_width.left, s.end_width.right);
        let k = &s.widths[0];
        let rk = &r.widths[0];
        assert!((rk.t - (1. - k.t)).abs() < 1e-9);
        assert_eq!(rk.width.right, k.width.left);
    }
    #[test]
    fn join_all_endpoint_orientations_and_merge_are_valid() {
        for ai in [0, 2] {
            for bi in [0, 2] {
                let mut d = fixture();
                let a = at(&d, 0, ai);
                let b = at(&d, 1, bi);
                let paint = d.strokes[0].appearance.clone();
                join(&mut d, a, b).unwrap();
                assert_eq!(d.strokes.len(), 1);
                assert_eq!(d.strokes[0].nodes.len(), 6);
                assert_eq!(d.strokes[0].appearance, paint);
                d.validate().unwrap();
                let mut d = fixture();
                let a = at(&d, 0, ai);
                let b = at(&d, 1, bi);
                let m = merge(&mut d, a, b).unwrap();
                assert_eq!(d.strokes[0].nodes.len(), 5);
                assert_eq!(m.stroke, a.stroke);
                d.validate().unwrap();
            }
        }
    }
    #[test]
    fn failing_topology_edit_is_atomic() {
        let mut d = fixture();
        let before = d.clone();
        let a = at(&d, 0, 1);
        let b = at(&d, 1, 2);
        assert!(join(&mut d, a, b).is_err());
        assert_eq!(d, before);
        let endpoint = at(&d, 0, 0);
        assert!(separate(&mut d, endpoint).is_err());
        assert_eq!(d, before);
    }
    #[test]
    fn merged_closed_seam_does_not_leave_dangling_width_keys() {
        let mut d = fixture();
        d.strokes[0].closed = true;
        let id = d.id();
        let mut n = d.strokes[0].nodes[2].clone();
        n.id = id;
        n.p = Point::new(130., 80.);
        d.strokes[0].nodes.push(n);
        let a = at(&d, 0, 3);
        let b = at(&d, 0, 0);
        merge(&mut d, a, b).unwrap();
        d.validate().unwrap();
        assert_eq!(d.strokes[0].nodes.len(), 3);
    }
    #[test]
    fn deleting_anchor_cleans_keys_and_corner_references() {
        let mut d = fixture();
        let a = at(&d, 0, 1);
        delete_anchors(&mut d, &[a]).unwrap();
        d.validate().unwrap();
        assert!(d.strokes[0].widths.is_empty());
    }
    #[test]
    fn union_bakes_effects_and_keeps_disconnected_regions() {
        let mut d = fixture();
        let ids: Vec<_> = d.strokes.iter().map(|s| s.id).collect();
        let id = union(&mut d, &ids).unwrap();
        assert_eq!(id, ids[0]);
        assert_eq!(d.strokes.len(), 1);
        assert!(d.strokes[0].appearance.is_none());
        assert!(d.strokes[0].contours.as_ref().unwrap().len() >= 2);
        d.validate().unwrap();
        assert!(!outline::paint(&d.strokes[0]).unwrap().contours.is_empty());
    }
}
