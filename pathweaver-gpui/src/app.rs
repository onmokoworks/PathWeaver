use crate::theme::{Role, color as ui_color};
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
    model::{Document, Join, WidthTarget},
    outline, snapping,
};
use std::{cell::Cell, rc::Rc};

pub(crate) struct IconTooltip(pub SharedString);
impl Render for IconTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .shadow_md()
            .bg(rgb(ui_color(Role::Surface)))
            .border_1()
            .border_color(rgb(ui_color(Role::Border)))
            .text_color(rgb(ui_color(Role::Text)))
            .text_size(px(11.))
            .font_family(crate::assets::UI_FONT)
            .child(self.0.clone())
    }
}

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
    Numeric {
        key: Editing,
        start: Point<Pixels>,
        last: Point<Pixels>,
        value: f64,
        initial: f64,
        step: f64,
        min: f64,
        max: f64,
        moved: bool,
    },
    None,
    Pan {
        nodes: bool,
        last: Point<Pixels>,
    },
    Guide {
        index: usize,
    },
    Move {
        start: KPoint,
        origin: KPoint,
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
    ConnectInput {
        to: u64,
        socket: crate::nodes::Socket,
        at: Point<Pixels>,
    },
    Cut {
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
}
#[derive(Clone, PartialEq)]
pub enum Editing {
    GridSpacing,
    AnchorX,
    AnchorY,
    PathName,
    NodeName,
    Parameter(Parameter),
    Color,
    SetName,
    Category,
    Width,
    Rotation,
    Scale,
}
/// The document objects an edit began on. Pointer position and later selection
/// changes must never redirect an already-open field to another object.
#[derive(Clone)]
pub struct EditTarget {
    path: Option<u64>,
    node: Option<u64>,
    selected: Vec<u64>,
    anchors: Vec<Anchor>,
}
impl EditTarget {
    fn capture(session: &Session, node: Option<u64>) -> Self {
        Self {
            path: session.active().map(|s| s.id),
            node,
            selected: session.selected.clone(),
            anchors: session.anchors.clone(),
        }
    }
    fn path_mut<'a>(
        &self,
        doc: &'a mut Document,
    ) -> Result<&'a mut pathweaver_core::model::Stroke, String> {
        doc.strokes
            .iter_mut()
            .find(|s| Some(s.id) == self.path)
            .ok_or_else(|| "編集を開始したパスがありません。取消して選び直してください".into())
    }
    fn node_mut<'a>(
        &self,
        doc: &'a mut Document,
    ) -> Result<&'a mut pathweaver_core::appearance::Node, String> {
        let id = self.node.ok_or("編集を開始したノードがありません")?;
        self.path_mut(doc)?
            .appearance
            .as_mut()
            .and_then(|g| g.node_mut(id))
            .ok_or_else(|| "編集を開始したノードがありません。取消して選び直してください".into())
    }
    fn apply_color(&self, doc: &mut Document, color: [u8; 3]) -> Result<(), String> {
        if self.node.is_some() {
            self.node_mut(doc)?.color = color;
        } else {
            self.path_mut(doc)?.edge = Some(color);
        }
        Ok(())
    }
    fn matches_numeric(&self, session: &Session, nodes: &[u64], kind: &Editing) -> bool {
        match kind {
            Editing::GridSpacing => true,
            Editing::Parameter(_) => {
                self.path == session.active().map(|s| s.id) && self.node == nodes.last().copied()
            }
            Editing::AnchorX | Editing::AnchorY => {
                self.selected == session.selected && self.anchors == session.anchors
            }
            _ => self.selected == session.selected,
        }
    }
}
#[derive(Clone, Copy)]
enum ButtonEditPolicy {
    Commit,
    Explicit,
}
pub struct Workspace {
    pub session: Session,
    pub dialog_open: bool,
    pub outlines: Vec<(u64, outline::Outline)>,
    rendered_strokes: Vec<pathweaver_core::model::Stroke>,
    node_owner: Option<u64>,
    pub tool: Tool,
    pub drag: Drag,
    pub node_move_modal: bool,
    pub art_view: View,
    pub node_view: View,
    pub art_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub node_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub focus: FocusHandle,
    pub nodes_open: bool,
    pub node_height: f32,
    pub node_selection: Vec<u64>,
    pub node_region: bool,
    pub active_nodes: bool,
    pub inspector: bool,
    pub add_open: bool,
    pub add_input: Entity<TextInput>,
    pub add_anchor: Point<Pixels>,
    pub add_index: usize,
    pub node_placement_selection: Option<Vec<u64>>,
    pub library_open: bool,
    pub search: String,
    pub set_name: String,
    pub category: String,
    pub input: Entity<TextInput>,
    pub editing: Option<Editing>,
    pub edit_target: Option<EditTarget>,
    pub status: String,
    pub pen_path: Option<u64>,
    pub cursor: Point<Pixels>,
    pub merge_open: bool,
    pub numeric_before: Option<Document>,
    pub numeric_error: bool,
    pub numeric_snap: bool,
    pub edit_initial: f64,
    pub inspector_tab: usize,
    pub snap_frames: Vec<snapping::Frame>,
    pub snap_guide: Option<snapping::Guide>,
}
impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        cx.observe_window_activation(window, |s, w, cx| {
            if !w.is_window_active() && !matches!(s.drag, Drag::None) {
                s.finish_interaction(false, cx);
                s.refresh(cx);
            }
        })
        .detach();
        let add_input = cx.new(TextInput::new);
        cx.observe(&add_input, |s, input, cx| {
            let query = input.read(cx).content.to_string();
            if s.search != query {
                s.search = query;
                s.add_index = 0;
                cx.notify();
            }
        })
        .detach();
        Self {
            session: Session::default(),
            dialog_open: false,
            outlines: vec![],
            rendered_strokes: vec![],
            node_owner: None,
            tool: Tool::Pen,
            drag: Drag::None,
            node_move_modal: false,
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
            active_nodes: false,
            inspector: true,
            add_open: false,
            add_input,
            add_anchor: point(px(0.), px(0.)),
            add_index: 0,
            node_placement_selection: None,
            library_open: false,
            search: String::new(),
            set_name: String::new(),
            category: String::new(),
            input: cx.new(TextInput::new),
            editing: None,
            edit_target: None,
            status: "P：ペン   V：範囲選択   A：制御点   W：幅   ⌘S：保存".into(),
            pen_path: None,
            cursor: point(px(0.), px(0.)),
            merge_open: false,
            numeric_before: None,
            numeric_error: false,
            numeric_snap: false,
            edit_initial: 0.,
            inspector_tab: 0,
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
        if self.editing.is_some() {
            self.commit_edit(window, cx);
            if self.editing.is_some() {
                return;
            }
        }
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
        self.finish_interaction(true, cx);
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
        self.finish_interaction(true, cx);
        self.confirm_discard(window, cx, |s, _, cx| s.pick_document(cx));
    }
    fn reset_document_interaction(&mut self) {
        // Called only after a successful document replacement.
        self.node_region = false;
        self.active_nodes = false;
        self.drag = Drag::None;
        self.node_move_modal = false;
        self.pen_path = None;
        self.editing = None;
        self.edit_target = None;
        self.numeric_before = None;
        self.numeric_error = false;
        self.numeric_snap = false;
        self.snap_frames.clear();
        self.snap_guide = None;
        self.node_selection.clear();
        self.node_owner = None;
        self.node_view = View {
            pan: [30., 30.],
            zoom: 0.9,
        };
        self.set_name.clear();
        self.category.clear();
        self.search.clear();
        self.add_index = 0;
        self.node_placement_selection = None;
        self.add_open = false;
        self.library_open = false;
        self.merge_open = false;
        self.outlines.clear();
        self.rendered_strokes.clear();
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
                                    s.reset_document_interaction();
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
    #[track_caller]
    pub fn button(
        &self,
        label: impl Into<SharedString>,
        active: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let label = label.into();
        let caller = std::panic::Location::caller();
        // Scope stock controls to their call site. Object-name buttons supply
        // their own persistent ID via button_with_id instead.
        let id: SharedString = format!(
            "{}:{}:{}:{label}",
            caller.file(),
            caller.line(),
            caller.column()
        )
        .into();
        self.button_with_id(id, label, active, cx, f)
    }
    pub fn button_with_id(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        active: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        self.edit_button(
            id.into(),
            label.into(),
            active,
            ButtonEditPolicy::Commit,
            cx,
            f,
        )
    }
    pub fn icon_button(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
        label: impl Into<SharedString>,
        active: bool,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let label = label.into();
        div()
            .id(id.into())
            .size(px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .cursor_pointer()
            .bg(rgb(ui_color(if active {
                Role::Selected
            } else {
                Role::Toolbar
            })))
            .hover(move |s| {
                s.bg(rgb(ui_color(if active {
                    Role::Selected
                } else {
                    Role::Hover
                })))
            })
            .child(
                svg()
                    .path(format!("icons/{icon}.svg"))
                    .size(px(18.))
                    .text_color(rgb(ui_color(if active {
                        Role::OnAccent
                    } else {
                        Role::Text
                    }))),
            )
            .tooltip(move |_, cx| cx.new(|_| IconTooltip(label.clone())).into())
            .on_click(cx.listener(move |s, _, w, cx| {
                if s.editing.is_some() {
                    s.commit_edit(w, cx);
                    if s.editing.is_some() {
                        return;
                    }
                }
                s.finish_interaction(true, cx);
                f(s, w, cx);
                cx.notify();
            }))
            .into_any_element()
    }
    fn edit_button(
        &self,
        id: ElementId,
        label: SharedString,
        active: bool,
        policy: ButtonEditPolicy,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(id)
            .px_2()
            .py_1()
            .rounded(px(4.))
            .text_size(px(12.))
            .cursor_pointer()
            .bg(rgb(if active {
                ui_color(Role::Selected)
            } else {
                ui_color(Role::Button)
            }))
            .hover(move |s| {
                s.bg(rgb(ui_color(if active {
                    Role::Selected
                } else {
                    Role::Hover
                })))
            })
            .text_color(rgb(ui_color(if active {
                Role::OnAccent
            } else {
                Role::Text
            })))
            .child(label)
            .on_click(cx.listener(move |s, _, w, cx| {
                if s.editing.is_some() && matches!(policy, ButtonEditPolicy::Commit) {
                    s.commit_edit(w, cx);
                    if s.editing.is_some() {
                        return;
                    }
                }
                s.finish_interaction(true, cx);
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
        let node = if matches!(kind, Editing::NodeName | Editing::Parameter(_)) {
            self.node_selection.last().copied()
        } else {
            None
        };
        self.start_target_edit(kind, node, value, window, cx);
    }
    pub fn start_node_edit(
        &mut self,
        kind: Editing,
        node: u64,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_target_edit(kind, Some(node), value, window, cx);
    }
    fn start_target_edit(
        &mut self,
        kind: Editing,
        node: Option<u64>,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_initial = self.numeric_meta(&kind).map(|v| v.0).unwrap_or(0.);
        self.numeric_error = false;
        self.edit_target = Some(EditTarget::capture(&self.session, node));
        self.editing = Some(kind);
        self.input.update(cx, |i, cx| i.set(value, window, cx));
        // A canvas double-click starts this field during mouse-down dispatch.
        // Its ancestor can still apply tracked focus later in that same event.
        // Restore the field's focus only after the current dispatch has finished.
        cx.defer_in(window, |s, window, cx| {
            if s.editing.is_some() {
                s.input.read(cx).focus_handle(cx).focus(window);
            }
        });
        cx.notify();
    }
    pub fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.editing.clone() else {
            return;
        };
        let Some(target) = self.edit_target.clone() else {
            self.status = "編集対象がありません。取消して選び直してください".into();
            self.numeric_error = true;
            cx.notify();
            return;
        };
        let text = self.input.read(cx).content.to_string();
        let before = self.session.doc.clone();
        self.session.begin();
        let result = (|| -> Result<(), String> {
            match kind {
                Editing::GridSpacing
                | Editing::AnchorX
                | Editing::AnchorY
                | Editing::Width
                | Editing::Parameter(_)
                | Editing::Rotation
                | Editing::Scale => {
                    if !target.matches_numeric(&self.session, &self.node_selection, &kind) {
                        return Err(
                            "入力中に編集対象が変わりました。取消して選び直してください".into()
                        );
                    }
                    let value = crate::controls::expression(&text, self.edit_initial)?;
                    self.apply_numeric(&kind, value)?;
                }
                Editing::SetName => {
                    if text.chars().count() > 80 {
                        return Err("名前は80文字以内です".into());
                    }
                    target
                        .path_mut(&mut self.session.doc)?
                        .appearance
                        .as_mut()
                        .ok_or("編集を開始したノードグラフがありません")?
                        .name = text.clone();
                    self.set_name = text;
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
                    target.path_mut(&mut self.session.doc)?.name = text;
                }
                Editing::NodeName => {
                    if text.chars().count() > 80 {
                        return Err("名前は80文字以内です".into());
                    }
                    target.node_mut(&mut self.session.doc)?.label = text;
                }
                Editing::Color => {
                    let hex = text.trim().trim_start_matches('#');
                    if hex.len() != 6 {
                        return Err("色は #RRGGBB で入力してください".into());
                    }
                    let v = u32::from_str_radix(hex, 16)
                        .map_err(|_| "色は #RRGGBB で入力してください")?;
                    let rgb = [(v >> 16) as u8, (v >> 8) as u8, v as u8];
                    target.apply_color(&mut self.session.doc, rgb)?;
                }
            }
            self.session.doc.validate()?;
            Ok(())
        })();
        if let Err(e) = result {
            self.session.doc = before;
            self.session.commit();
            self.status = e;
            self.numeric_error = true;
            cx.notify();
            return;
        }
        self.session.commit();
        self.numeric_error = false;
        self.editing = None;
        self.edit_target = None;
        self.focus.focus(window);
        self.refresh(cx);
    }
    pub fn edit_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .gap_1()
            .items_center()
            .p_2()
            .bg(rgb(ui_color(Role::Soft)))
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .child(self.edit_button(
                "edit-apply".into(),
                "適用".into(),
                true,
                ButtonEditPolicy::Explicit,
                cx,
                |s, w, cx| s.commit_edit(w, cx),
            ))
            .child(self.edit_button(
                "edit-cancel".into(),
                "取消".into(),
                false,
                ButtonEditPolicy::Explicit,
                cx,
                |s, w, _| {
                    s.editing = None;
                    s.edit_target = None;
                    s.numeric_error = false;
                    s.focus.focus(w);
                },
            ))
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
            self.commit_edit(w, cx);
            if self.editing.is_some() {
                return;
            }
        }
        if self.handle_drag_down(e, cx) {
            return;
        }
        self.focus.focus(w);
        self.node_region = false;
        self.active_nodes = false;
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
        if self.guide_down(p, e, cx) {
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
                self.session.begin();
                let id = self.session.doc.id();
                outline::insert_width_key(self.session.active_mut().unwrap(), id, i, t);
                self.session.commit();
                self.refresh(cx);
                return;
            }
        }
        if let Some(id) = self.hit_art(p) {
            self.session.select(id, e.modifiers.shift);
            if !self.session.selected.contains(&id) {
                cx.notify();
                return;
            }
            self.session.begin();
            self.start_path_move(p);
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
        if self.numeric_move(e, cx) {
            return;
        }
        if matches!(self.drag, Drag::None) {
            // Menus and inspectors preserve the last canvas context.
            let before = self.node_region;
            if self.nodes_open && self.node_bounds.get().contains(&e.position) {
                self.node_region = true;
            } else if self.art_bounds.get().contains(&e.position) {
                self.node_region = false;
            }
            if before != self.node_region {
                cx.notify();
            }
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
            Drag::Guide { index } => {
                if let Some(guide) = self.session.doc.guides.get_mut(index) {
                    guide.position = (if guide.vertical { p.x } else { p.y }).clamp(-1e6, 1e6);
                    self.status = format!(
                        "{}ガイド  {:.2}  ·  定規の外へ戻すと削除",
                        if guide.vertical { "縦" } else { "横" },
                        guide.position
                    );
                }
            }
            Drag::Move {
                start,
                origin,
                last,
            } => {
                let (snapped, guide) = snapping::translation(
                    &self.session.doc,
                    p,
                    start,
                    origin,
                    &self.snap_frames,
                    self.art_view.zoom as f64,
                    e.modifiers.control,
                );
                self.snap_guide = guide;
                self.session.transform(Affine::translate(snapped - last));
                self.drag = Drag::Move {
                    start,
                    origin,
                    last: snapped,
                };
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
    pub fn mouse_up(&mut self, e: &MouseUpEvent, w: &mut Window, cx: &mut Context<Self>) {
        if self.node_move_modal || self.drag.release_button() != Some(e.button) {
            return;
        }
        if let Drag::Numeric {
            key,
            initial,
            moved,
            ..
        } = self.drag.clone()
        {
            self.session.commit();
            self.drag = Drag::None;
            self.numeric_before = None;
            self.numeric_snap = false;
            if !moved {
                self.start_edit(key, initial.to_string(), w, cx);
            } else {
                self.status = "値を変更しました（⌘Zで元に戻す）".into();
                self.refresh(cx);
            }
            return;
        }
        match self.drag.clone() {
            Drag::Guide { index } => {
                let b = self.art_bounds.get();
                if (!b.contains(&e.position)
                    || e.position.x < b.left() + px(20.)
                    || e.position.y < b.top() + px(20.))
                    && index < self.session.doc.guides.len()
                {
                    self.session.doc.guides.remove(index);
                    self.status = "ガイドを削除しました（⌘Zで元に戻す）".into();
                } else {
                    self.status = "ガイドを配置しました。Vでドラッグして移動できます".into();
                }
            }
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
            Drag::Connect { .. }
            | Drag::ConnectInput { .. }
            | Drag::NodeBox { .. }
            | Drag::Cut { .. } => {
                self.node_up(e);
                // node_up has resolved or cancelled the proposal. Commit the result,
                // rather than treating it as an abandoned wire in finish_interaction.
                self.drag = Drag::None;
            }
            _ => {}
        }
        self.finish_interaction(true, cx);
        self.refresh(cx);
    }
    pub fn scroll(&mut self, e: &ScrollWheelEvent, nodes: bool, cx: &mut Context<Self>) {
        // A held-button gesture owns its coordinate system until release.
        // Keyboard node movement can pan/zoom, rebasing its cursor below.
        if !matches!(self.drag, Drag::None) && !self.node_move_modal {
            return;
        }
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
        let before = view.world(self.cursor, b);
        let delta = e.delta.pixel_delta(px(20.));
        if e.modifiers.platform || e.modifiers.control || !e.delta.precise() {
            view.zoom_at(e.position, b, (f32::from(delta.y) * 0.008).exp());
        } else {
            view.pan[0] += f32::from(delta.x);
            view.pan[1] += f32::from(delta.y);
        }
        if nodes && self.node_move_modal {
            let after = view.world(self.cursor, b);
            self.drag.rebase_nodes(after - before);
        }
        cx.notify();
    }
    fn key(&mut self, e: &KeyDownEvent, w: &mut Window, cx: &mut Context<Self>) {
        if self.node_add_key(e, w, cx) {
            return;
        }
        let k = &e.keystroke;
        let cmd = k.modifiers.platform || k.modifiers.control;
        if matches!(self.drag, Drag::Numeric { .. }) && k.key != "escape" {
            if k.key == "q" {
                self.numeric_snap = true;
            }
            return;
        }
        if self.editing.is_some() {
            if matches!(k.key.as_str(), "up" | "down")
                && let Some(kind) = self.editing.clone()
                && let Some((_, min, max, step)) = self.numeric_meta(&kind)
            {
                let current = crate::controls::expression(
                    self.input.read(cx).content.as_ref(),
                    self.edit_initial,
                )
                .unwrap_or(self.edit_initial);
                let value = crate::controls::adjusted(
                    current,
                    if k.key == "up" { 1. } else { -1. },
                    step,
                    min,
                    max,
                    (k.modifiers.alt, k.modifiers.shift, false),
                );
                self.input
                    .update(cx, |i, cx| i.set(value.to_string(), w, cx));
                return;
            }
            if k.key == "enter" {
                if self.input.read(cx).is_composing() {
                    return;
                }
                self.commit_edit(w, cx);
            } else if k.key == "escape" {
                self.editing = None;
                self.edit_target = None;
                self.focus.focus(w);
                cx.notify();
            }
            return;
        }
        if k.key == "escape" {
            self.finish_interaction(false, cx);
            self.pen_path = None;
            self.add_open = false;
            self.library_open = false;
            self.merge_open = false;
            self.refresh(cx);
            return;
        }
        if self.node_move_modal && k.key == "enter" {
            self.finish_interaction(true, cx);
            self.refresh(cx);
            return;
        }
        // Native menu actions and shortcuts share one command path. TextInput
        // handles local editing actions before workspace actions bubble here.
        if cmd {
            return;
        }
        if !matches!(self.drag, Drag::None) {
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
    pub(crate) fn menu_command(
        &mut self,
        command: crate::menus::Command,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::menus::Command;
        use crate::path_edit::Operation;
        if self.dialog_open {
            return;
        }
        if self.editing.is_some() {
            if self.input.read(cx).is_composing() {
                return;
            }
            self.commit_edit(w, cx);
            if self.editing.is_some() {
                return;
            }
        }
        self.finish_interaction(true, cx);
        self.add_open = false;
        self.focus.focus(w);
        match command {
            Command::New => self.confirm_discard(w, cx, |s, _, cx| {
                s.session = Session::default();
                s.reset_document_interaction();
                s.art_view = View::default();
                s.refresh(cx);
            }),
            Command::Open => self.open(w, cx),
            Command::Save => self.save(false, cx),
            Command::SaveAs => self.save(true, cx),
            Command::Export => self.export(cx),
            Command::Close => self.confirm_discard(w, cx, |_, w, _| w.remove_window()),
            Command::Undo => self.history_step(false, cx),
            Command::Redo => self.history_step(true, cx),
            Command::SelectAll => {
                if self.active_nodes && self.nodes_open {
                    self.node_selection = self
                        .session
                        .graph()
                        .map(|g| g.nodes.iter().map(|n| n.id).collect())
                        .unwrap_or_default();
                } else if matches!(self.tool, Tool::Direct | Tool::Convert) {
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
            Command::Duplicate => {
                if self.active_nodes && self.nodes_open {
                    self.duplicate_nodes(cx);
                } else {
                    self.session.duplicate();
                }
            }
            Command::Delete => {
                if self.active_nodes && self.nodes_open {
                    self.delete_nodes();
                } else if matches!(self.tool, Tool::Direct | Tool::Convert) {
                    self.path_operation(Operation::Delete, cx);
                } else {
                    self.session.remove();
                }
            }
            Command::FitArt => self.fit_art(),
            Command::ActualSize => self.art_view.zoom_at(
                self.art_bounds.get().center(),
                self.art_bounds.get(),
                1. / self.art_view.zoom,
            ),
            Command::ToggleNodes => self.nodes_open = !self.nodes_open,
            Command::ToggleTheme => {
                self.status = crate::theme::toggle().err().unwrap_or_default();
                self.input.update(cx, |_, cx| cx.notify());
            }
            Command::SelectTool
            | Command::DirectTool
            | Command::ConvertTool
            | Command::PenTool
            | Command::WidthTool => {
                self.tool = match command {
                    Command::SelectTool => Tool::Select,
                    Command::DirectTool => Tool::Direct,
                    Command::ConvertTool => Tool::Convert,
                    Command::PenTool => Tool::Pen,
                    _ => Tool::Width,
                };
                self.pen_path = None;
                self.node_region = false;
                self.active_nodes = false;
                self.focus.focus(w);
            }
            Command::Separate => self.path_operation(Operation::Separate, cx),
            Command::Join => self.path_operation(Operation::Join, cx),
            Command::MergeCenter => self.path_operation(
                Operation::Merge(pathweaver_core::editing::MergeTarget::Center),
                cx,
            ),
            Command::Straighten => self.path_operation(Operation::Straighten, cx),
            Command::Union => self.path_operation(Operation::Union, cx),
            Command::AddNode => {
                self.nodes_open = true;
                self.open_node_add(w, cx);
            }
            Command::FitNodes => {
                self.nodes_open = true;
                self.fit_nodes();
            }
            Command::DuplicateNodes => {
                self.active_nodes = true;
                self.duplicate_nodes(cx);
            }
            Command::DeleteNodes => {
                self.active_nodes = true;
                self.delete_nodes();
            }
            Command::ToggleNode => {
                self.active_nodes = true;
                self.toggle_nodes_bypass();
            }
        }
        self.refresh(cx);
    }
    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .h(px(36.))
            .px_2()
            .bg(rgb(ui_color(Role::Toolbar)))
            .border_b_1()
            .border_color(rgb(ui_color(Role::Border)))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("PathWeaver"),
            )
            .child(div().flex_1())
            .child(self.icon_button(
                "theme",
                if crate::theme::is_dark() {
                    "sun"
                } else {
                    "moon"
                },
                if crate::theme::is_dark() {
                    "ライトに切り替え"
                } else {
                    "ダークに切り替え"
                },
                false,
                cx,
                |s, _, cx| {
                    s.status = crate::theme::toggle().err().unwrap_or_default();
                    s.input.update(cx, |_, cx| cx.notify());
                },
            ))
            .child(self.icon_button(
                "fit-art",
                "fit",
                "作品全体を表示  F",
                false,
                cx,
                |s, _, _| s.fit_art(),
            ))
            .child(self.icon_button(
                "toggle-nodes",
                "nodes",
                "ノードエディターを表示／隠す",
                self.nodes_open,
                cx,
                |s, _, _| s.nodes_open = !s.nodes_open,
            ))
            .into_any_element()
    }
    fn tool_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut tools = div()
            .w(px(40.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .py_2()
            .bg(rgb(ui_color(Role::Surface)))
            .border_r_1()
            .border_color(rgb(ui_color(Role::Border)));
        for (tool, icon, label) in [
            (Tool::Select, "select", "選択 V"),
            (Tool::Direct, "direct", "制御点 A"),
            (Tool::Convert, "anchor", "アンカー ⇧C"),
            (Tool::Pen, "pen", "ペン P"),
            (Tool::Width, "width", "幅 W"),
        ] {
            tools = tools.child(self.icon_button(
                SharedString::from(format!("tool-{icon}")),
                icon,
                label,
                self.tool == tool,
                cx,
                move |s, w, _| {
                    s.tool = tool;
                    s.pen_path = None;
                    s.node_region = false;
                    s.active_nodes = false;
                    s.focus.focus(w);
                },
            ));
        }
        tools.into_any_element()
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
                    .child(self.icon_button(
                        ("path-visibility", id),
                        if path.visible { "eye" } else { "eye-off" },
                        if path.visible {
                            "パスを隠す"
                        } else {
                            "パスを表示"
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
                    .child(div().flex_1().min_w_0().child(self.button_with_id(
                        ("path", id),
                        path.name.clone(),
                        self.session.selected.contains(&id),
                        cx,
                        move |s, w, cx| {
                            s.session.select(id, w.modifiers().shift);
                            s.node_region = false;
                            s.active_nodes = false;
                            s.pen_path = None;
                            s.focus.focus(w);
                            cx.notify();
                        },
                    ))),
            );
        }
        div()
            .h(relative(0.26))
            .max_h(px(132.))
            .min_h(px(70.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(ui_color(Role::Surface)))
            .border_b_1()
            .border_color(rgb(ui_color(Role::Border)))
            .child(
                div()
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child("パス")
                    .child(self.icon_button(
                        "new-path",
                        "plus",
                        "新しいパスを描く  P",
                        false,
                        cx,
                        |s, w, _| {
                            s.tool = Tool::Pen;
                            s.pen_path = None;
                            s.node_region = false;
                            s.active_nodes = false;
                            s.focus.focus(w);
                        },
                    )),
            )
            .child(list)
            .into_any_element()
    }
    fn inspector(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs = div()
            .flex()
            .gap_1()
            .p_2()
            .border_b_1()
            .border_color(rgb(ui_color(Role::Border)))
            .child(
                self.button("プロパティ", self.inspector_tab == 0, cx, |s, _, _| {
                    s.inspector_tab = 0
                }),
            )
            .child(self.button(
                "グリッド・ガイド",
                self.inspector_tab == 1,
                cx,
                |s, _, _| s.inspector_tab = 1,
            ));
        let content = if self.inspector_tab == 0 {
            self.properties(cx)
        } else {
            div()
                .id("grid-panel")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .child(self.snap_properties(cx))
                .into_any_element()
        };
        div()
            .w(px(280.))
            .flex_shrink_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(rgb(ui_color(Role::Surface)))
            .border_l_1()
            .border_color(rgb(ui_color(Role::Border)))
            .child(self.paths_panel(cx))
            .child(tabs)
            .child(
                div().h(px(42.)).flex_shrink_0().child(
                    if self
                        .editing
                        .as_ref()
                        .is_some_and(|k| !crate::controls::is_numeric(k))
                    {
                        self.edit_strip(cx)
                    } else {
                        div()
                            .px_3()
                            .py_2()
                            .text_size(px(11.))
                            .text_color(rgb(ui_color(Role::Muted)))
                            .child("クリックで入力 · ドラッグで調整")
                            .into_any_element()
                    },
                ),
            )
            .child(content)
            .into_any_element()
    }
    fn properties(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .id(if self.merge_open {
                "merge-properties"
            } else {
                "properties"
            })
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(ui_color(Role::Surface)))
            .border_l_1()
            .border_color(rgb(ui_color(Role::Border)))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_color(rgb(ui_color(Role::Muted)))
                    .child("パスの設定"),
            );
        col = col.child(self.anchor_properties(cx));
        if let Some(path) = self.session.active() {
            let name = path.name.clone();
            let color = path.edge.unwrap_or([224; 3]);
            col = col
                .child(self.button_with_id(
                    ("path-name", path.id),
                    name.clone(),
                    false,
                    cx,
                    move |s, w, cx| s.start_edit(Editing::PathName, name.clone(), w, cx),
                ))
                .child(format!("{} パスを選択", self.session.selected.len()))
                .child(self.numeric_field("最大幅", Editing::Width, cx))
                .child(self.button(
                    format!("線色  #{:02X}{:02X}{:02X}", color[0], color[1], color[2]),
                    false,
                    cx,
                    move |s, w, cx| {
                        s.node_region = false;
                        s.active_nodes = false;
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
                .child(self.numeric_field("回転 °", Editing::Rotation, cx))
                .child(self.numeric_field("拡大率", Editing::Scale, cx))
                .child(self.button("複製 ⌘D", false, cx, |s, _, cx| {
                    s.session.duplicate();
                    s.refresh(cx);
                }))
                .child(self.button("パスを削除", false, cx, |s, _, cx| {
                    s.session.remove();
                    s.refresh(cx);
                }))
                .child(div().h(px(8.)))
                .child(
                    self.button("ノードを編集", self.nodes_open, cx, |s, w, cx| {
                        s.nodes_open = true;
                        s.node_region = true;
                        s.active_nodes = true;
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
        col.into_any_element()
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.node_height = self
            .node_height
            .min((f32::from(window.viewport_size().height) * 0.48).max(180.));
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
            .child(self.tool_palette(cx))
            .child(canvas)
            .child(self.inspector(cx));
        let mut root = crate::menus::handlers(div(), self, cx)
            .relative()
            .key_context("PathWeaver")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(ui_color(Role::Background)))
            .text_color(rgb(ui_color(Role::Text)))
            .text_size(px(12.))
            .font_family(crate::assets::UI_FONT)
            .on_key_down(cx.listener(Self::key))
            .on_key_up(cx.listener(|s, e: &KeyUpEvent, _, _| {
                if e.keystroke.key == "q" {
                    s.numeric_snap = false;
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|s, _, _, cx| {
                    if s.node_move_modal {
                        s.finish_interaction(false, cx);
                    }
                }),
            )
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::mouse_up))
            .child(self.toolbar(cx));
        root = root.child(main);
        if self.nodes_open {
            root = root.child(self.nodes_panel(cx));
        }
        root = root.child(
            div()
                .h(px(25.))
                .flex_shrink_0()
                .px_3()
                .flex()
                .items_center()
                .justify_between()
                .bg(rgb(ui_color(Role::Toolbar)))
                .text_color(rgb(ui_color(Role::Muted)))
                .child(self.status.clone())
                .child(format!(
                    "{} パス    {:.0}%",
                    self.session.doc.strokes.len(),
                    self.art_view.zoom * 100.
                )),
        );
        if self.add_open {
            root = root.child(self.node_add_popup(window, cx));
        }
        root
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

#[cfg(test)]
mod edit_target_tests {
    use super::{EditTarget, Editing};
    use crate::session::Session;
    use kurbo::Point;
    use pathweaver_core::appearance::{Graph, Kind};

    fn paths_with_graph() -> (Session, u64, u64, u64) {
        let mut session = Session::default();
        let first = session.new_path(Point::new(0., 0.));
        let mut graph = Graph::default();
        let node = graph.add(Kind::Fill, [200., 100.]).unwrap();
        session.active_mut().unwrap().appearance = Some(graph);
        let second = session.new_path(Point::new(100., 0.));
        session.selected = vec![first];
        (session, first, second, node)
    }

    #[test]
    fn node_color_stays_on_the_original_node_after_selection_changes() {
        let (mut session, first, second, node) = paths_with_graph();
        let target = EditTarget::capture(&session, Some(node));
        let untouched_edge = session.active().unwrap().edge;
        session.selected = vec![second];
        let other_edge = session.active().unwrap().edge;
        target.apply_color(&mut session.doc, [12, 34, 56]).unwrap();
        let original = session.doc.strokes.iter().find(|s| s.id == first).unwrap();
        assert_eq!(
            original
                .appearance
                .as_ref()
                .unwrap()
                .node(node)
                .unwrap()
                .color,
            [12, 34, 56]
        );
        assert_eq!(original.edge, untouched_edge);
        assert_eq!(session.active().unwrap().edge, other_edge);
    }

    #[test]
    fn path_color_never_becomes_a_node_color() {
        let (mut session, first, second, node) = paths_with_graph();
        let target = EditTarget::capture(&session, None);
        let node_color = session.graph().unwrap().node(node).unwrap().color;
        session.selected = vec![second];
        target.apply_color(&mut session.doc, [90, 80, 70]).unwrap();
        let original = session.doc.strokes.iter().find(|s| s.id == first).unwrap();
        assert_eq!(original.edge, Some([90, 80, 70]));
        assert_eq!(
            original
                .appearance
                .as_ref()
                .unwrap()
                .node(node)
                .unwrap()
                .color,
            node_color
        );
    }

    #[test]
    fn rename_uses_the_clicked_node_even_when_another_node_is_active() {
        let (mut session, _, _, clicked) = paths_with_graph();
        let active = session
            .graph_mut()
            .unwrap()
            .add(Kind::Shadow, [400., 100.])
            .unwrap();
        let target = EditTarget::capture(&session, Some(clicked));
        let active_name = session.graph().unwrap().node(active).unwrap().label.clone();
        target.node_mut(&mut session.doc).unwrap().label = "クリックした対象".into();
        assert_eq!(
            session.graph().unwrap().node(clicked).unwrap().label,
            "クリックした対象"
        );
        assert_eq!(
            session.graph().unwrap().node(active).unwrap().label,
            active_name
        );
    }

    #[test]
    fn removed_target_is_rejected_without_modifying_another_path() {
        let (mut session, first, second, node) = paths_with_graph();
        let target = EditTarget::capture(&session, Some(node));
        session.doc.strokes.retain(|s| s.id != first);
        session.selected = vec![second];
        let before = session.doc.clone();
        assert!(target.apply_color(&mut session.doc, [1, 2, 3]).is_err());
        assert_eq!(session.doc, before);
    }

    #[test]
    fn numeric_edit_rejects_a_changed_selection_instead_of_redirecting() {
        let (mut session, _, second, node) = paths_with_graph();
        let target = EditTarget::capture(&session, Some(node));
        assert!(target.matches_numeric(&session, &[node], &Editing::Width));
        session.selected = vec![second];
        assert!(!target.matches_numeric(&session, &[node], &Editing::Width));
        assert!(target.matches_numeric(&session, &[], &Editing::GridSpacing));
    }
}
