use crate::{
    drawing::View,
    input::TextInput,
    session::{Session, atomic_write, inside},
};
use gpui::{prelude::*, *};
use kurbo::{Affine, Point as KPoint, Rect};
use pathweaver_core::{
    appearance::{Graph, Parameter},
    editing::Anchor,
    model::{Document, Join, WidthKey, WidthTarget},
    outline, snapping,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tool {
    Select,
    Direct,
    Convert,
    Pen,
    Width,
}
#[derive(Clone)]
pub enum Drag {
    None,
    Pan {
        nodes: bool,
        last: Point<Pixels>,
    },
    Move {
        last: KPoint,
    },
    Anchor {
        node: u64,
        handle: i8,
    },
    Pen {
        node: u64,
    },
    Width {
        target: WidthTarget,
        left: bool,
    },
    Marquee {
        start: KPoint,
        current: KPoint,
        add: bool,
    },
    Resize,
    Nodes {
        last: KPoint,
    },
    NodeBox {
        start: KPoint,
        current: KPoint,
        add: bool,
    },
    Connect {
        from: u64,
        at: Point<Pixels>,
        lazy: bool,
    },
    Cut {
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
}
#[derive(Clone)]
pub enum Editing {
    AnchorX,
    AnchorY,
    PathName,
    NodeName,
    Parameter(Parameter),
    Color,
    SetName,
    Category,
    Search,
    Width,
    Rotation,
    Scale,
}
pub struct Workspace {
    pub session: Session,
    pub dialog_open: bool,
    pub outlines: Vec<(u64, outline::Outline)>,
    rendered_strokes: Vec<pathweaver_core::model::Stroke>,
    node_owner: Option<u64>,
    pub tool: Tool,
    pub drag: Drag,
    pub art_view: View,
    pub node_view: View,
    pub art_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub node_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub focus: FocusHandle,
    pub nodes_open: bool,
    pub node_height: f32,
    pub node_selection: Vec<u64>,
    pub node_region: bool,
    pub inspector: bool,
    pub add_open: bool,
    pub library_open: bool,
    pub search: String,
    pub set_name: String,
    pub category: String,
    pub input: Entity<TextInput>,
    pub editing: Option<Editing>,
    pub status: String,
    pub pen_path: Option<u64>,
    pub cursor: Point<Pixels>,
    pub merge_open: bool,
    pub snap_frames: Vec<snapping::Frame>,
    pub snap_guide: Option<snapping::Guide>,
}
impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        Self {
            session: Session::default(),
            dialog_open: false,
            outlines: vec![],
            rendered_strokes: vec![],
            node_owner: None,
            tool: Tool::Pen,
            drag: Drag::None,
            art_view: View::default(),
            node_view: View {
                pan: [30., 30.],
                zoom: 0.9,
            },
            art_bounds: Rc::new(Cell::new(Bounds::default())),
            node_bounds: Rc::new(Cell::new(Bounds::default())),
            focus,
            nodes_open: true,
            node_height: 300.,
            node_selection: vec![],
            node_region: false,
            inspector: true,
            add_open: false,
            library_open: false,
            search: String::new(),
            set_name: String::new(),
            category: String::new(),
            input: cx.new(TextInput::new),
            editing: None,
            status: "P：ペン   V：範囲選択   A：制御点   W：幅   ⌘S：保存".into(),
            pen_path: None,
            cursor: point(px(0.), px(0.)),
            merge_open: false,
            snap_frames: vec![],
            snap_guide: None,
        }
    }
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.session.reconcile();
        let previous = std::mem::take(&mut self.outlines);
        for stroke in self.session.doc.strokes.iter().filter(|s| s.visible) {
            let unchanged = self
                .rendered_strokes
                .iter()
                .find(|s| s.id == stroke.id)
                .is_some_and(|old| same_art(old, stroke));
            if unchanged && let Some((id, out)) = previous.iter().find(|(id, _)| *id == stroke.id) {
                self.outlines.push((*id, out.clone()));
                continue;
            }
            match outline::paint(stroke) {
                Ok(out) => self.outlines.push((stroke.id, out)),
                Err(e) => self.status = e,
            }
        }
        self.rendered_strokes = self.session.doc.strokes.clone();
        if let Some(g) = self.session.graph() {
            self.node_selection.retain(|id| g.node(*id).is_some());
        } else {
            self.node_selection.clear();
        }
        cx.notify();
    }
    pub fn confirm_discard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        next: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.session.dirty() {
            next(self, window, cx);
            return;
        }
        if self.dialog_open {
            return;
        }
        self.dialog_open = true;
        let answer = window.prompt(
            PromptLevel::Warning,
            "未保存の変更があります",
            Some("変更を破棄しますか？ 保存する場合はキャンセルしてください。"),
            &["キャンセル", "破棄して続ける"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            let _ = this.update_in(cx, move |s, w, cx| {
                s.dialog_open = false;
                if answer == Ok(1) {
                    next(s, w, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn directory(&self) -> std::path::PathBuf {
        self.session
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_owned())
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| std::path::PathBuf::from(p).join("Documents"))
                    .unwrap_or_default()
            })
    }
    fn save(&mut self, save_as: bool, cx: &mut Context<Self>) {
        if !save_as && let Some(path) = self.session.path.clone() {
            self.status = match self.session.save(&path) {
                Ok(()) => "保存しました".into(),
                Err(e) => e,
            };
            cx.notify();
            return;
        }
        let answer = cx.prompt_for_new_path(&self.directory(), Some("Untitled.pathweaver"));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, move |s, cx| {
                match result {
                    Ok(Ok(Some(mut p))) => {
                        if p.extension().is_none() {
                            p.set_extension("pathweaver");
                        }
                        s.status = match s.session.save(&p) {
                            Ok(()) => "保存しました".into(),
                            Err(e) => e,
                        };
                    }
                    Ok(Err(e)) => s.status = e.to_string(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, |s, _, cx| s.pick_document(cx));
    }
    fn pick_document(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("PathWeaver文書を開く".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, move |s, cx| {
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(p) = paths.first() {
                            match s.session.load(p) {
                                Ok(()) => {
                                    s.pen_path = None;
                                    s.node_selection.clear();
                                    s.status = "ファイルを開きました".into();
                                    s.fit_art();
                                }
                                Err(e) => s.status = e,
                            }
                        }
                    }
                    Ok(Err(e)) => s.status = e.to_string(),
                    _ => {}
                }
                s.refresh(cx);
            });
        })
        .detach();
    }
    pub fn export_text(
        &mut self,
        name: &str,
        extension: &'static str,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let answer = cx.prompt_for_new_path(&self.directory(), Some(name));
        cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, move |s, cx| {
                match result {
                    Ok(Ok(Some(mut p))) => {
                        if p.extension().is_none() {
                            p.set_extension(extension);
                        }
                        s.status = match atomic_write(&p, text.as_bytes()) {
                            Ok(()) => "書き出しました".into(),
                            Err(e) => e,
                        };
                    }
                    Ok(Err(e)) => s.status = e.to_string(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        match outline::svg(&self.session.doc) {
            Ok(svg) => self.export_text("PathWeaver.svg", "svg", svg, cx),
            Err(e) => self.status = e,
        }
    }
    pub fn fit_art(&mut self) {
        if let Some(r) = self.session.bounds(false) {
            let b = self.art_bounds.get();
            let z = ((f32::from(b.size.width) - 100.) / r.width().max(1.) as f32)
                .min((f32::from(b.size.height) - 80.) / r.height().max(1.) as f32)
                .clamp(0.08, 3.);
            self.art_view = View {
                zoom: z,
                pan: [
                    f32::from(b.size.width) * 0.5 - r.center().x as f32 * z,
                    f32::from(b.size.height) * 0.5 - r.center().y as f32 * z,
                ],
            };
        }
    }
    pub fn button(
        &self,
        label: impl Into<SharedString>,
        active: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let label = label.into();
        let completes_edit = label.as_ref() == "適用" || label.as_ref() == "取消";
        div()
            .id(label.clone())
            .px_2()
            .py_1()
            .rounded(px(3.))
            .text_size(px(12.))
            .cursor_pointer()
            .bg(rgb(if active { 0x526777 } else { 0x383838 }))
            .hover(|s| s.bg(rgb(0x4b4b4b)))
            .child(label)
            .on_click(cx.listener(move |s, _, w, cx| {
                if s.editing.is_some() && !completes_edit {
                    s.commit_edit(w, cx);
                    if s.editing.is_some() {
                        return;
                    }
                }
                f(s, w, cx);
                cx.notify();
            }))
            .into_any_element()
    }
    pub fn start_edit(
        &mut self,
        kind: Editing,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing = Some(kind);
        self.input.update(cx, |i, cx| i.set(value, window, cx));
        cx.notify();
    }
    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.editing.clone() else {
            return;
        };
        let text = self.input.read(cx).content.to_string();
        let before = self.session.doc.clone();
        self.session.begin();
        let result = (|| -> Result<(), String> {
            match kind {
                Editing::AnchorX | Editing::AnchorY => {
                    let v = text
                        .parse::<f64>()
                        .map_err(|_| "座標を数値で入力してください")?;
                    if !v.is_finite() || v.abs() > 1e6 {
                        return Err("座標は ±1,000,000 の範囲です".into());
                    }
                    if let Some(p) = self.session.anchor_position() {
                        let delta = if matches!(kind, Editing::AnchorX) {
                            kurbo::Vec2::new(v - p.x, 0.)
                        } else {
                            kurbo::Vec2::new(0., v - p.y)
                        };
                        self.session.move_anchors(delta);
                    }
                }
                Editing::Search => self.search = text,
                Editing::SetName => {
                    if text.chars().count() > 80 {
                        return Err("名前は80文字以内です".into());
                    }
                    self.set_name = text.clone();
                    if let Some(g) = self.session.graph_mut() {
                        g.name = text;
                    }
                }
                Editing::Category => {
                    if text.chars().count() > 40 {
                        return Err("種類は40文字以内です".into());
                    }
                    self.category = text;
                }
                Editing::PathName => {
                    if text.trim().is_empty() || text.chars().count() > 80 {
                        return Err("名前は1〜80文字です".into());
                    }
                    if let Some(s) = self.session.active_mut() {
                        s.name = text;
                    }
                }
                Editing::NodeName => {
                    if text.chars().count() > 80 {
                        return Err("名前は80文字以内です".into());
                    }
                    let id = self.node_selection.last().copied();
                    if let Some(n) = id.and_then(|id| self.session.graph_mut()?.node_mut(id)) {
                        n.label = text;
                    }
                }
                Editing::Parameter(key) => {
                    let v = text.parse::<f64>().map_err(|_| "数値を入力してください")?;
                    let id = self.node_selection.last().copied();
                    if let Some(n) = id.and_then(|id| self.session.graph_mut()?.node_mut(id)) {
                        if let Some(spec) = n.parameters().iter().find(|p| p.key == key)
                            && (!v.is_finite() || v < spec.min || v > spec.max)
                        {
                            return Err(format!("{}〜{}で入力してください", spec.min, spec.max));
                        }
                        n.set_number(key, v);
                    }
                }
                Editing::Color => {
                    let hex = text.trim().trim_start_matches('#');
                    if hex.len() != 6 {
                        return Err("色は #RRGGBB で入力してください".into());
                    }
                    let v = u32::from_str_radix(hex, 16)
                        .map_err(|_| "色は #RRGGBB で入力してください")?;
                    let rgb = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
                    if self.node_region {
                        let id = self.node_selection.last().copied();
                        if let Some(n) = id.and_then(|id| self.session.graph_mut()?.node_mut(id)) {
                            n.color = rgb;
                        }
                    } else if let Some(s) = self.session.active_mut() {
                        s.edge = Some(rgb);
                    }
                }
                Editing::Width => {
                    let v = text.parse::<f64>().map_err(|_| "数値を入力してください")?;
                    if !v.is_finite() || !(0.1..=1000.).contains(&v) {
                        return Err("幅は0.1〜1000です".into());
                    }
                    for s in &mut self.session.doc.strokes {
                        if self.session.selected.contains(&s.id) {
                            s.set_line_width(v);
                        }
                    }
                }
                Editing::Rotation | Editing::Scale => {
                    let v = text.parse::<f64>().map_err(|_| "数値を入力してください")?;
                    if !v.is_finite() {
                        return Err("有限の数値を入力してください".into());
                    }
                    if let Some(r) = self.session.bounds(true) {
                        let transform = if matches!(kind, Editing::Scale) {
                            if !(0.01..=100.).contains(&v) {
                                return Err("拡大率は0.01〜100です".into());
                            }
                            Affine::scale(v)
                        } else {
                            Affine::rotate(v.to_radians())
                        };
                        self.session.transform(
                            Affine::translate(r.center().to_vec2())
                                * transform
                                * Affine::translate(-r.center().to_vec2()),
                        );
                    }
                }
            }
            self.session.doc.validate()?;
            Ok(())
        })();
        if let Err(e) = result {
            self.session.doc = before;
            self.session.commit();
            self.status = e;
            return;
        }
        self.session.commit();
        self.editing = None;
        self.focus.focus(window);
        self.refresh(cx);
    }
    pub fn edit_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .gap_1()
            .items_center()
            .p_2()
            .bg(rgb(0x343d45))
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .child(self.button("適用", true, cx, |s, w, cx| s.commit_edit(w, cx)))
            .child(self.button("取消", false, cx, |s, w, _| {
                s.editing = None;
                s.focus.focus(w);
            }))
            .into_any_element()
    }
    pub fn hit_art(&self, p: KPoint) -> Option<u64> {
        self.session
            .doc
            .strokes
            .iter()
            .rev()
            .filter(|s| s.visible)
            .find(|s| {
                self.outlines
                    .iter()
                    .find(|(id, _)| *id == s.id)
                    .is_some_and(|(_, out)| inside(p, &out.contours))
                    || outline::nearest(s, p)
                        .is_some_and(|(_, _, distance)| distance < 8. / self.art_view.zoom as f64)
                    || s.nodes
                        .iter()
                        .any(|n| n.p.distance(p) < 8. / self.art_view.zoom as f64)
            })
            .map(|s| s.id)
    }
    fn art_down(&mut self, e: &MouseDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        self.focus.focus(w);
        self.node_region = false;
        let p = self.art_view.world(e.position, self.art_bounds.get());
        if e.button == MouseButton::Middle {
            self.drag = Drag::Pan {
                nodes: false,
                last: e.position,
            };
            return;
        }
        if e.button != MouseButton::Left {
            return;
        }
        if self.tool == Tool::Pen {
            self.session.begin();
            let same = self.pen_path.is_some_and(|id| {
                self.session
                    .active()
                    .is_some_and(|s| s.id == id && !s.closed)
            });
            self.capture_frames(if same {
                self.session.active().and_then(|s| {
                    s.nodes.last().map(|n| Anchor {
                        stroke: s.id,
                        node: n.id,
                    })
                })
            } else {
                None
            });
            let snapped = self.snap_point(p, e.modifiers.control);
            if same {
                let s = self.session.active().unwrap();
                if s.nodes
                    .last()
                    .is_some_and(|n| n.p.distance(p) < 8. / self.art_view.zoom as f64)
                {
                    let n = self.session.active_mut().unwrap().nodes.last_mut().unwrap();
                    n.outgoing = kurbo::Vec2::ZERO;
                    n.exact_outgoing = false;
                    n.smooth = false;
                    self.status =
                        "次の点はクリックで直線、ドラッグで曲線。前の曲線は保持します".into();
                    self.session.commit();
                    self.refresh(cx);
                    return;
                }
                if s.nodes.len() > 2 && s.nodes[0].p.distance(p) < 8. / self.art_view.zoom as f64 {
                    self.session.active_mut().unwrap().closed = true;
                    self.pen_path = None;
                    self.session.commit();
                    self.refresh(cx);
                    return;
                }
            }
            let node = if same {
                self.session.add_point(snapped).unwrap()
            } else {
                let id = self.session.new_path(snapped);
                self.pen_path = Some(id);
                self.session.active().unwrap().nodes[0].id
            };
            let stroke = self.session.active().unwrap().id;
            self.session.select_anchor(Anchor { stroke, node }, false);
            self.capture_frames(Some(Anchor { stroke, node }));
            self.drag = Drag::Pen { node };
            self.refresh(cx);
            return;
        }
        if matches!(self.tool, Tool::Direct | Tool::Convert) && self.direct_down(p, e, cx) {
            return;
        }
        if self.tool == Tool::Width
            && let Some(s) = self.session.active()
        {
            for target in s.width_targets() {
                if let Some((center, d)) = outline::width_location(s, target) {
                    let wid = s.width(target).unwrap();
                    for (v, left) in [
                        (center + d * wid.left, true),
                        (center - d * wid.right, false),
                    ] {
                        if v.distance(p) < 9. / self.art_view.zoom as f64 {
                            self.drag = Drag::Width { target, left };
                            self.session.begin();
                            return;
                        }
                    }
                }
            }
            if e.click_count == 2
                && let Some((i, t, distance)) = outline::nearest(s, p)
                && distance < 20. / self.art_view.zoom as f64
            {
                let segment = s.nodes[i].id;
                let width = s.start_width;
                self.session.begin();
                let id = self.session.doc.id();
                self.session.active_mut().unwrap().widths.push(WidthKey {
                    id,
                    segment,
                    t,
                    width,
                });
                self.session.commit();
                self.refresh(cx);
                return;
            }
        }
        if let Some(id) = self.hit_art(p) {
            self.session.select(id, e.modifiers.shift);
            self.session.begin();
            self.drag = Drag::Move { last: p };
        } else {
            if !e.modifiers.shift {
                self.session.selected.clear();
            }
            self.drag = Drag::Marquee {
                start: p,
                current: p,
                add: e.modifiers.shift,
            };
        }
        cx.notify();
    }
    pub fn mouse_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.cursor = e.position;
        if matches!(self.drag, Drag::None) {
            self.node_region = self.nodes_open && e.position.y >= self.art_bounds.get().bottom();
            return;
        }
        let raw = self.art_view.world(e.position, self.art_bounds.get());
        let p = if matches!(self.drag, Drag::Anchor { .. } | Drag::Pen { .. }) {
            self.snap_point(raw, e.modifiers.control)
        } else {
            raw
        };
        let mut changed = false;
        match self.drag.clone() {
            Drag::None => {}
            Drag::Pan { nodes, last } => {
                let v = if nodes {
                    &mut self.node_view
                } else {
                    &mut self.art_view
                };
                v.pan[0] += f32::from(e.position.x - last.x);
                v.pan[1] += f32::from(e.position.y - last.y);
                self.drag = Drag::Pan {
                    nodes,
                    last: e.position,
                };
            }
            Drag::Resize => {
                self.node_height = (f32::from(self.art_bounds.get().bottom()) + self.node_height
                    - f32::from(e.position.y))
                .clamp(170., 600.);
            }
            Drag::Move { last } => {
                self.session.transform(Affine::translate(p - last));
                self.drag = Drag::Move { last: p };
                changed = true;
            }
            Drag::Anchor { node, handle } => {
                if handle == 0 {
                    if let Some(old) = self
                        .session
                        .active()
                        .and_then(|s| s.nodes.iter().find(|n| n.id == node))
                        .map(|n| n.p)
                    {
                        self.session.move_anchors(p - old);
                    }
                } else if let Some(n) = self
                    .session
                    .active_mut()
                    .and_then(|s| s.nodes.iter_mut().find(|n| n.id == node))
                {
                    {
                        let v = p - n.p;
                        if handle < 0 {
                            n.incoming = v;
                            n.exact_incoming = false;
                            if e.modifiers.alt {
                                n.smooth = false;
                            }
                            if n.smooth {
                                n.outgoing = -v;
                                n.exact_outgoing = false;
                            }
                        } else {
                            n.outgoing = v;
                            n.exact_outgoing = false;
                            if e.modifiers.alt {
                                n.smooth = false;
                            }
                            if n.smooth {
                                n.incoming = -v;
                                n.exact_incoming = false;
                            }
                        }
                    }
                }
                changed = true;
            }
            Drag::Pen { node } => {
                if let Some(n) = self
                    .session
                    .active_mut()
                    .and_then(|s| s.nodes.iter_mut().find(|n| n.id == node))
                {
                    let v = p - n.p;
                    n.outgoing = v;
                    n.incoming = -v;
                    n.exact_outgoing = false;
                    n.exact_incoming = false;
                    n.smooth = v.hypot() > 1.;
                }
                changed = true;
            }
            Drag::Width { target, left } => {
                if let Some(s) = self.session.active_mut()
                    && let Some((center, d)) = outline::width_location(s, target)
                {
                    let value =
                        ((p - center).dot(d) * if left { 1. } else { -1. }).clamp(0., 1000.);
                    let width = s.width_mut(target).unwrap();
                    if left {
                        width.left = value;
                    } else {
                        width.right = value;
                    }
                    if width.linked && !e.modifiers.alt {
                        width.left = value;
                        width.right = value;
                    }
                }
                changed = true;
            }
            Drag::Marquee { start, add, .. } => {
                self.drag = Drag::Marquee {
                    start,
                    current: p,
                    add,
                }
            }
            _ => {
                self.node_drag(e);
                changed = true;
            }
        }
        if changed {
            self.refresh(cx);
        } else {
            cx.notify();
        }
    }
    pub fn mouse_up(&mut self, e: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.drag.clone() {
            Drag::Marquee {
                start,
                current,
                add,
            } => {
                if !add {
                    self.session.selected.clear();
                }
                let r = Rect::from_points(start, current);
                if matches!(self.tool, Tool::Direct | Tool::Convert) {
                    if !add {
                        self.session.anchors.clear();
                    }
                    let hits: Vec<_> = self
                        .session
                        .doc
                        .strokes
                        .iter()
                        .filter(|s| s.visible && s.contours.is_none())
                        .flat_map(|s| {
                            s.nodes.iter().filter(|n| r.contains(n.p)).map(|n| Anchor {
                                stroke: s.id,
                                node: n.id,
                            })
                        })
                        .collect();
                    for at in hits {
                        if !self.session.anchors.contains(&at) {
                            self.session.select_anchor(at, true);
                        }
                    }
                } else {
                    for (id, o) in &self.outlines {
                        let touches = inside(r.center(), &o.contours)
                            || o.contours.iter().any(|c| {
                                c.iter().any(|p| r.contains(*p))
                                    || c.iter()
                                        .zip(c.iter().cycle().skip(1))
                                        .take(c.len())
                                        .any(|(a, b)| segment_hits_rect(*a, *b, r))
                            });
                        if touches && !self.session.selected.contains(id) {
                            self.session.selected.push(*id);
                        }
                    }
                }
            }
            Drag::Connect { .. } | Drag::NodeBox { .. } | Drag::Cut { .. } => self.node_up(e),
            _ => {}
        }
        self.session.commit();
        self.drag = Drag::None;
        self.snap_guide = None;
        self.snap_frames.clear();
        self.refresh(cx);
    }
    pub fn scroll(&mut self, e: &ScrollWheelEvent, nodes: bool, cx: &mut Context<Self>) {
        let b = if nodes {
            self.node_bounds.get()
        } else {
            self.art_bounds.get()
        };
        let view = if nodes {
            &mut self.node_view
        } else {
            &mut self.art_view
        };
        let delta = e.delta.pixel_delta(px(20.));
        if e.modifiers.platform || e.modifiers.control || !e.delta.precise() {
            view.zoom_at(e.position, b, (f32::from(delta.y) * 0.008).exp());
        } else {
            view.pan[0] += f32::from(delta.x);
            view.pan[1] += f32::from(delta.y);
        }
        cx.notify();
    }
    fn key(&mut self, e: &KeyDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let cmd = k.modifiers.platform || k.modifiers.control;
        if self.editing.is_some() {
            if k.key == "enter" {
                if self.input.read(cx).is_composing() {
                    return;
                }
                self.commit_edit(w, cx);
            } else if k.key == "escape" {
                self.editing = None;
                self.focus.focus(w);
                cx.notify();
            }
            return;
        }
        if k.key == "escape" {
            self.session.cancel();
            self.drag = Drag::None;
            self.snap_guide = None;
            self.pen_path = None;
            self.add_open = false;
            self.merge_open = false;
            self.refresh(cx);
            return;
        }
        if cmd {
            match k.key.as_str() {
                "s" => self.save(k.modifiers.shift, cx),
                "o" => self.open(w, cx),
                "e" => self.export(cx),
                "z" => {
                    if k.modifiers.shift {
                        self.session.redo();
                    } else {
                        self.session.undo();
                    }
                    self.refresh(cx);
                }
                "a" => {
                    if !self.node_region && matches!(self.tool, Tool::Direct | Tool::Convert) {
                        self.session.anchors.clear();
                        let hits: Vec<_> = self
                            .session
                            .doc
                            .strokes
                            .iter()
                            .filter(|s| s.visible && s.contours.is_none())
                            .flat_map(|s| {
                                s.nodes.iter().map(|n| Anchor {
                                    stroke: s.id,
                                    node: n.id,
                                })
                            })
                            .collect();
                        for at in hits {
                            self.session.select_anchor(at, true);
                        }
                    } else if self.node_region {
                        self.node_selection = self
                            .session
                            .graph()
                            .map(|g| g.nodes.iter().map(|n| n.id).collect())
                            .unwrap_or_default();
                    } else {
                        self.session.selected = self
                            .session
                            .doc
                            .strokes
                            .iter()
                            .filter(|s| s.visible)
                            .map(|s| s.id)
                            .collect();
                    }
                }
                "d" if !self.node_region => {
                    self.session.duplicate();
                    self.refresh(cx);
                }
                "q" | "w" => {
                    self.confirm_discard(w, cx, |_, w, _| w.remove_window());
                }
                _ => {}
            }
            cx.notify();
            return;
        }
        if self.node_region {
            self.node_key(e, w, cx);
            return;
        }
        match k.key.as_str() {
            "v" => {
                self.tool = Tool::Select;
                self.pen_path = None;
            }
            "a" => self.tool = Tool::Direct,
            "c" if k.modifiers.shift => self.tool = Tool::Convert,
            "p" => {
                self.tool = Tool::Pen;
                self.pen_path = None;
            }
            "w" => self.tool = Tool::Width,
            "m" if matches!(self.tool, Tool::Direct | Tool::Convert) => {
                self.merge_open = !self.merge_open;
            }
            "enter" => {
                self.pen_path = None;
                self.session.commit();
            }
            "backspace" | "delete" | "x" => {
                if matches!(self.tool, Tool::Direct | Tool::Convert) {
                    self.path_operation(crate::path_edit::Operation::Delete, cx);
                } else {
                    self.session.remove();
                    self.refresh(cx);
                }
            }
            "home" | "f" => self.fit_art(),
            "0" => self.art_view.zoom_at(
                self.art_bounds.get().center(),
                self.art_bounds.get(),
                1. / self.art_view.zoom,
            ),
            "n" => self.nodes_open = !self.nodes_open,
            _ => {}
        }
        cx.notify();
    }
    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(40.))
            .px_2()
            .bg(rgb(0x303030))
            .border_b_1()
            .border_color(rgb(0x171717))
            .child(
                div()
                    .w(px(145.))
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("PathWeaver"),
            )
            .child(self.button("開く", false, cx, |s, w, cx| s.open(w, cx)))
            .child(self.button("保存", false, cx, |s, _, cx| s.save(false, cx)))
            .child(self.button("SVG書き出し", false, cx, |s, _, cx| s.export(cx)))
            .child(div().w(px(16.)))
            .child(self.button("元に戻す", false, cx, |s, _, cx| {
                s.session.undo();
                s.refresh(cx);
            }))
            .child(self.button("やり直す", false, cx, |s, _, cx| {
                s.session.redo();
                s.refresh(cx);
            }))
            .child(div().flex_1())
            .child(self.button("全体表示", false, cx, |s, _, _| s.fit_art()))
            .child(self.button("ノード", self.nodes_open, cx, |s, _, _| {
                s.nodes_open = !s.nodes_open
            }))
            .into_any_element()
    }
    fn paths_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div()
            .id("paths-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2();
        for path in &self.session.doc.strokes {
            let id = path.id;
            list = list.child(
                div()
                    .flex()
                    .gap_1()
                    .mb_1()
                    .child(self.button(
                        if path.visible {
                            format!("◉ {id}")
                        } else {
                            format!("○ {id}")
                        },
                        false,
                        cx,
                        move |s, _, cx| {
                            s.session.edit(|s| {
                                if let Some(p) = s.doc.strokes.iter_mut().find(|p| p.id == id) {
                                    p.visible = !p.visible;
                                }
                            });
                            s.refresh(cx);
                        },
                    ))
                    .child(div().flex_1().min_w_0().child(self.button(
                        path.name.clone(),
                        self.session.selected.contains(&id),
                        cx,
                        move |s, w, cx| {
                            s.session.select(id, w.modifiers().shift);
                            s.node_region = false;
                            s.pen_path = None;
                            s.focus.focus(w);
                            cx.notify();
                        },
                    ))),
            );
        }
        div()
            .w(px(205.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(0x2b2b2b))
            .border_r_1()
            .border_color(rgb(0x171717))
            .child(div().p_3().text_color(rgb(0xaaaaaa)).child("パス"))
            .child(
                div().flex().flex_wrap().gap_1().px_2().children(
                    [
                        (Tool::Select, "選択 V"),
                        (Tool::Direct, "制御点 A"),
                        (Tool::Convert, "アンカー ⇧C"),
                        (Tool::Pen, "ペン P"),
                        (Tool::Width, "幅 W"),
                    ]
                    .into_iter()
                    .map(|(tool, label)| {
                        self.button(label, self.tool == tool, cx, move |s, w, _| {
                            s.tool = tool;
                            s.pen_path = None;
                            s.node_region = false;
                            s.focus.focus(w);
                        })
                    }),
                ),
            )
            .child(list)
            .child(div().p_2().child(self.snap_properties(cx)))
            .child(
                div()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.button("新規パス", false, cx, |s, w, _| {
                        s.tool = Tool::Pen;
                        s.pen_path = None;
                        s.focus.focus(w);
                    }))
                    .child(self.button("サンプルを開く", false, cx, |s, w, cx| {
                        s.confirm_discard(w, cx, |s, _, cx| {
                            s.session = Session::default();
                            s.session.doc = Document::example();
                            s.session.selected = vec![s.session.doc.strokes[0].id];
                            s.fit_art();
                            s.refresh(cx);
                        });
                    })),
            )
            .into_any_element()
    }
    fn properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .id(if self.merge_open {
                "merge-properties"
            } else {
                "properties"
            })
            .w(px(225.))
            .flex_shrink_0()
            .h_full()
            .overflow_y_scroll()
            .bg(rgb(0x2b2b2b))
            .border_l_1()
            .border_color(rgb(0x171717))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_color(rgb(0xaaaaaa)).child("パスの設定"));
        col = col.child(self.anchor_properties(cx));
        if let Some(path) = self.session.active() {
            let name = path.name.clone();
            let width = path.start_width.left + path.start_width.right;
            let color = path.edge.unwrap_or([224; 3]);
            col = col
                .child(self.button(name.clone(), false, cx, move |s, w, cx| {
                    s.start_edit(Editing::PathName, name.clone(), w, cx)
                }))
                .child(format!("{} パスを選択", self.session.selected.len()))
                .child(
                    self.button(format!("線幅  {width:.1}"), false, cx, move |s, w, cx| {
                        s.start_edit(Editing::Width, format!("{width:.1}"), w, cx)
                    }),
                )
                .child(self.button(
                    format!("線色  #{:02X}{:02X}{:02X}", color[0], color[1], color[2]),
                    false,
                    cx,
                    move |s, w, cx| {
                        s.node_region = false;
                        s.start_edit(
                            Editing::Color,
                            format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2]),
                            w,
                            cx,
                        );
                    },
                ))
                .child(self.button(
                    if path.fill.is_some() {
                        "塗り：あり"
                    } else {
                        "塗り：なし"
                    },
                    false,
                    cx,
                    |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(p) = s.active_mut() {
                                p.fill = if p.fill.is_some() {
                                    None
                                } else {
                                    Some([192, 192, 192])
                                };
                            }
                        });
                        s.refresh(cx);
                    },
                ))
                .child(self.button(
                    if path.closed {
                        "パス：閉じる"
                    } else {
                        "パス：開く"
                    },
                    false,
                    cx,
                    |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(p) = s.active_mut() {
                                p.closed = !p.closed;
                            }
                        });
                        s.refresh(cx);
                    },
                ))
                .child(self.button(
                    format!("角の結合：{:?}", path.join),
                    false,
                    cx,
                    |s, _, cx| {
                        s.session.edit(|s| {
                            if let Some(p) = s.active_mut() {
                                p.join = match p.join {
                                    Join::Miter => Join::Round,
                                    Join::Round => Join::Bevel,
                                    Join::Bevel => Join::Miter,
                                };
                            }
                        });
                        s.refresh(cx);
                    },
                ))
                .child(self.button("回転…", false, cx, |s, w, cx| {
                    s.start_edit(Editing::Rotation, "15".into(), w, cx)
                }))
                .child(self.button("拡大・縮小…", false, cx, |s, w, cx| {
                    s.start_edit(Editing::Scale, "1.2".into(), w, cx)
                }))
                .child(self.button("複製 ⌘D", false, cx, |s, _, cx| {
                    s.session.duplicate();
                    s.refresh(cx);
                }))
                .child(self.button("削除", false, cx, |s, _, cx| {
                    s.session.remove();
                    s.refresh(cx);
                }))
                .child(div().h(px(8.)))
                .child(
                    self.button("ノードを編集", self.nodes_open, cx, |s, w, cx| {
                        s.nodes_open = true;
                        s.node_region = true;
                        s.focus.focus(w);
                        if s.session.graph().is_none() {
                            s.session.edit(|s| {
                                if let Some(p) = s.active_mut() {
                                    p.appearance = Some(Graph::default());
                                }
                            });
                            s.fit_nodes();
                            s.refresh(cx);
                        }
                    }),
                );
        } else {
            col = col.child("パスを選択すると設定を編集できます。");
        }
        col.child(self.button(
            if self.session.doc.snap_grid {
                "グリッド吸着：オン"
            } else {
                "グリッド吸着：オフ"
            },
            false,
            cx,
            |s, _, _| s.session.edit(|s| s.doc.snap_grid = !s.doc.snap_grid),
        ))
        .into_any_element()
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = self.session.active().map(|s| s.id);
        if self.node_owner != owner {
            self.node_owner = owner;
            self.node_selection.clear();
            self.set_name = self
                .session
                .graph()
                .map(|g| g.name.clone())
                .unwrap_or_default();
            self.category.clear();
        }
        let title = self
            .session
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "名称未設定".into());
        window.set_window_title(&format!(
            "{}{} — PathWeaver",
            if self.session.dirty() { "• " } else { "" },
            title
        ));
        let view = cx.entity();
        let bounds = self.art_bounds.clone();
        let canvas = div()
            .id("art-canvas")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |b, _, _| {
                        bounds.set(b);
                    },
                    move |b, _, w, cx| view.update(cx, |s, cx| s.paint_art(b, w, cx)),
                )
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::art_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::art_down))
            .on_scroll_wheel(cx.listener(|s, e, _, cx| s.scroll(e, false, cx)));
        let main = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(self.paths_panel(cx))
            .child(canvas)
            .child(self.properties(cx));
        let mut root = div()
            .key_context("PathWeaver")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x252525))
            .text_color(rgb(0xdddddd))
            .text_size(px(12.))
            .font_family("Hiragino Sans")
            .on_key_down(cx.listener(Self::key))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .child(self.toolbar(cx));
        if self.editing.is_some() {
            root = root.child(self.edit_strip(cx));
        }
        root = root.child(main);
        if self.nodes_open {
            root = root.child(self.nodes_panel(cx));
        }
        root.child(
            div()
                .h(px(25.))
                .flex_shrink_0()
                .px_3()
                .flex()
                .items_center()
                .justify_between()
                .bg(rgb(0x323232))
                .text_color(rgb(0xb8b8b8))
                .child(self.status.clone())
                .child(format!(
                    "{} パス    {:.0}%",
                    self.session.doc.strokes.len(),
                    self.art_view.zoom * 100.
                )),
        )
    }
}
pub fn segment_hits_rect(a: KPoint, b: KPoint, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let mut low = 0_f64;
    let mut high = 1_f64;
    for (p, q) in [
        (-(b.x - a.x), a.x - r.x0),
        (b.x - a.x, r.x1 - a.x),
        (-(b.y - a.y), a.y - r.y0),
        (b.y - a.y, r.y1 - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0. {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0. {
                low = low.max(t);
            } else {
                high = high.min(t);
            }
            if low > high {
                return false;
            }
        }
    }
    true
}

fn same_art(a: &pathweaver_core::model::Stroke, b: &pathweaver_core::model::Stroke) -> bool {
    let graphs = match (&a.appearance, &b.appearance) {
        (Some(a), Some(b)) => a.same_rendering(b),
        (None, None) => true,
        _ => false,
    };
    if !graphs {
        return false;
    }
    let mut a = a.clone();
    let mut b = b.clone();
    a.appearance = None;
    b.appearance = None;
    a.name.clear();
    b.name.clear();
    a == b
}
#[cfg(test)]
mod tests {
    use super::{Document, Graph, KPoint, Rect, same_art, segment_hits_rect};
    #[test]
    fn changing_node_layout_does_not_invalidate_geometry() {
        let mut a = Document::example().strokes.remove(0);
        a.appearance = Some(Graph::rough());
        let mut b = a.clone();
        let n = b.appearance.as_mut().unwrap().node_mut(3).unwrap();
        n.position = [999., 888.];
        n.label = "紙".into();
        assert!(same_art(&a, &b));
        b.appearance.as_mut().unwrap().node_mut(3).unwrap().amount = 20.;
        assert!(!same_art(&a, &b));
    }
    #[test]
    fn marquee_segment_intersection_handles_crossing_and_outside_edges() {
        let r = Rect::new(0., 0., 10., 10.);
        assert!(segment_hits_rect(
            KPoint::new(-2., 5.),
            KPoint::new(12., 5.),
            r
        ));
        assert!(!segment_hits_rect(
            KPoint::new(-2., 12.),
            KPoint::new(12., 12.),
            r
        ));
    }
}
