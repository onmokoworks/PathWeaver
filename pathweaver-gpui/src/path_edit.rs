use crate::app::{Drag, Editing, Tool, Workspace};
use gpui::{prelude::*, *};
use kurbo::Point as KPoint;
use pathweaver_core::{
    editing::{self, Anchor, AnchorKind, MergeTarget},
    outline, snapping,
};

#[derive(Clone, Copy)]
pub enum Operation {
    Kind(AnchorKind),
    Separate,
    Join,
    Merge(MergeTarget),
    Straighten,
    Union,
    Delete,
}

impl Workspace {
    pub fn guide_down(&mut self, p: KPoint, e: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        let b = self.art_bounds.get();
        let left = e.position.x < b.left() + px(20.);
        let top = e.position.y < b.top() + px(20.);
        if left && top {
            return true;
        }
        let index = if left || top {
            self.session.begin();
            self.session.doc.guides.push(pathweaver_core::model::Guide {
                vertical: left,
                position: if left { p.x } else { p.y },
            });
            self.session.doc.guides.len() - 1
        } else if self.tool == Tool::Select && self.hit_art(p).is_none() {
            let hit = self
                .session
                .doc
                .guides
                .iter()
                .enumerate()
                .map(|(i, g)| (i, (if g.vertical { p.x } else { p.y } - g.position).abs()))
                .filter(|v| v.1 < 6. / self.art_view.zoom as f64)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((i, _)) = hit else {
                return false;
            };
            self.session.begin();
            i
        } else {
            return false;
        };
        self.drag = Drag::Guide { index };
        self.pen_path = None;
        cx.notify();
        true
    }
    pub fn start_path_move(&mut self, p: KPoint) {
        let nearest = self
            .session
            .doc
            .strokes
            .iter()
            .filter(|s| self.session.selected.contains(&s.id))
            .flat_map(|s| {
                s.nodes.iter().map(|n| {
                    (
                        Anchor {
                            stroke: s.id,
                            node: n.id,
                        },
                        n.p,
                    )
                })
            })
            .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)));
        let origin = nearest
            .map(|v| v.1)
            .or_else(|| {
                self.session
                    .bounds_cached(&self.outlines)
                    .map(|r| KPoint::new(r.x0, r.y0))
            })
            .unwrap_or(p);
        self.capture_frames(nearest.map(|v| v.0));
        self.drag = Drag::Move {
            start: p,
            origin,
            last: origin,
        };
    }
    pub fn capture_frames(&mut self, primary: Option<Anchor>) {
        self.snap_frames.clear();
        // Use the gesture's anchor first; never recompute its direction while dragging.
        if let Some(a) = primary
            && let Some(s) = self.session.doc.strokes.iter().find(|s| s.id == a.stroke)
            && let Some(f) = snapping::frame(s, a.node)
        {
            self.snap_frames.push(f);
        }
        for s in self.session.doc.strokes.iter().filter(|s| {
            s.visible
                && s.contours.is_none()
                && (self.tool != Tool::Select || !self.session.selected.contains(&s.id))
        }) {
            for n in &s.nodes {
                let a = Anchor {
                    stroke: s.id,
                    node: n.id,
                };
                if Some(a) != primary
                    && !self.session.anchors.contains(&a)
                    && let Some(f) = snapping::frame(s, n.id)
                {
                    self.snap_frames.push(f);
                }
            }
        }
    }
    pub fn snap_point(&mut self, p: KPoint, bypass: bool) -> KPoint {
        let (p, guide) = if bypass {
            (p, None)
        } else {
            snapping::snap(
                &self.session.doc,
                p,
                &self.snap_frames,
                self.art_view.zoom as f64,
            )
        };
        self.snap_guide = guide;
        p
    }
    pub fn direct_down(&mut self, p: KPoint, e: &MouseDownEvent, cx: &mut Context<Self>) -> bool {
        let tolerance = 7. / self.art_view.zoom as f64;
        // Anchors are selectable across paths; handles only belong to selected paths.
        let mut hit = None;
        for s in self
            .session
            .doc
            .strokes
            .iter()
            .rev()
            .filter(|s| s.visible && s.contours.is_none())
        {
            for n in &s.nodes {
                if n.p.distance(p) < tolerance {
                    hit = Some((
                        Anchor {
                            stroke: s.id,
                            node: n.id,
                        },
                        0,
                    ));
                    break;
                }
            }
            if hit.is_some() {
                break;
            }
        }
        if hit.is_none() && self.tool == Tool::Direct {
            'paths: for s in self
                .session
                .doc
                .strokes
                .iter()
                .rev()
                .filter(|s| s.visible && self.session.selected.contains(&s.id))
            {
                for n in &s.nodes {
                    for (v, h) in [(n.incoming, -1), (n.outgoing, 1)] {
                        if v.hypot() > 0.1 && (n.p + v).distance(p) < tolerance {
                            hit = Some((
                                Anchor {
                                    stroke: s.id,
                                    node: n.id,
                                },
                                h,
                            ));
                            break 'paths;
                        }
                    }
                }
            }
        }
        if let Some((at, handle)) = hit {
            self.session
                .select_anchor(at, e.modifiers.shift && handle == 0);
            if !self.session.anchors.contains(&at) {
                cx.notify();
                return true;
            }
            self.capture_frames(Some(at));
            self.session.begin();
            if self.tool == Tool::Convert {
                if let Err(e) =
                    editing::set_anchor_kind(&mut self.session.doc, &[at], AnchorKind::Corner)
                {
                    self.status = e;
                }
                self.drag = Drag::Pen { node: at.node };
            } else {
                self.drag = Drag::Anchor {
                    node: at.node,
                    handle,
                };
            }
            self.refresh(cx);
            return true;
        }
        let nearest = self
            .session
            .doc
            .strokes
            .iter()
            .rev()
            .filter(|s| s.visible && s.contours.is_none())
            .filter_map(|s| outline::nearest(s, p).map(|(i, t, d)| (s.id, s.nodes[i].id, i, t, d)))
            .filter(|v| v.4 < tolerance)
            .min_by(|a, b| a.4.total_cmp(&b.4));
        if let Some((stroke, node, segment, t, _)) = nearest {
            self.session.select(stroke, false);
            self.session.segment = Some(Anchor { stroke, node });
            if e.modifiers.alt {
                self.path_operation(Operation::Straighten, cx);
            } else if e.click_count == 2 {
                self.session.begin();
                match editing::insert_anchor(&mut self.session.doc, stroke, segment, t) {
                    Ok(at) => {
                        self.session.select_anchor(at, false);
                        self.status = "形を保ったまま点を追加しました".into();
                    }
                    Err(e) => self.status = e,
                }
                self.session.commit();
                self.refresh(cx);
            } else {
                self.status = "線分を選択：右側で直線化 / ダブルクリックで点を追加".into();
                cx.notify();
            }
            return true;
        }
        // Empty canvas in direct selection always starts an anchor marquee.
        if !e.modifiers.shift {
            self.session.anchors.clear();
            self.session.selected.clear();
            self.session.segment = None;
        }
        self.drag = Drag::Marquee {
            start: p,
            current: p,
            add: e.modifiers.shift,
        };
        cx.notify();
        true
    }
    pub fn path_operation(&mut self, op: Operation, cx: &mut Context<Self>) {
        let anchors = self.session.anchors.clone();
        self.session.begin();
        let result = (|| -> Result<(), String> {
            match op {
                Operation::Kind(kind) => {
                    if anchors.is_empty() {
                        return Err("制御点 A で点を選択してください".into());
                    }
                    editing::set_anchor_kind(&mut self.session.doc, &anchors, kind)?;
                }
                Operation::Separate => {
                    if anchors.len() != 1 {
                        return Err("分離する点を1つ選択してください".into());
                    }
                    let selected = editing::separate(&mut self.session.doc, anchors[0])?;
                    self.session.anchors.clear();
                    for at in selected {
                        self.session.select_anchor(at, true);
                    }
                }
                Operation::Join => {
                    if anchors.len() != 2 {
                        return Err("Shift＋クリックで端点を2つ選択してください".into());
                    }
                    let at = editing::join(&mut self.session.doc, anchors[0], anchors[1])?;
                    self.session.anchors.clear();
                    self.session.select_anchor(at, false);
                }
                Operation::Merge(target) => {
                    let at = editing::merge_selected(&mut self.session.doc, &anchors, target)?;
                    self.session.anchors.clear();
                    self.session.select_anchor(at, false);
                    self.merge_open = false;
                }
                Operation::Straighten => {
                    let at = self
                        .session
                        .segment
                        .or_else(|| anchors.last().copied())
                        .ok_or("線分または始点を選択してください")?;
                    editing::straighten(&mut self.session.doc, at.stroke, at.node)?;
                }
                Operation::Union => {
                    let id = editing::union(&mut self.session.doc, &self.session.selected)?;
                    self.session.selected = vec![id];
                    self.session.anchors.clear();
                    self.session.segment = None;
                }
                Operation::Delete => {
                    if anchors.is_empty() {
                        return Err("削除する点を選択してください".into());
                    }
                    editing::delete_anchors(&mut self.session.doc, &anchors)?;
                }
            }
            Ok(())
        })();
        self.session.commit();
        self.status = match result {
            Err(e) => e,
            Ok(()) => match op {
                Operation::Union => {
                    "輪郭を合体しました。効果を焼き込み、最初に選択したパスの単色になります"
                }
                Operation::Join | Operation::Merge(_) => {
                    "編集しました。別パスの場合は最初に選択したパスの色と効果を引き継ぎます"
                }
                _ => "パスを編集しました（⌘Z で元に戻す）",
            }
            .into(),
        };
        self.pen_path = None;
        self.refresh(cx);
    }
    pub fn anchor_properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div().flex().flex_col().gap_2();
        if matches!(self.tool, Tool::Direct | Tool::Convert) {
            col = col.child(div().text_color(rgb(0x9fb9d5)).child(format!(
                "制御点  ·  {} 点を選択",
                self.session.anchors.len()
            )));
            if self.merge_open {
                col = col.child(div().text_color(rgb(0xf0a85b)).child("点のマージ先"));
                for (label, target) in [
                    ("中心へ", MergeTarget::Center),
                    ("最初に選択した点へ", MergeTarget::First),
                    ("最後に選択した点へ", MergeTarget::Last),
                ] {
                    col = col.child(self.button(label, false, cx, move |s, _, cx| {
                        s.path_operation(Operation::Merge(target), cx)
                    }));
                }
                col = col.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0xaaaaaa))
                        .child("中心＝選択点の平均座標。最初／最後は点を選んだ順番です。"),
                );
            }
            if let Some(p) = self.session.anchor_position() {
                for (kind, label, value) in
                    [(Editing::AnchorX, "X", p.x), (Editing::AnchorY, "Y", p.y)]
                {
                    col = col.child(self.button(
                        format!("{label}  {value:.3}"),
                        false,
                        cx,
                        move |s, w, cx| s.start_edit(kind.clone(), value.to_string(), w, cx),
                    ));
                }
                if self.session.anchors.len() > 1 {
                    col = col.child("座標は最後の点を基準に一緒に移動");
                }
                let mut row = div().flex().flex_wrap().gap_1();
                for (label, kind) in [
                    ("角", AnchorKind::Corner),
                    ("スムーズ", AnchorKind::Smooth),
                    ("独立", AnchorKind::Independent),
                ] {
                    row = row.child(self.button(label, false, cx, move |s, _, cx| {
                        s.path_operation(Operation::Kind(kind), cx)
                    }));
                }
                col = col.child(row);
                for (label, op) in [
                    ("点で分離", Operation::Separate),
                    ("端点を連結", Operation::Join),
                ] {
                    col = col.child(
                        self.button(label, false, cx, move |s, _, cx| s.path_operation(op, cx)),
                    );
                }
                col =
                    col.child(self.button("点をマージ M", self.merge_open, cx, |s, w, _| {
                        s.merge_open = !s.merge_open;
                        s.focus.focus(w);
                    }));
            }
            if self.session.segment.is_some() || !self.session.anchors.is_empty() {
                col = col.child(self.button("次の線分を直線化", false, cx, |s, _, cx| {
                    s.path_operation(Operation::Straighten, cx)
                }));
            }
            col=col.child(div().text_size(px(11.)).text_color(rgb(0xaaaaaa)).child("Shift：点を複数選択\n曲線をダブルクリック：点を追加\n⌥＋線分クリック：直線化\nアンカー ⇧C：クリックで角、ドラッグで曲線"));
        }
        if self.session.selected.len() > 1 {
            col = col
                .child(self.button("輪郭を合体", false, cx, |s, _, cx| {
                    s.path_operation(Operation::Union, cx)
                }))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0xaaaaaa))
                        .child("効果を焼き込み、最初のパスの単色に変換"),
                );
        }
        col.into_any_element()
    }
    pub fn snap_properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_color(rgb(0xaaaaaa)).child("スナップ"));
        for (index, label, on) in [
            (0, "接線", self.session.doc.snap_tangent),
            (1, "法線", self.session.doc.snap_normal),
            (2, "ガイド", self.session.doc.snap_guides),
            (3, "グリッド", self.session.doc.snap_grid),
        ] {
            col = col.child(self.button(
                format!("{label}吸着：{}", if on { "オン" } else { "オフ" }),
                on,
                cx,
                move |s, _, _| {
                    s.session.edit(|s| {
                        let target = match index {
                            0 => &mut s.doc.snap_tangent,
                            1 => &mut s.doc.snap_normal,
                            2 => &mut s.doc.snap_guides,
                            _ => &mut s.doc.snap_grid,
                        };
                        *target = !*target;
                    })
                },
            ));
        }
        let spacing = self.session.doc.grid_spacing;
        col = col.child(self.button(
            format!("グリッド間隔  {spacing}"),
            false,
            cx,
            move |s, w, cx| s.start_edit(Editing::GridSpacing, spacing.to_string(), w, cx),
        ));
        col.child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0xaaaaaa))
                .child("定規からドラッグ：ガイド追加\nVでガイド移動、外へ戻すと削除"),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0xaaaaaa))
                .child("Control：一時的に吸着を解除"),
        )
        .into_any_element()
    }
}
