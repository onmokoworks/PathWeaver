//! Anchor and topology edits for lettering. Operations validate a copy before committing.
use crate::{
    model::{Document, Stroke, Width, WidthKey},
    outline,
};
use kurbo::{Point, Vec2};

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
        interpolated: false,
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
/// Destination is based on selection order, independently of path traversal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeTarget {
    Center,
    First,
    Last,
}

/// Collapse one connected run of anchors, or weld two open-path endpoints.
/// Disconnected interior selections would require a branching path graph.
pub fn merge_selected(
    doc: &mut Document,
    anchors: &[Anchor],
    target: MergeTarget,
) -> Result<Anchor, String> {
    transaction(doc, |d| {
        let mut selected = Vec::new();
        for at in anchors {
            index(d, *at)?;
            if !selected.contains(at) {
                selected.push(*at);
            }
        }
        if selected.len() < 2 {
            return Err("まとめる点を2つ以上選択してください".into());
        }
        let positions = selected
            .iter()
            .map(|at| {
                let (si, ni) = index(d, *at).unwrap();
                d.strokes[si].nodes[ni].p
            })
            .collect::<Vec<_>>();
        let p = match target {
            MergeTarget::First => positions[0],
            MergeTarget::Last => *positions.last().unwrap(),
            MergeTarget::Center => (positions
                .iter()
                .fold(Vec2::ZERO, |sum, p| sum + p.to_vec2())
                / positions.len() as f64)
                .to_point(),
        };
        let retained = if target == MergeTarget::Last {
            selected.last().unwrap().node
        } else {
            selected[0].node
        };
        let stroke = selected[0].stroke;
        if selected.iter().any(|at| at.stroke != stroke) {
            if selected.len() != 2 {
                return Err("別パス間では開いたパスの端点を2つ選択してください".into());
            }
            join_impl(d, selected[0], selected[1])?;
        }
        let si = d.strokes.iter().position(|s| s.id == stroke).unwrap();
        let s = &mut d.strokes[si];
        let ids: Vec<_> = selected.iter().map(|at| at.node).collect();
        let count = s.nodes.len();
        let all = ids.len() == count;
        let chosen = |i: usize| ids.contains(&s.nodes[i].id);
        // Welding an open path's two ends closes its remaining centerline.
        if !s.closed && !all && chosen(0) && chosen(count - 1) {
            s.closed = true;
        }
        let starts: Vec<_> = (0..count)
            .filter(|&i| {
                chosen(i)
                    && (if i == 0 {
                        !s.closed || !chosen(count - 1)
                    } else {
                        !chosen(i - 1)
                    })
            })
            .collect();
        if !all && starts.len() != 1 {
            return Err("同じパスでは連続した点を選択してください（離れた途中の点は分岐になるため未対応です）".into());
        }
        let head = if all { 0 } else { starts[0] };
        let mut tail = head;
        for _ in 1..ids.len() {
            tail = (tail + 1) % count;
        }
        let a = s.nodes[head].clone();
        let b = s.nodes[tail].clone();
        let mut merged = a.clone();
        merged.id = retained;
        merged.p = p;
        merged.incoming = if all || a.incoming.hypot() < 1e-9 {
            Vec2::ZERO
        } else {
            a.p + a.incoming - p
        };
        merged.outgoing = if all || b.outgoing.hypot() < 1e-9 {
            Vec2::ZERO
        } else {
            b.p + b.outgoing - p
        };
        merged.exact_outgoing = b.exact_outgoing;
        merged.smooth = false;
        // Only the last selected node's outgoing segment survives the collapse.
        s.widths
            .retain(|k| !ids.contains(&k.segment) || (!all && k.segment == b.id));
        for key in &mut s.widths {
            if key.segment == b.id {
                key.segment = retained;
            }
        }
        s.corners.retain(|c| !ids.contains(&c.node));
        let insertion = (0..count).find(|&i| ids.contains(&s.nodes[i].id)).unwrap();
        let mut nodes = Vec::with_capacity(count - ids.len() + 1);
        for (i, n) in s.nodes.iter().enumerate() {
            if i == insertion {
                nodes.push(merged.clone());
            }
            if !ids.contains(&n.id) {
                nodes.push(n.clone());
            }
        }
        s.nodes = nodes;
        if all {
            s.closed = false;
        }
        keep_keys(s);
        Ok(Anchor {
            stroke,
            node: retained,
        })
    })
}
/// Compatibility helper for merging a pair at its center.
pub fn merge(doc: &mut Document, a: Anchor, b: Anchor) -> Result<Anchor, String> {
    merge_selected(doc, &[a, b], MergeTarget::Center)
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
        result.take_fill_from(first);
        result.contours = Some(contours);
        let id = result.id;
        let position = d.strokes.iter().position(|s| s.id == id).unwrap();
        d.strokes[position] = result;
        d.strokes.retain(|s| s.id == id || !ids.contains(&s.id));
        Ok(id)
    })
}

/// Illustrator-style Pathfinder on whole paths (their painted outlines,
/// effects baked in). Stacking order decides "front" and "back".
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum PathfinderOp {
    /// 合体
    Unite,
    /// 前面で型抜き: the backmost path minus everything in front of it
    MinusFront,
    /// 背面で型抜き: the frontmost path minus everything behind it
    MinusBack,
    /// 交差: only where all paths overlap
    Intersect,
    /// 中マド: where an odd number of paths overlap
    Exclude,
    /// 分割: every region becomes its own path, coloured by the topmost path over it
    Divide,
}

/// Returns the ids of the resulting paths (front-most last).
pub fn pathfinder(doc: &mut Document, ids: &[u64], op: PathfinderOp) -> Result<Vec<u64>, String> {
    use i_overlay::core::overlay_rule::OverlayRule;
    transaction(doc, |d| {
        // selected paths in stacking order (back first)
        let order: Vec<usize> = (0..d.strokes.len()).filter(|&i| ids.contains(&d.strokes[i].id)).collect();
        if order.len() < 2 {
            return Err("パスファインダーにはパスを2つ以上選択してください".into());
        }
        let shapes: Vec<Vec<Vec<Point>>> = order
            .iter()
            .map(|&i| outline::paint(&d.strokes[i]).map(|p| p.contours))
            .collect::<Result<_, _>>()?;
        // The source whose paint a result takes (by stacking index).
        let color = |i: usize| i;
        let union_of = |list: &[Vec<Vec<Point>>]| {
            let all: Vec<Vec<Point>> = list.iter().flatten().cloned().collect();
            outline::boolean_contours(&all, &[], OverlayRule::Subject)
        };
        let last = order.len() - 1;
        // (contours, colour, the source path whose place/folder it takes)
        let mut results: Vec<(Vec<Vec<Point>>, usize, usize)> = match op {
            PathfinderOp::Unite => vec![(union_of(&shapes), color(last), last)],
            PathfinderOp::MinusFront => vec![(
                outline::boolean_contours(&shapes[0], &union_of(&shapes[1..]), OverlayRule::Difference),
                color(0),
                0,
            )],
            PathfinderOp::MinusBack => vec![(
                outline::boolean_contours(&shapes[last], &union_of(&shapes[..last]), OverlayRule::Difference),
                color(last),
                last,
            )],
            PathfinderOp::Intersect => {
                let mut acc = outline::boolean_contours(&shapes[0], &[], OverlayRule::Subject);
                for s in &shapes[1..] {
                    acc = outline::boolean_contours(&acc, s, OverlayRule::Intersect);
                }
                vec![(acc, color(last), last)]
            }
            PathfinderOp::Exclude => {
                let mut acc = outline::boolean_contours(&shapes[0], &[], OverlayRule::Subject);
                for s in &shapes[1..] {
                    acc = outline::boolean_contours(&acc, s, OverlayRule::Xor);
                }
                vec![(acc, color(last), last)]
            }
            PathfinderOp::Divide => {
                // faces: (region, topmost path index over it)
                let mut faces: Vec<(Vec<Vec<Point>>, usize)> = vec![];
                let mut covered: Vec<Vec<Point>> = vec![];
                for (i, s) in shapes.iter().enumerate() {
                    let mut next = vec![];
                    for (f, owner) in faces {
                        let inside = outline::boolean_contours(&f, s, OverlayRule::Intersect);
                        let outside = outline::boolean_contours(&f, s, OverlayRule::Difference);
                        if !inside.is_empty() { next.push((inside, i)); }
                        if !outside.is_empty() { next.push((outside, owner)); }
                    }
                    let fresh = outline::boolean_contours(s, &covered, OverlayRule::Difference);
                    if !fresh.is_empty() { next.push((fresh, i)); }
                    covered = outline::boolean_contours(&covered, s, OverlayRule::Union);
                    faces = next;
                }
                // every separate region becomes its own path
                let mut out = vec![];
                for (f, owner) in faces {
                    for shape in outline::boolean_shapes(&f, &[], OverlayRule::Subject) {
                        out.push((shape, color(owner), owner));
                    }
                }
                out
            }
        };
        results.retain(|r| !r.0.is_empty());
        if results.is_empty() {
            return Err("結果の形がありません（重なりがないなど）".into());
        }
        // Results replace the sources, placed where the front-most source was.
        let front = order[last];
        let anchor_id = d.strokes[front].id;
        let folder = d.strokes[front].folder;
        let label = match op {
            PathfinderOp::Unite => "合体",
            PathfinderOp::MinusFront => "前面で型抜き",
            PathfinderOp::MinusBack => "背面で型抜き",
            PathfinderOp::Intersect => "交差",
            PathfinderOp::Exclude => "中マド",
            PathfinderOp::Divide => "分割",
        };
        let mut new_strokes = vec![];
        for (k, (contours, color, src)) in results.into_iter().enumerate() {
            let (source_id, source_name) = (d.strokes[order[src]].id, d.strokes[order[src]].name.clone());
            let id = if k == 0 { source_id } else { d.id() };
            let name = if k == 0 { format!("{source_name} / {label}") } else { format!("{source_name} / {label} {}", k + 1) };
            let mut s = Stroke::new(id, name);
            s.take_fill_from(&d.strokes[order[color]]);
            s.contours = Some(contours);
            s.folder = folder;
            new_strokes.push(s);
        }
        let ids_out: Vec<u64> = new_strokes.iter().map(|s| s.id).collect();
        let at = d.strokes.iter().position(|s| s.id == anchor_id).unwrap();
        let mut rebuilt = vec![];
        for (i, s) in d.strokes.iter().enumerate() {
            if i == at {
                rebuilt.extend(new_strokes.iter().cloned());
            } else if !ids.contains(&s.id) {
                rebuilt.push(s.clone());
            }
        }
        d.strokes = rebuilt;
        Ok(ids_out)
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
    fn pathfinder_modes_follow_stacking_order() {
        use super::super::model::Document;
        fn square(d: &mut Document, x: f64, color: [u8; 3]) -> u64 {
            let mut s = Stroke::new(d.id(), "sq".into());
            s.fill = Some(color);
            s.edge = None;
            s.contours = Some(vec![vec![Point::new(x, 0.), Point::new(x + 20., 0.), Point::new(x + 20., 20.), Point::new(x, 20.)]]);
            let id = s.id;
            d.strokes.push(s);
            id
        }
        let area = |d: &Document, id: u64| -> f64 {
            let s = d.strokes.iter().find(|s| s.id == id).unwrap();
            s.contours.as_ref().unwrap().iter().map(|r| {
                let mut a = 0.;
                for i in 0..r.len() { let (p, q) = (r[i], r[(i + 1) % r.len()]); a += p.x * q.y - q.x * p.y; }
                a.abs() * 0.5
            }).sum()
        };
        let setup = || { let mut d = Document::default(); let a = square(&mut d, 0., [255, 0, 0]); let b = square(&mut d, 10., [0, 0, 255]); (d, a, b) };
        let (mut d, a, b) = setup();
        let r = pathfinder(&mut d, &[b, a], PathfinderOp::MinusFront).unwrap();
        assert_eq!(r, vec![a]);
        assert!((area(&d, a) - 200.).abs() < 1e-6, "back square minus the front one");
        assert_eq!(d.strokes.len(), 1);
        assert_eq!(d.strokes[0].fill, Some([255, 0, 0]));
        let (mut d, a, b) = setup();
        let r = pathfinder(&mut d, &[a, b], PathfinderOp::Intersect).unwrap();
        assert!((area(&d, r[0]) - 200.).abs() < 1e-6);
        assert_eq!(d.strokes[0].fill, Some([0, 0, 255]), "front colour");
        let (mut d, a, b) = setup();
        let r = pathfinder(&mut d, &[a, b], PathfinderOp::Exclude).unwrap();
        assert!((area(&d, r[0]) - 400.).abs() < 1e-6);
        let (mut d, a, b) = setup();
        let r = pathfinder(&mut d, &[a, b], PathfinderOp::Divide).unwrap();
        assert_eq!(r.len(), 3, "left, overlap, right");
        let total: f64 = r.iter().map(|id| area(&d, *id)).sum();
        assert!((total - 600.).abs() < 1e-6);
        // the overlap belongs to the front (blue) path
        let blue: f64 = d.strokes.iter().filter(|s| s.fill == Some([0, 0, 255])).map(|s| area(&d, s.id)).sum();
        assert!((blue - 400.).abs() < 1e-6);
        let (mut d, a, b) = setup();
        pathfinder(&mut d, &[a, b], PathfinderOp::MinusBack).unwrap();
        assert_eq!(d.strokes[0].fill, Some([0, 0, 255]));
        let (mut d, a, _) = setup();
        assert!(pathfinder(&mut d, &[a], PathfinderOp::Unite).is_err());
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
    fn many_points() -> Document {
        let mut d = Document::default();
        let mut s = Stroke::new(d.id(), "merge".into());
        for (x, y) in [
            (0., 0.),
            (20., 10.),
            (80., 60.),
            (110., -10.),
            (200., 0.),
            (250., 40.),
        ] {
            s.nodes.push(Node {
                id: d.id(),
                p: Point::new(x, y),
                incoming: Vec2::new(-6., 2.),
                outgoing: Vec2::new(9., -4.),
                smooth: false,
                exact_incoming: false,
                exact_outgoing: false,
            });
        }
        let seg = s.nodes[3].id;
        s.widths.push(width_key(&mut d, seg, 0.4, Width::default()));
        d.strokes.push(s);
        d
    }
    #[test]
    fn merge_destinations_use_original_positions_and_selection_order() {
        for target in [MergeTarget::Center, MergeTarget::First, MergeTarget::Last] {
            let mut d = many_points();
            let before = d.strokes[0].clone();
            let selection = [at(&d, 0, 3), at(&d, 0, 1), at(&d, 0, 2)];
            let expected = match target {
                MergeTarget::Center => Point::new(70., 20.),
                MergeTarget::First => before.nodes[3].p,
                MergeTarget::Last => before.nodes[2].p,
            };
            let merged = merge_selected(&mut d, &selection, target).unwrap();
            let s = &d.strokes[0];
            let n = &s.nodes[1];
            assert_eq!(n.p, expected);
            assert_eq!(
                n.id,
                if target == MergeTarget::Last {
                    selection[2].node
                } else {
                    selection[0].node
                }
            );
            assert_eq!(n.id, merged.node);
            assert_eq!(s.nodes.len(), 4);
            assert_eq!(
                n.p + n.incoming,
                before.nodes[1].p + before.nodes[1].incoming
            );
            assert_eq!(
                n.p + n.outgoing,
                before.nodes[3].p + before.nodes[3].outgoing
            );
            assert_eq!(s.widths[0].segment, n.id);
            d.validate().unwrap();
        }
    }
    #[test]
    fn merge_closed_seam_and_open_endpoints_preserve_traversal() {
        for closed in [false, true] {
            let mut d = many_points();
            d.strokes[0].closed = closed;
            let original = d.strokes[0].clone();
            let selection = [at(&d, 0, 5), at(&d, 0, 0)];
            let merged = merge_selected(&mut d, &selection, MergeTarget::Last).unwrap();
            let s = &d.strokes[0];
            assert!(s.closed);
            assert_eq!(s.nodes[0].id, merged.node);
            assert_eq!(s.nodes[0].p, original.nodes[0].p);
            assert_eq!(s.nodes[1].id, original.nodes[1].id);
            assert_eq!(s.nodes[4].id, original.nodes[4].id);
            assert_eq!(
                s.nodes[0].p + s.nodes[0].incoming,
                original.nodes[5].p + original.nodes[5].incoming
            );
            d.validate().unwrap();
        }
    }
    #[test]
    fn merge_cross_path_destinations_survive_reversing_endpoints() {
        for ai in [0, 2] {
            for bi in [0, 2] {
                for target in [MergeTarget::First, MergeTarget::Last] {
                    let mut d = fixture();
                    let selection = [at(&d, 0, ai), at(&d, 1, bi)];
                    let expected = if target == MergeTarget::First {
                        d.strokes[0].nodes[ai].p
                    } else {
                        d.strokes[1].nodes[bi].p
                    };
                    let result = merge_selected(&mut d, &selection, target).unwrap();
                    assert_eq!(d.strokes.len(), 1);
                    assert_eq!(
                        d.strokes[0]
                            .nodes
                            .iter()
                            .find(|n| n.id == result.node)
                            .unwrap()
                            .p,
                        expected
                    );
                    d.validate().unwrap();
                }
            }
        }
    }
    #[test]
    fn merge_rejects_disconnected_interior_selection_atomically() {
        let mut d = many_points();
        let original = d.clone();
        let selection = [at(&d, 0, 1), at(&d, 0, 3)];
        assert!(merge_selected(&mut d, &selection, MergeTarget::Center).is_err());
        assert_eq!(d, original);
        assert!(merge_selected(&mut d, &[selection[0], selection[0]], MergeTarget::First).is_err());
        assert_eq!(d, original);
    }
    #[test]
    fn all_selected_points_collapse_to_one_valid_anchor() {
        for closed in [false, true] {
            let mut d = many_points();
            d.strokes[0].closed = closed;
            let selection: Vec<_> = (0..6).map(|n| at(&d, 0, n)).collect();
            merge_selected(&mut d, &selection, MergeTarget::Center).unwrap();
            assert_eq!(d.strokes[0].nodes.len(), 1);
            assert!(!d.strokes[0].closed);
            assert!(d.strokes[0].widths.is_empty());
            d.validate().unwrap();
        }
    }
}
