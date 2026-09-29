use super::{
    model::*,
    outline::{self, Outline},
};
use egui::{Color32, Pos2, Stroke as EStroke, Vec2 as EVec2};
use kurbo::{Point, Vec2};
use std::{collections::HashMap, path::PathBuf};

const INK: Color32 = Color32::from_gray(224);
const ACCENT: Color32 = Color32::from_rgb(255, 112, 24);
const GUIDE: Color32 = Color32::from_gray(155);
const PAPER: Color32 = Color32::from_gray(13);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Direct,
    Pen,
    Rectangle,
    Ellipse,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum OutlineView {
    #[default]
    Fill,
    Always,
    Editing,
}
#[derive(Clone, Copy)]
enum Drag {
    Primitive(Point, bool),
    Guide(usize),
    Contour(usize, usize),
    Corner {
        node: u64,
        origin: Point,
        radius: f64,
    },
    Scale {
        rotation: f64,
        anchor: Point,
        handle: Point,
        axes: Vec2,
        center: Point,
    },
    Rotate {
        center: Point,
        angle: f64,
    },
    Marquee(Point, Point),
    Nodes(Point),
    Node(usize),
    Handle(usize, bool),
    Pen(usize),
    Width(WidthTarget, bool),
    WidthPosition(u64),
    Cap(bool),
    Move(Point),
    Segment(usize, f64, Point),
    Pan(Pos2, EVec2),
}
#[derive(Clone, Copy)]
enum Pending {
    New,
    Open,
    Example,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Canvas,
    Project,
    Properties,
    Grid,
    Layers,
}

pub struct Editor {
    dock: egui_dock::DockState<Panel>,
    canvas_rect: egui::Rect,
    doc: Document,
    saved: Document,
    file: Option<PathBuf>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    transaction: Option<Document>,
    selected: Option<u64>,
    width_selected: Option<WidthTarget>,
    node_selected: Option<usize>,
    handle_selected: Option<(u64, bool)>,
    selected_nodes: Vec<u64>,
    boolean_selection: Vec<u64>,
    show_widths: bool,
    tool_changed: bool,
    bend_segments: bool,
    scale_widths: bool,
    corner_all: bool,
    corner_kind: CornerKind,
    tool: Tool,
    drawing: bool,
    extend_start: bool,
    resume_endpoint: bool,
    drag: Option<Drag>,
    zoom: f32,
    pan: EVec2,
    preview: bool,
    outline_view: OutlineView,
    grid: bool,
    show_guides: bool,
    rulers: bool,
    lock_guides: bool,
    cache: HashMap<u64, (Stroke, Result<Outline, String>)>,
    status: String,
    pending: Option<Pending>,
    allow_close: bool,
}

impl Editor {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            "C:/Windows/Fonts/meiryo.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                fonts
                    .font_data
                    .insert("jp".into(), egui::FontData::from_owned(data).into());
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .push("jp".into());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        Self::theme().apply_to_context(&cc.egui_ctx, true);
        Self::default()
    }
    fn theme() -> crate::shared_ui::Theme {
        let mut t = crate::shared_ui::Theme::dark();
        t.canvas = PAPER;
        t.panel = Color32::from_gray(16);
        t.bg1 = Color32::from_gray(21);
        t.bg2 = Color32::from_gray(34);
        t.border = Color32::from_gray(38);
        t.text1 = Color32::from_gray(232);
        t.text2 = Color32::from_gray(170);
        t.accent = Color32::from_gray(220);
        t.accent_bg = Color32::from_gray(40);
        t
    }
    fn default_dock() -> egui_dock::DockState<Panel> {
        use egui_dock::NodeIndex;
        let mut dock = egui_dock::DockState::new(vec![Panel::Canvas]);
        let [center, right] = dock.main_surface_mut().split_right(
            NodeIndex::root(),
            0.81,
            vec![Panel::Properties, Panel::Grid],
        );
        dock.main_surface_mut()
            .split_below(right, 0.54, vec![Panel::Layers]);
        dock.main_surface_mut()
            .split_left(center, 0.14 / 0.81, vec![Panel::Project]);
        dock
    }
    fn stroke(&self) -> Option<&Stroke> {
        let id = self.selected?;
        self.doc.strokes.iter().find(|s| s.id == id)
    }
    fn stroke_mut(&mut self) -> Option<&mut Stroke> {
        let id = self.selected?;
        self.doc.strokes.iter_mut().find(|s| s.id == id)
    }
    fn dirty(&self) -> bool {
        self.doc != self.saved
    }
    fn record(&mut self, old: Document) {
        if old != self.doc {
            self.undo.push(old);
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    fn commit(&mut self) {
        if matches!(self.drag, Some(Drag::Primitive(..)))
            && self.transaction.is_some()
            && self.stroke().is_some_and(|s| {
                let mut min = Point::new(f64::INFINITY, f64::INFINITY);
                let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
                for n in &s.nodes {
                    min.x = min.x.min(n.p.x);
                    min.y = min.y.min(n.p.y);
                    max.x = max.x.max(n.p.x);
                    max.y = max.y.max(n.p.y);
                }
                max.x - min.x < 1e-6 || max.y - min.y < 1e-6
            })
        {
            self.cancel_drag();
            return;
        }
        if let Some(old) = self.transaction.take() {
            self.record(old);
        }
        self.drag = None;
    }
    fn cancel_drag(&mut self) {
        if let Some(old) = self.transaction.take() {
            self.doc = old;
            self.clean_selection();
        }
        self.drag = None;
    }
    fn undo(&mut self) {
        self.cancel_drag();
        if let Some(old) = self.undo.pop() {
            self.redo.push(self.doc.clone());
            self.doc = old;
            self.clean_selection();
        }
    }
    fn redo(&mut self) {
        self.cancel_drag();
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.doc.clone());
            self.doc = next;
            self.clean_selection();
        }
    }
    fn clean_selection(&mut self) {
        self.handle_selected = None;
        self.selected_nodes.clear();
        self.width_selected = None;
        self.node_selected = None;
        self.drawing = false;
        if self.stroke().is_none() {
            self.selected = self.doc.strokes.last().map(|s| s.id);
        }
    }
    fn screen(&self, p: Point, rect: egui::Rect) -> Pos2 {
        rect.min + self.pan + EVec2::new(p.x as f32, p.y as f32) * self.zoom
    }
    fn world(&self, p: Pos2, rect: egui::Rect) -> Point {
        let v = (p - rect.min - self.pan) / self.zoom;
        Point::new(v.x as f64, v.y as f64)
    }
    fn selection_bounds(&self) -> Option<kurbo::Rect> {
        let (_, result) = self.cache.get(&self.selected?)?;
        let o = result.as_ref().ok()?;
        let mut min = Point::new(f64::INFINITY, f64::INFINITY);
        let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        let unrotate = kurbo::Affine::rotate(-self.stroke()?.frame_angle);
        for p in &o.vertices {
            let p = unrotate * *p;
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        (min.x.is_finite() && max.x > min.x && max.y > min.y)
            .then(|| kurbo::Rect::from_points(min, max))
    }
    fn corner_targets(&self) -> Vec<u64> {
        self.doc
            .strokes
            .iter()
            .filter(|s| s.visible)
            .flat_map(|s| {
                s.nodes.iter().enumerate().filter_map(|(i, n)| {
                    let selected = if self.corner_all {
                        Some(s.id) == self.selected
                    } else {
                        self.selected_nodes.contains(&n.id)
                            || (Some(s.id) == self.selected && self.node_selected == Some(i))
                    };
                    (selected && super::corners::eligible(s, i)).then_some(n.id)
                })
            })
            .collect()
    }
    fn set_corners(&mut self, targets: &[u64], radius: f64, kind: CornerKind) {
        for s in &mut self.doc.strokes {
            for id in targets {
                if !s.nodes.iter().any(|n| n.id == *id) {
                    continue;
                }
                if let Some(c) = s.corners.iter_mut().find(|c| c.node == *id) {
                    c.radius = radius;
                    c.kind = kind;
                } else {
                    s.corners.push(Corner {
                        node: *id,
                        radius,
                        kind,
                    });
                }
            }
        }
    }
    fn set_corner_radii(&mut self, targets: &[u64], radius: f64) {
        let mut proposed = self.doc.clone();
        for s in &mut proposed.strokes {
            for id in targets {
                if !s.nodes.iter().any(|n| n.id == *id) {
                    continue;
                }
                if let Some(c) = s.corners.iter_mut().find(|c| c.node == *id) {
                    c.radius = radius;
                } else {
                    s.corners.push(Corner {
                        node: *id,
                        radius,
                        kind: self.corner_kind,
                    });
                }
            }
        }
        let mut limit = 10000_f64;
        for s in &proposed.strokes {
            for (i, n) in s
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| targets.contains(&n.id))
            {
                let kind = s
                    .corners
                    .iter()
                    .find(|c| c.node == n.id)
                    .map_or(self.corner_kind, |c| c.kind);
                limit = limit.min(super::corners::radius_limit(s, i, kind));
            }
        }
        let radius = radius.clamp(0., limit);
        for id in targets {
            let kind = self
                .doc
                .strokes
                .iter()
                .flat_map(|s| &s.corners)
                .find(|c| c.node == *id)
                .map_or(self.corner_kind, |c| c.kind);
            self.set_corners(&[*id], radius, kind);
        }
    }
    fn transform_hit(&self, p: Point) -> Option<Drag> {
        let rotation = self.stroke()?.frame_angle;
        let world_p = p;
        let p = kurbo::Affine::rotate(-rotation) * p;
        let r = self.selection_bounds()?;
        let center = r.center();
        for (x, y) in [
            (0., 0.),
            (0.5, 0.),
            (1., 0.),
            (1., 0.5),
            (1., 1.),
            (0.5, 1.),
            (0., 1.),
            (0., 0.5),
        ] {
            let handle = Point::new(r.x0 + r.width() * x, r.y0 + r.height() * y);
            if handle.distance(p) * (self.zoom as f64) < 8. {
                return Some(Drag::Scale {
                    rotation,
                    anchor: Point::new(r.x1 - r.width() * x, r.y1 - r.height() * y),
                    handle,
                    axes: Vec2::new(
                        if x == 0.5 { 0. } else { 1. },
                        if y == 0.5 { 0. } else { 1. },
                    ),
                    center,
                });
            }
        }
        let rotation_grip = Point::new(center.x, r.y0 - 28. / self.zoom as f64);
        let near_corner = [(r.x0, r.y0), (r.x1, r.y0), (r.x0, r.y1), (r.x1, r.y1)]
            .iter()
            .any(|(x, y)| Point::new(*x, *y).distance(p) * (self.zoom as f64) < 22.);
        if rotation_grip.distance(p) * (self.zoom as f64) < 8. || (!r.contains(p) && near_corner) {
            let center = kurbo::Affine::rotate(rotation) * center;
            let v = world_p - center;
            return Some(Drag::Rotate {
                center,
                angle: v.y.atan2(v.x),
            });
        }
        None
    }
    fn press_with_shift(
        &mut self,
        p: Point,
        screen: Pos2,
        ctrl: bool,
        space: bool,
        shift: bool,
        alt: bool,
    ) {
        self.bend_segments = alt;
        if shift && !ctrl && !space && self.tool == Tool::Direct && !self.preview {
            let hit = self
                .doc
                .strokes
                .iter()
                .rev()
                .filter(|s| s.visible)
                .find_map(|s| {
                    s.nodes
                        .iter()
                        .find(|n| n.p.distance(p) * (self.zoom as f64) < 7.)
                        .map(|n| (s.id, n.id))
                });
            if let Some((stroke, id)) = hit {
                if let Some(n) = self
                    .stroke()
                    .and_then(|s| self.node_selected.and_then(|i| s.nodes.get(i)))
                {
                    if !self.selected_nodes.contains(&n.id) {
                        self.selected_nodes.push(n.id);
                    }
                }
                if self.selected_nodes.contains(&id) {
                    self.selected_nodes.retain(|n| *n != id);
                } else {
                    self.selected_nodes.push(id);
                }
                self.selected = Some(stroke);
                self.node_selected = None;
                self.width_selected = None;
                self.drag = None;
                return;
            }
        }
        self.press(p, screen, ctrl, space);
    }
    fn set_tool(&mut self, tool: Tool) {
        self.tool_changed = true;
        self.handle_selected = None;
        self.selected_nodes.clear();
        self.commit();
        self.tool = tool;
        self.drawing = false;
        self.resume_endpoint = false;
        self.width_selected = None;
        self.node_selected = None;
    }

    fn save(&mut self, save_as: bool) -> bool {
        self.commit();
        let path = if save_as || self.file.is_none() {
            rfd::FileDialog::new()
                .add_filter("PathWeaver", &["pathweaver"])
                .set_file_name("Untitled.pathweaver")
                .save_file()
        } else {
            self.file.clone()
        };
        let Some(path) = path else {
            return false;
        };
        let result = (|| -> Result<(), String> {
            self.doc.validate()?;
            let bytes = serde_json::to_vec_pretty(&self.doc).map_err(|e| e.to_string())?;
            let tmp = path.with_extension(format!("pathweaver.{}.tmp", std::process::id()));
            std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::rename(&tmp, &path) {
                let _ = std::fs::remove_file(&tmp);
                return Err(e.to_string());
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.file = Some(path);
                self.saved = self.doc.clone();
                self.status = "プロジェクトを保存しました".into();
                true
            }
            Err(e) => {
                self.status = format!("保存できません: {e}");
                false
            }
        }
    }
    fn export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_file_name("Artwork.svg")
            .save_file()
        else {
            return;
        };
        match outline::svg(&self.doc)
            .and_then(|data| std::fs::write(path, data).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.status =
                    "SVGを書き出しました — 編集用データはプロジェクト保存で保持できます".into()
            }
            Err(e) => self.status = format!("SVGを書き出せません: {e}"),
        }
    }
    fn request(&mut self, action: Pending) {
        self.commit();
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action);
        }
    }
    fn perform(&mut self, action: Pending) {
        let document = match action {
            Pending::Close => {
                self.allow_close = true;
                return;
            }
            Pending::New => Document::default(),
            Pending::Example => Document::example(),
            Pending::Open => {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("PathWeaver", &["pathweaver"])
                    .pick_file()
                else {
                    return;
                };
                let result = (|| -> Result<Document, String> {
                    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024
                    {
                        return Err("ファイルが大きすぎます（上限16MB）".into());
                    }
                    let mut doc: Document =
                        serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?;
                    doc.upgrade()?;
                    Ok(doc)
                })();
                match result {
                    Ok(doc) => {
                        self.file = Some(path);
                        doc
                    }
                    Err(e) => {
                        self.status = format!("開けません: {e}");
                        return;
                    }
                }
            }
        };
        if !matches!(action, Pending::Open) {
            self.file = None;
        }
        self.doc = document;
        self.saved = self.doc.clone();
        self.undo.clear();
        self.redo.clear();
        self.cache.clear();
        self.selected = self.doc.strokes.first().map(|s| s.id);
        self.clean_selection();
        self.pan = EVec2::new(40., 80.);
        self.zoom = 1.;
        self.status = "準備できました".into();
    }
    fn confirmation(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.pending {
            egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                ui.heading("変更を保存しますか？");
                ui.label("まだ保存していない編集があります。");
                ui.horizontal(|ui| {
                    if ui.button("保存して続ける").clicked() && self.save(false) {
                        self.pending = None;
                        self.perform(action);
                    }
                    if ui.button("保存せず続ける").clicked() {
                        self.pending = None;
                        self.perform(action);
                    }
                    if ui.button("キャンセル").clicked() {
                        self.pending = None;
                    }
                });
            });
        }
    }
    fn top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .exact_height(40.)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let name = self
                        .file
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or("Untitled".into());
                    ui.strong(format!("{}{}", name, if self.dirty() { " *" } else { "" }));
                    ui.separator();
                    ui.menu_button("ファイル", |ui| {
                        if ui.button("新規").clicked() {
                            self.request(Pending::New);
                            ui.close();
                        }
                        if ui.button("開く…    Ctrl+O").clicked() {
                            self.request(Pending::Open);
                            ui.close();
                        }
                        if ui.button("保存    Ctrl+S").clicked() {
                            self.save(false);
                            ui.close();
                        }
                        if ui.button("名前を付けて保存…").clicked() {
                            self.save(true);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("SVGを書き出す…").clicked() {
                            self.export();
                            ui.close();
                        }
                    });
                    ui.menu_button("編集", |ui| {
                        if ui
                            .add_enabled(
                                !self.undo.is_empty(),
                                egui::Button::new("元に戻す    Ctrl+Z"),
                            )
                            .clicked()
                        {
                            self.undo();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                !self.redo.is_empty(),
                                egui::Button::new("やり直す    Ctrl+Shift+Z"),
                            )
                            .clicked()
                        {
                            self.redo();
                            ui.close();
                        }
                    });
                    ui.menu_button("パネル", |ui| {
                        if ui.button("配置をリセット").clicked() {
                            self.dock = Self::default_dock();
                            ui.close();
                        }
                    });
                    ui.separator();
                    ui.checkbox(&mut self.preview, "仕上がり表示");
                });
            });
    }
    fn project_ui(&mut self, ui: &mut egui::Ui) {
        for (tool, label) in [
            (Tool::Select, "V  選択"),
            (Tool::Direct, "A  ダイレクト選択"),
            (Tool::Pen, "P  ペン"),
            (Tool::Rectangle, "M  四角"),
            (Tool::Ellipse, "L  楕円"),
        ] {
            if ui.selectable_label(self.tool == tool, label).clicked() {
                self.set_tool(tool);
            }
        }
        ui.separator();
        if ui.button("新しいパス").clicked() {
            self.selected = None;
            self.set_tool(Tool::Pen);
        }
        if ui.button("サンプルを開く").clicked() {
            self.request(Pending::Example);
        }
        ui.separator();
        ui.label("操作");
        ui.small("A：アンカー・接線を編集");
        ui.small("P：端点をクリックして描画を再開");
        ui.small("回転中にShift：角度をスナップ");
        ui.small("Ctrl＋芯をクリック：幅点を追加");
        ui.small("Alt＋芯のドラッグ：曲線を曲げる");
        ui.small("Ctrl＋端点：切り口を回転");
        ui.small("Alt＋幅ハンドル：片側だけ調整");
        ui.small("中ボタンドラッグ：パン");
        ui.small("ホイール：ズーム");
        ui.small("Enter：描画終了 / Esc：取消");
    }
    fn layers_ui(&mut self, ui: &mut egui::Ui) {
        let rows: Vec<_> = self
            .doc
            .strokes
            .iter()
            .map(|s| (s.id, s.name.clone(), s.visible))
            .collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (id, name, mut visible) in rows.into_iter().rev() {
                ui.horizontal(|ui| {
                    let mut operand = self.boolean_selection.contains(&id);
                    if ui
                        .checkbox(&mut operand, "")
                        .on_hover_text("パスファインダーの対象")
                        .changed()
                    {
                        self.boolean_selection.retain(|x| *x != id);
                        if operand {
                            self.boolean_selection.push(id);
                        }
                    }
                    if ui.checkbox(&mut visible, "").changed() {
                        if let Some(s) = self.doc.strokes.iter_mut().find(|s| s.id == id) {
                            s.visible = visible;
                        }
                    }
                    if ui
                        .selectable_label(self.selected == Some(id), name)
                        .clicked()
                    {
                        self.selected = Some(id);
                        self.selected_nodes.clear();
                        self.node_selected = None;
                        self.width_selected = None;
                        self.drawing = false;
                    }
                });
            }
        });
    }
    fn expand_selected(&mut self) {
        let Some(source) = self.stroke().cloned() else {
            return;
        };
        match outline::paint(&source) {
            Ok(o) => {
                let position = self
                    .doc
                    .strokes
                    .iter()
                    .position(|s| s.id == source.id)
                    .unwrap();
                self.doc.strokes.remove(position);
                for (i, layer) in o.layers.into_iter().enumerate() {
                    let mut shape = source.clone();
                    if i > 0 {
                        shape.id = self.doc.id();
                    }
                    shape.contours = Some(layer.outline.contours);
                    shape.closed = true;
                    shape.nodes.clear();
                    shape.widths.clear();
                    shape.corners.clear();
                    shape.fill = Some(layer.color);
                    shape.edge = None;
                    self.doc.strokes.insert(position + i, shape);
                }
                self.clean_selection();
                self.tool = Tool::Select;
                self.drawing = false;
            }
            Err(e) => self.status = e,
        }
    }
    fn combine_selected(&mut self, op: i_overlay::core::overlay_rule::OverlayRule) {
        let sources: Vec<_> = self
            .doc
            .strokes
            .iter()
            .filter(|s| self.boolean_selection.contains(&s.id))
            .cloned()
            .collect();
        if sources.len() < 2 {
            return;
        }
        let result = (|| -> Result<Vec<Vec<Point>>, String> {
            let mut contours = outline::paint(&sources[0])?.contours;
            for s in &sources[1..] {
                contours = outline::boolean_contours(&contours, &outline::paint(s)?.contours, op);
            }
            Ok(contours)
        })();
        match result {
            Ok(contours) => {
                let mut result = sources[0].clone();
                result.contours = Some(contours);
                result.fill = result.edge.or(result.fill);
                result.edge = None;
                result.nodes.clear();
                result.widths.clear();
                result.corners.clear();
                result.name = format!("{} / 合成", result.name);
                let position = self
                    .doc
                    .strokes
                    .iter()
                    .position(|s| s.id == result.id)
                    .unwrap();
                self.doc
                    .strokes
                    .retain(|s| !self.boolean_selection.contains(&s.id));
                self.selected = Some(result.id);
                self.doc
                    .strokes
                    .insert(position.min(self.doc.strokes.len()), result);
                self.boolean_selection.clear();
                self.clean_selection();
                self.tool = Tool::Select;
                self.drawing = false;
            }
            Err(e) => self.status = e,
        }
    }
    fn properties_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| self.properties_contents(ui));
    }
    fn properties_contents(&mut self, ui: &mut egui::Ui) {
        if let Some(s) = self.stroke_mut() {
            ui.label("塗りと線");
            for (label, value) in [("塗り", &mut s.fill), ("線", &mut s.edge)] {
                ui.horizontal(|ui| {
                    let mut enabled = value.is_some();
                    if ui.checkbox(&mut enabled, label).changed() {
                        *value = enabled.then_some([224, 224, 224]);
                    }
                    if let Some(rgb) = value {
                        ui.color_edit_button_srgb(rgb);
                    }
                });
            }
            if s.edge.is_some() {
                let mut width = if s.contours.is_some() {
                    s.edge_width
                } else {
                    s.start_width.left + s.start_width.right
                };
                if ui
                    .add(
                        egui::DragValue::new(&mut width)
                            .range(0.0..=2000.0)
                            .speed(0.2)
                            .prefix("線幅 "),
                    )
                    .on_hover_text("パス自体の太さ。幅点も同じ太さに揃えます")
                    .changed()
                {
                    s.set_line_width(width);
                }
            }
            if s.contours.is_none() && s.nodes.len() > 2 {
                ui.checkbox(&mut s.closed, "パスを閉じる");
            }
        }
        let can_expand = self
            .stroke()
            .is_some_and(|s| s.contours.is_none() && s.nodes.len() > 1);
        if ui
            .add_enabled(can_expand, egui::Button::new("アウトライン化"))
            .clicked()
        {
            self.expand_selected();
        }
        ui.separator();
        ui.label("パスファインダー");
        ui.small("レイヤー左端のチェックで対象を指定");
        ui.small("塗りと線を合成\n前面を型抜き：一番背面からほかを引く");
        ui.add_enabled_ui(self.boolean_selection.len() >= 2, |ui| {
            ui.horizontal_wrapped(|ui| {
                use i_overlay::core::overlay_rule::OverlayRule;
                for (label, op) in [
                    ("合体", OverlayRule::Union),
                    ("前面を型抜き", OverlayRule::Difference),
                    ("交差", OverlayRule::Intersect),
                    ("中マド", OverlayRule::Xor),
                ] {
                    if ui.button(label).clicked() {
                        self.combine_selected(op);
                    }
                }
            });
        });
        ui.separator();
        ui.label("表示");
        ui.checkbox(&mut self.show_widths, "太さのハンドルを常時表示（A・P）");
        ui.checkbox(&mut self.scale_widths, "拡大縮小に合わせて線幅も変更");
        if self.tool == Tool::Select {
            ui.small(
                "枠：拡大縮小 / 枠外・上の丸：回転\nShift：比率固定・角度スナップ / Alt：中心基準",
            );
        }
        ui.horizontal_wrapped(|ui| {
            for (value, icon, label) in [
                (OutlineView::Fill, super::icons::FILL, "塗り"),
                (OutlineView::Always, super::icons::OUTLINE, "輪郭"),
                (OutlineView::Editing, super::icons::EDITING, "編集中のみ"),
            ] {
                if super::icons::choice(ui, icon, label, self.outline_view == value).clicked() {
                    self.outline_view = value;
                }
            }
        });
        ui.separator();
        ui.label("角を丸める");
        ui.checkbox(&mut self.corner_all, "選択パスのすべての角");
        let targets = self.corner_targets();
        let existing = self
            .doc
            .strokes
            .iter()
            .flat_map(|s| &s.corners)
            .find(|c| targets.contains(&c.node));
        let mut radius = existing.map_or(0., |c| c.radius);
        let mut kind = existing.map_or(self.corner_kind, |c| c.kind);
        let values: Vec<_> = targets
            .iter()
            .map(|id| {
                self.doc
                    .strokes
                    .iter()
                    .flat_map(|s| &s.corners)
                    .find(|c| c.node == *id)
                    .map_or((0., self.corner_kind), |c| (c.radius, c.kind))
            })
            .collect();
        let mixed_radius = values.iter().any(|(r, _)| (*r - radius).abs() > 1e-6);
        let mixed_kind = values.iter().any(|(_, k)| *k != kind);
        ui.small(format!("対象：{}個の角", targets.len()));
        let mut kind_changed = false;
        if mixed_kind {
            ui.small("種類：混在");
        }
        ui.horizontal_wrapped(|ui| {
            for (value, icon, label) in [
                (CornerKind::Circular, super::icons::ROUND, "円弧"),
                (CornerKind::Clothoid, super::icons::CLOTHOID, "クロソイド"),
                (CornerKind::Bevel, super::icons::BEVEL, "ベベル"),
            ] {
                if super::icons::choice(ui, icon, label, !mixed_kind && kind == value).clicked() {
                    kind = value;
                    kind_changed = true;
                }
            }
        });
        self.corner_kind = kind;
        let changed = ui
            .add_enabled(
                !targets.is_empty(),
                egui::DragValue::new(&mut radius)
                    .range(0.0..=10000.0)
                    .speed(0.5)
                    .custom_formatter(move |value, _| {
                        if mixed_radius {
                            "混在".into()
                        } else {
                            format!("{value:.2}")
                        }
                    })
                    .prefix("半径 "),
            )
            .changed();
        if kind_changed {
            for (id, (radius, _)) in targets.iter().zip(&values) {
                let limit = self
                    .doc
                    .strokes
                    .iter()
                    .find_map(|s| {
                        s.nodes
                            .iter()
                            .position(|n| n.id == *id)
                            .map(|i| super::corners::radius_limit(s, i, kind))
                    })
                    .unwrap_or(0.);
                self.set_corners(&[*id], radius.min(limit), kind);
            }
        }
        if changed {
            self.set_corner_radii(&targets, radius);
        }
        ui.small("半径0で元に戻す。隣の曲線にも接続");
        ui.separator();
        ui.label("回転のスナップ間隔");
        ui.add(
            egui::DragValue::new(&mut self.doc.angle_step)
                .range(1.0..=90.0)
                .speed(1.)
                .suffix("°"),
        );
        ui.separator();
        let width = self.width_selected;
        let node = self.node_selected;
        ui.scope(|ui| {
            if let Some(s) = self.stroke_mut() {
                ui.text_edit_singleline(&mut s.name);
                ui.label("角の接合");
                ui.horizontal_wrapped(|ui| {
                    for (value, icon, label) in [
                        (Join::Miter, super::icons::MITER, "マイター"),
                        (Join::Round, super::icons::ROUND, "ラウンド"),
                        (Join::Bevel, super::icons::BEVEL, "ベベル"),
                    ] {
                        if super::icons::choice(ui, icon, label, s.join == value).clicked() {
                            s.join = value;
                        }
                    }
                });
                if s.join == Join::Miter {
                    ui.add(
                        egui::DragValue::new(&mut s.miter_limit)
                            .range(1.0..=500.0)
                            .speed(0.1)
                            .prefix("マイター制限 "),
                    );
                }
                ui.separator();
                ui.label("切り口（ワールド座標）");
                ui.small("0° 水平 / 90° 垂直");
                if !s.closed && s.nodes.len() > 1 {
                    for (end, label) in [(false, "始点"), (true, "終点")] {
                        let mut angle = outline::cap_angle(s, end);
                        let enabled = if end {
                            &mut s.end_cap_locked
                        } else {
                            &mut s.start_cap_locked
                        };
                        if ui
                            .checkbox(enabled, format!("{label}の角度を指定"))
                            .changed()
                            && *enabled
                        {
                            if end {
                                s.end_angle = angle;
                            } else {
                                s.start_angle = angle;
                            }
                        }
                        let enabled = if end {
                            s.end_cap_locked
                        } else {
                            s.start_cap_locked
                        };
                        if ui
                            .add_enabled(
                                enabled,
                                egui::DragValue::new(&mut angle)
                                    .speed(1.)
                                    .prefix(label)
                                    .suffix("°"),
                            )
                            .changed()
                        {
                            let value = angle.rem_euclid(180.);
                            if end {
                                s.end_angle = value;
                                s.end_cap_locked = true;
                            } else {
                                s.start_angle = value;
                                s.start_cap_locked = true;
                            }
                        }
                    }
                }
                if let Some(target) = width {
                    if let Some(w) = s.width_mut(target) {
                        ui.separator();
                        ui.label("幅");
                        if ui.checkbox(&mut w.linked, "左右を連動").changed() && w.linked {
                            w.left = (w.left + w.right) * 0.5;
                            w.right = w.left;
                        }
                        if ui
                            .add(
                                egui::DragValue::new(&mut w.left)
                                    .range(0.0..=1000.0)
                                    .speed(0.25)
                                    .prefix("左 "),
                            )
                            .changed()
                            && w.linked
                        {
                            w.right = w.left;
                        }
                        if ui
                            .add(
                                egui::DragValue::new(&mut w.right)
                                    .range(0.0..=1000.0)
                                    .speed(0.25)
                                    .prefix("右 "),
                            )
                            .changed()
                            && w.linked
                        {
                            w.left = w.right;
                        }
                        ui.add(
                            egui::DragValue::new(&mut w.angle)
                                .range(-180.0..=180.0)
                                .speed(1.)
                                .prefix("方向 ")
                                .suffix("°"),
                        );
                    }
                }
                if let Some(i) = node {
                    if let Some(n) = s.nodes.get_mut(i) {
                        ui.separator();
                        ui.label("アンカー");
                        ui.checkbox(&mut n.smooth, "接線を連動");
                    }
                }
            } else {
                ui.label("パスを選択してください");
            }
        });
    }
    fn cache(&mut self) {
        self.cache
            .retain(|id, _| self.doc.strokes.iter().any(|s| s.id == *id));
        for s in &mut self.doc.strokes {
            if self.cache.get(&s.id).is_none_or(|(old, _)| old != s) {
                let result = outline::paint(s);
                if let Err(error) = &result {
                    self.status = error.clone();
                }
                self.cache.insert(s.id, (s.clone(), result));
            }
        }
    }
    fn hit_shape(&self, p: Point) -> Option<u64> {
        for s in self.doc.strokes.iter().rev().filter(|s| s.visible) {
            if let Some((_, Ok(o))) = self.cache.get(&s.id) {
                for tri in o.indices.chunks_exact(3) {
                    let a = o.vertices[tri[0] as usize];
                    let b = o.vertices[tri[1] as usize];
                    let c = o.vertices[tri[2] as usize];
                    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
                    let x = cross(b - a, p - a);
                    let y = cross(c - b, p - b);
                    let z = cross(a - c, p - c);
                    if (x >= 0. && y >= 0. && z >= 0.) || (x <= 0. && y <= 0. && z <= 0.) {
                        return Some(s.id);
                    }
                }
            }
            if outline::nearest(s, p).is_some_and(|(_, _, d)| d < 8. / self.zoom as f64) {
                return Some(s.id);
            }
        }
        None
    }
    fn hit_center(&self, p: Point) -> Option<(u64, usize, f64)> {
        self.doc
            .strokes
            .iter()
            .rev()
            .filter(|s| s.visible)
            .find_map(|s| {
                outline::nearest(s, p)
                    .filter(|(_, _, d)| *d < 7. / self.zoom as f64)
                    .map(|(i, t, _)| (s.id, i, t))
            })
    }
    fn pen_edit_target(&self, p: Point) -> Option<(u64, usize, f64, bool)> {
        for s in self.doc.strokes.iter().rev().filter(|s| s.visible) {
            if let Some(i) = s
                .nodes
                .iter()
                .position(|n| n.p.distance(p) < 7. / self.zoom as f64)
            {
                if s.closed || (i > 0 && i + 1 < s.nodes.len()) {
                    return Some((s.id, i, 0., true));
                }
                return None; // endpoints resume drawing
            }
        }
        self.hit_center(p)
            .filter(|(_, _, t)| (0.02..0.98).contains(t))
            .map(|(id, i, t)| (id, i, t, false))
    }
    fn snap_point(&self, mut p: Point) -> Point {
        let original = p;
        if self.doc.snap_grid {
            let step = self.doc.grid_spacing;
            p.x = (p.x / step).round() * step;
            p.y = (p.y / step).round() * step;
        }
        if self.doc.snap_guides && self.show_guides {
            for vertical in [true, false] {
                let v = if vertical { original.x } else { original.y };
                if let Some(g) = self
                    .doc
                    .guides
                    .iter()
                    .filter(|g| {
                        g.vertical == vertical && (g.position - v).abs() < 7. / self.zoom as f64
                    })
                    .min_by(|a, b| (a.position - v).abs().total_cmp(&(b.position - v).abs()))
                {
                    if vertical {
                        p.x = g.position;
                    } else {
                        p.y = g.position;
                    }
                }
            }
        }
        p
    }
    fn grid_ui(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.grid, "グリッドを表示");
        ui.checkbox(&mut self.doc.snap_grid, "グリッドにスナップ");
        ui.add(
            egui::DragValue::new(&mut self.doc.grid_spacing)
                .range(1.0..=10000.0)
                .prefix("間隔 "),
        );
        ui.separator();
        ui.checkbox(&mut self.rulers, "定規を表示");
        ui.small("定規からドラッグしてガイドを作成");
        ui.checkbox(&mut self.show_guides, "ガイドを表示");
        ui.checkbox(&mut self.doc.snap_guides, "ガイドにスナップ");
        ui.checkbox(&mut self.lock_guides, "ガイドをロック");
        ui.small("ロック解除後はVでドラッグ移動");
        ui.horizontal(|ui| {
            for (label, vertical) in [("水平ガイド", false), ("垂直ガイド", true)] {
                if ui.button(label).clicked() {
                    let p = self.world(self.canvas_rect.center(), self.canvas_rect);
                    self.doc.guides.push(Guide {
                        vertical,
                        position: if vertical { p.x } else { p.y },
                    });
                }
            }
        });
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, g) in self.doc.guides.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut g.position)
                            .range(-1e6..=1e6)
                            .prefix(if g.vertical { "X " } else { "Y " }),
                    );
                    if ui.button("削除").clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = remove {
            self.doc.guides.remove(i);
        }
    }
    fn primitive_to(&mut self, start: Point, end: Point, ellipse: bool, alt: bool, shift: bool) {
        let mut d = end - start;
        if shift {
            let size = d.x.abs().max(d.y.abs());
            d.x = size * if d.x < 0. { -1. } else { 1. };
            d.y = size * if d.y < 0. { -1. } else { 1. };
        }
        let a = if alt { start - d } else { start };
        let b = start + d;
        let min = Point::new(a.x.min(b.x), a.y.min(b.y));
        let max = Point::new(a.x.max(b.x), a.y.max(b.y));
        if let Some(s) = self.stroke_mut() {
            let center = min.lerp(max, 0.5);
            let rx = (max.x - min.x) * 0.5;
            let ry = (max.y - min.y) * 0.5;
            let k = 0.5522847498307936;
            let data = if ellipse {
                [
                    (Point::new(center.x, min.y), Vec2::new(rx * k, 0.)),
                    (Point::new(max.x, center.y), Vec2::new(0., ry * k)),
                    (Point::new(center.x, max.y), Vec2::new(-rx * k, 0.)),
                    (Point::new(min.x, center.y), Vec2::new(0., -ry * k)),
                ]
            } else {
                [
                    (min, Vec2::ZERO),
                    (Point::new(max.x, min.y), Vec2::ZERO),
                    (max, Vec2::ZERO),
                    (Point::new(min.x, max.y), Vec2::ZERO),
                ]
            };
            for (n, (p, v)) in s.nodes.iter_mut().zip(data) {
                n.p = p;
                n.outgoing = v;
                n.incoming = -v;
                n.smooth = ellipse;
            }
        }
    }
    fn press(&mut self, p: Point, screen: Pos2, ctrl: bool, space: bool) {
        self.handle_selected = None;
        if space {
            self.drag = Some(Drag::Pan(screen, self.pan));
            return;
        }
        if self.preview {
            return;
        }
        if self.tool == Tool::Select && self.show_guides && !self.lock_guides {
            if let Some(i) = self.doc.guides.iter().position(|g| {
                (g.position - if g.vertical { p.x } else { p.y }).abs() < 6. / self.zoom as f64
            }) {
                self.drag = Some(Drag::Guide(i));
                return;
            }
        }
        if matches!(self.tool, Tool::Rectangle | Tool::Ellipse) {
            let p = self.snap_point(p);
            let id = self.doc.id();
            let ellipse = self.tool == Tool::Ellipse;
            let mut shape = Stroke::new(
                id,
                if ellipse {
                    "楕円".into()
                } else {
                    "四角".into()
                },
            );
            shape.closed = true;
            shape.fill = Some([224, 224, 224]);
            shape.edge = None;
            shape.set_line_width(1.);
            for _ in 0..4 {
                shape.nodes.push(Node {
                    id: self.doc.id(),
                    p,
                    incoming: Vec2::ZERO,
                    outgoing: Vec2::ZERO,
                    smooth: ellipse,
                    exact_incoming: false,
                    exact_outgoing: false,
                });
            }
            self.doc.strokes.push(shape);
            self.selected = Some(id);
            self.node_selected = None;
            self.width_selected = None;
            self.selected_nodes.clear();
            self.drawing = false;
            self.drag = Some(Drag::Primitive(p, ellipse));
            return;
        }
        let radius = 7. / self.zoom as f64;
        if self.tool == Tool::Pen && self.show_widths && !ctrl {
            if let Some(s) = self.stroke().filter(|s| s.visible) {
                if !s.nodes.iter().any(|n| n.p.distance(p) < radius) {
                    for target in s.width_targets() {
                        if let Some((c, n)) = outline::width_location(s, target) {
                            let w = s.width(target).unwrap();
                            for (left, q) in [
                                (true, c + n * (w.left + 10. / self.zoom as f64)),
                                (false, c - n * (w.right + 10. / self.zoom as f64)),
                            ] {
                                if q.distance(p) < radius {
                                    self.width_selected = Some(target);
                                    self.node_selected = None;
                                    self.drag = Some(Drag::Width(target, left));
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        if self.tool == Tool::Select && !ctrl {
            if let Some(drag) = self.transform_hit(p) {
                self.drag = Some(drag);
                return;
            }
        }
        if self.tool == Tool::Pen && self.stroke().is_some_and(|s| s.contours.is_some()) {
            self.selected = None;
            self.drawing = false;
        }
        if self.tool == Tool::Direct {
            if let Some(s) = self.stroke().filter(|s| s.visible) {
                if let Some(contours) = &s.contours {
                    let nearest = contours
                        .iter()
                        .enumerate()
                        .flat_map(|(i, c)| {
                            c.iter()
                                .enumerate()
                                .map(move |(j, q)| (i, j, q.distance(p)))
                        })
                        .min_by(|a, b| a.2.total_cmp(&b.2));
                    if let Some((i, j, d)) = nearest {
                        if d < 7. / self.zoom as f64 {
                            self.drag = Some(Drag::Contour(i, j));
                            return;
                        }
                    }
                }
            }
        }
        if self.tool == Tool::Direct && !ctrl {
            for s in self.doc.strokes.iter().filter(|s| s.visible) {
                for (i, n) in s.nodes.iter().enumerate() {
                    if !(self.selected_nodes.contains(&n.id)
                        || Some(s.id) == self.selected
                            && (self.node_selected == Some(i) || self.corner_all))
                    {
                        continue;
                    }
                    if let Some((h, _, _)) = super::corners::widget(s, i, self.zoom as f64) {
                        let radius = s.corners.iter().find(|c| c.node == n.id).map_or(0., |c| {
                            c.radius.min(super::corners::radius_limit(s, i, c.kind))
                        });
                        if h.distance(p) < 7. / self.zoom as f64 {
                            self.drag = Some(Drag::Corner {
                                node: n.id,
                                origin: p,
                                radius,
                            });
                            return;
                        }
                    }
                }
            }
            let group_hit = self.doc.strokes.iter().filter(|s| s.visible).any(|s| {
                s.nodes
                    .iter()
                    .any(|n| self.selected_nodes.contains(&n.id) && n.p.distance(p) < radius)
            });
            if group_hit {
                self.drag = Some(Drag::Nodes(p));
                return;
            }
            self.selected_nodes.clear();
        }
        if self.tool == Tool::Pen && self.drawing && !ctrl {
            if let Some(s) = self.stroke_mut() {
                if !s.closed && s.nodes.len() > 2 && s.nodes[0].p.distance(p) < radius {
                    s.closed = true;
                    self.drawing = false;
                    self.drag = None;
                    return;
                }
            }
        }
        if self.tool == Tool::Pen && !ctrl {
            if let Some((id, i, t, remove)) = self.pen_edit_target(p) {
                self.selected = Some(id);
                self.width_selected = None;
                if remove {
                    self.node_selected = Some(i);
                    self.delete();
                } else {
                    let new_id = self.doc.id();
                    self.stroke_mut().unwrap().split(i, t, new_id);
                    self.node_selected = Some(i + 1);
                }
                self.drawing = false;
                self.drag = None;
                return;
            }
            if !self.drawing {
                let endpoint = self
                    .doc
                    .strokes
                    .iter()
                    .rev()
                    .filter(|s| s.visible && !s.closed)
                    .find_map(|s| {
                        s.nodes
                            .iter()
                            .enumerate()
                            .find(|(i, n)| {
                                (*i == 0 || *i + 1 == s.nodes.len()) && n.p.distance(p) < radius
                            })
                            .map(|(i, _)| (s.id, i, s.nodes.len()))
                    });
                if let Some((id, i, len)) = endpoint {
                    self.selected = Some(id);
                    self.node_selected = Some(i);
                    self.width_selected = None;
                    self.extend_start = i == 0 && len > 1;
                    self.resume_endpoint = len > 1;
                    self.drawing = true;
                    return;
                }
                let id = self.doc.id();
                self.doc.strokes.push(Stroke::new(
                    id,
                    format!("パス {}", self.doc.strokes.len() + 1),
                ));
                self.selected = Some(id);
                self.drawing = true;
                self.extend_start = false;
                self.resume_endpoint = false;
            }
            let id = self.doc.id();
            let key_id = if self.resume_endpoint {
                Some(self.doc.id())
            } else {
                None
            };
            let prepend = self.extend_start;
            let s = self.stroke_mut().unwrap();
            if let Some(key_id) = key_id {
                let (segment, t, width) = if prepend {
                    (s.nodes[0].id, 0., s.start_width)
                } else {
                    (s.nodes.last().unwrap().id, 0., s.end_width)
                };
                s.widths.push(WidthKey {
                    id: key_id,
                    segment,
                    t,
                    width,
                });
            }
            let p = self.snap_point(p);
            let s = self.stroke_mut().unwrap();
            let node = Node {
                exact_incoming: false,
                exact_outgoing: false,
                id,
                p,
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
            };
            let i = if prepend {
                s.nodes.insert(0, node);
                0
            } else {
                s.nodes.push(node);
                s.nodes.len() - 1
            };
            self.resume_endpoint = false;
            self.node_selected = Some(i);
            self.width_selected = None;
            self.drag = Some(Drag::Pen(i));
            return;
        }
        if self.tool == Tool::Select && !ctrl {
            self.selected = self.hit_shape(p);
            self.node_selected = None;
            self.width_selected = None;
            self.drag = self.selected.map(|_| Drag::Move(p));
            return;
        }
        // The displayed rotation grip is an input target too, and takes
        // precedence over width grips only while Ctrl/Command is held.
        if ctrl {
            if let Some(s) = self
                .stroke()
                .filter(|s| s.visible && !s.closed && s.nodes.len() > 1)
            {
                for end in [false, true] {
                    let c = if end {
                        s.nodes.last().unwrap().p
                    } else {
                        s.nodes[0].p
                    };
                    let h = c + outline::cap_direction(s, end) * (42. / self.zoom as f64);
                    if c.distance(p) < radius || h.distance(p) < radius {
                        self.drag = Some(Drag::Cap(end));
                        return;
                    }
                }
            }
        }
        // Anchors remain selectable even when a zero-width grip overlaps them.
        if let Some(s) = self.stroke().filter(|s| s.visible) {
            if let Some(i) = s.nodes.iter().position(|n| n.p.distance(p) < radius) {
                if ctrl && !s.closed && s.nodes.len() > 1 && (i == 0 || i == s.nodes.len() - 1) {
                    self.drag = Some(Drag::Cap(i != 0));
                    return;
                }
                if !ctrl {
                    self.node_selected = Some(i);
                    self.width_selected = None;
                    self.drag = Some(Drag::Node(i));
                    return;
                }
            }
        }
        if let Some(s) = self.stroke().filter(|s| s.visible) {
            for target in s.width_targets().into_iter().filter(|t| {
                ctrl || (self.show_widths && self.tool != Tool::Select)
                    || self.width_selected == Some(*t)
            }) {
                if let Some((c, n)) = outline::width_location(s, target) {
                    let w = s.width(target).unwrap();
                    for (left, q) in [
                        (true, c + n * (w.left + 10. / self.zoom as f64)),
                        (false, c - n * (w.right + 10. / self.zoom as f64)),
                    ] {
                        if q.distance(p) < radius {
                            self.width_selected = Some(target);
                            self.node_selected = None;
                            self.drag = Some(Drag::Width(target, left));
                            return;
                        }
                    }
                }
            }
        }
        if ctrl {
            let hit = self
                .stroke()
                .into_iter()
                .chain(
                    self.doc
                        .strokes
                        .iter()
                        .rev()
                        .filter(|s| Some(s.id) != self.selected),
                )
                .filter(|s| s.visible && !s.closed && s.nodes.len() > 1)
                .find_map(|s| {
                    [false, true].into_iter().find_map(|end| {
                        let c = if end {
                            s.nodes.last().unwrap().p
                        } else {
                            s.nodes[0].p
                        };
                        (c.distance(p) < radius).then_some((s.id, end))
                    })
                });
            if let Some((id, end)) = hit {
                if self.selected != Some(id) {
                    self.drawing = false;
                }
                self.selected = Some(id);
                self.drag = Some(Drag::Cap(end));
                return;
            }
            if let Some((id, _, _)) = self.hit_center(p) {
                self.selected = Some(id);
                let key = self
                    .stroke()
                    .unwrap()
                    .widths
                    .iter()
                    .find(|k| {
                        outline::width_location(self.stroke().unwrap(), WidthTarget::Key(k.id))
                            .is_some_and(|(c, _)| c.distance(p) < radius)
                    })
                    .map(|k| k.id);
                if let Some(id) = key {
                    self.width_selected = Some(WidthTarget::Key(id));
                    self.drag = Some(Drag::WidthPosition(id));
                } else {
                    self.add_width(p);
                }
            }
            return;
        }
        if let Some(s) = self.stroke().filter(|s| s.visible) {
            for (i, n) in s.nodes.iter().enumerate() {
                if n.p.distance(p) < radius {
                    self.node_selected = Some(i);
                    self.width_selected = None;
                    self.drag = Some(Drag::Node(i));
                    return;
                }
                for (out, v) in [(true, n.outgoing), (false, n.incoming)] {
                    if !s.closed && ((out && i + 1 == s.nodes.len()) || (!out && i == 0)) {
                        continue;
                    }
                    if v.hypot() > 0.01 && (n.p + v).distance(p) < radius {
                        self.handle_selected = Some((n.id, out));
                        self.node_selected = Some(i);
                        self.width_selected = None;
                        self.drag = Some(Drag::Handle(i, out));
                        return;
                    }
                }
            }
        }
        if let Some((id, i, t)) = self.hit_center(p) {
            self.selected = Some(id);
            self.width_selected = None;
            self.node_selected = None;
            self.drawing = false;
            self.drag = if self.bend_segments {
                Some(Drag::Segment(i, t, p))
            } else {
                None
            };
            return;
        }
        self.selected = None;
        self.node_selected = None;
        self.width_selected = None;
        if self.tool == Tool::Direct {
            self.drag = Some(Drag::Marquee(p, p));
        }
    }
    fn add_width(&mut self, p: Point) {
        let Some(s) = self.stroke() else {
            return;
        };
        let Some((i, t, d)) = outline::nearest(s, p) else {
            return;
        };
        if d > 8. / self.zoom as f64
            || (i == 0 && t < 0.002)
            || (i == s.nodes.len() - 2 && t > 0.998)
        {
            return;
        }
        if s.widths
            .iter()
            .any(|k| k.segment == s.nodes[i].id && (k.t - t).abs() < 0.01)
        {
            return;
        }
        let samples = outline::samples(s);
        let width = outline::Profile::new(s, &samples).width(outline::at_distance(&samples, i, t));
        let id = self.doc.id();
        let s = self.stroke_mut().unwrap();
        s.widths.push(WidthKey {
            id,
            segment: s.nodes[i].id,
            t,
            width,
        });
        self.width_selected = Some(WidthTarget::Key(id));
        self.node_selected = None;
    }
    #[cfg(test)]
    fn drag_to(&mut self, p: Point, screen: Pos2, alt: bool) {
        self.drag_with_modifiers(p, screen, alt, false);
    }
    fn drag_with_modifiers(&mut self, p: Point, screen: Pos2, alt: bool, shift: bool) {
        let Some(drag) = self.drag else {
            return;
        };
        if let Drag::Pan(start, pan) = drag {
            self.pan = pan + (screen - start);
            return;
        }
        if let Drag::Guide(i) = drag {
            if let Some(g) = self.doc.guides.get_mut(i) {
                g.position = if g.vertical { p.x } else { p.y };
            }
            return;
        }
        if let Drag::Primitive(start, ellipse) = drag {
            self.primitive_to(start, self.snap_point(p), ellipse, alt, shift);
            return;
        }
        let p = if matches!(drag, Drag::Node(_) | Drag::Contour(..) | Drag::Scale { .. }) {
            self.snap_point(p)
        } else {
            p
        };
        if let Drag::Corner {
            node,
            origin,
            radius,
        } = drag
        {
            let info = self.doc.strokes.iter().find_map(|s| {
                s.nodes
                    .iter()
                    .position(|n| n.id == node)
                    .and_then(|i| crate::corners::widget(s, i, self.zoom as f64))
            });
            if let Some((_, dir, factor)) = info {
                let mut targets = self.corner_targets();
                if targets.is_empty() {
                    targets.push(node);
                }
                self.set_corner_radii(&targets, (radius + (p - origin).dot(dir) / factor).max(0.));
            }
            return;
        }
        if matches!(drag, Drag::Scale { .. } | Drag::Rotate { .. }) {
            let Some(old) = self
                .transaction
                .as_ref()
                .and_then(|d| d.strokes.iter().find(|s| Some(s.id) == self.selected))
                .cloned()
            else {
                return;
            };
            let (center, linear) = match drag {
                Drag::Scale {
                    rotation,
                    anchor,
                    handle,
                    axes,
                    center,
                } => {
                    let p = kurbo::Affine::rotate(-rotation) * p;
                    let origin = if alt { center } else { anchor };
                    let ratio = |value: f64, base: f64| {
                        if base.abs() < 1e-9 {
                            1.
                        } else {
                            (value / base).clamp(0.01, 100.)
                        }
                    };
                    let mut sx = if axes.x != 0. {
                        ratio(p.x - origin.x, handle.x - origin.x)
                    } else {
                        1.
                    };
                    let mut sy = if axes.y != 0. {
                        ratio(p.y - origin.y, handle.y - origin.y)
                    } else {
                        1.
                    };
                    if shift {
                        let value = if axes.x == 0. {
                            sy
                        } else if axes.y == 0. || (sx - 1.).abs() > (sy - 1.).abs() {
                            sx
                        } else {
                            sy
                        };
                        sx = value;
                        sy = value;
                    }
                    (
                        kurbo::Affine::rotate(rotation) * origin,
                        kurbo::Affine::rotate(rotation)
                            * kurbo::Affine::scale_non_uniform(sx, sy)
                            * kurbo::Affine::rotate(-rotation),
                    )
                }
                Drag::Rotate { center, angle } => {
                    let v = p - center;
                    if v.hypot() < 1. / self.zoom as f64 {
                        return;
                    }
                    let mut delta = v.y.atan2(v.x) - angle;
                    if shift {
                        let step = self.doc.angle_step.to_radians();
                        delta = (delta / step).round() * step;
                    }
                    (center, kurbo::Affine::rotate(delta))
                }
                _ => unreachable!(),
            };
            let matrix = kurbo::Affine::translate(center.to_vec2())
                * linear
                * kurbo::Affine::translate(-center.to_vec2());
            let transformed = old.transformed(matrix, self.scale_widths);
            if let Some(s) = self.stroke_mut() {
                *s = transformed;
            }
            return;
        }
        if let Drag::Marquee(start, _) = drag {
            self.drag = Some(Drag::Marquee(start, p));
            self.selected_nodes = self
                .doc
                .strokes
                .iter()
                .filter(|s| s.visible)
                .flat_map(|s| &s.nodes)
                .filter(|n| {
                    n.p.x >= start.x.min(p.x)
                        && n.p.x <= start.x.max(p.x)
                        && n.p.y >= start.y.min(p.y)
                        && n.p.y <= start.y.max(p.y)
                })
                .map(|n| n.id)
                .collect();
            self.selected = self
                .doc
                .strokes
                .iter()
                .find(|s| s.nodes.iter().any(|n| self.selected_nodes.contains(&n.id)))
                .map(|s| s.id);
            return;
        }
        if let Drag::Nodes(start) = drag {
            let anchor = self
                .transaction
                .as_ref()
                .and_then(|d| {
                    d.strokes
                        .iter()
                        .flat_map(|s| &s.nodes)
                        .find(|n| self.selected_nodes.contains(&n.id))
                })
                .map_or(start, |n| n.p);
            let p = start + (self.snap_point(anchor + (p - start)) - anchor);
            if let Some(original) = &self.transaction {
                for s in &mut self.doc.strokes {
                    if let Some(old) = original.strokes.iter().find(|old| old.id == s.id) {
                        for n in &mut s.nodes {
                            if self.selected_nodes.contains(&n.id) {
                                if let Some(base) = old.nodes.iter().find(|base| base.id == n.id) {
                                    n.p = base.p + (p - start);
                                }
                            }
                        }
                    }
                }
            }
            return;
        }
        let original = self
            .transaction
            .as_ref()
            .and_then(|d| d.strokes.iter().find(|s| Some(s.id) == self.selected))
            .cloned();
        let cap_dead_zone = 12. / self.zoom as f64;
        let p = if let Drag::Move(start) = drag {
            if let Some(old) = &original {
                let anchor = old
                    .nodes
                    .first()
                    .map(|n| n.p)
                    .or_else(|| {
                        old.contours
                            .as_ref()
                            .and_then(|c| c.first())
                            .and_then(|c| c.first())
                            .copied()
                    })
                    .unwrap_or(start);
                start + (self.snap_point(anchor + (p - start)) - anchor)
            } else {
                p
            }
        } else {
            p
        };
        let grip_offset = 10. / self.zoom as f64;
        let step = self.doc.angle_step;
        let prepend = self.extend_start;
        let Some(s) = self.stroke_mut() else {
            return;
        };
        match drag {
            Drag::Contour(i, j) => {
                if let Some(contours) = &mut s.contours {
                    if let Some(q) = contours.get_mut(i).and_then(|c| c.get_mut(j)) {
                        *q = p;
                    }
                }
            }
            Drag::Segment(i, t, start) => {
                if let Some(old) = original {
                    let weight = 3. * t * (1. - t);
                    if weight > 1e-6 {
                        let delta = (p - start) / weight;
                        let c = old.cubic(i);
                        s.nodes[i].outgoing = c.p1 - c.p0 + delta;
                        s.nodes[i].exact_outgoing = true;
                        let next = (i + 1) % s.nodes.len();
                        s.nodes[next].incoming = c.p2 - c.p3 + delta;
                        s.nodes[next].exact_incoming = true;
                    }
                }
            }
            Drag::Node(i) => {
                if let Some(n) = s.nodes.get_mut(i) {
                    if alt {
                        n.exact_incoming = false;
                        n.exact_outgoing = false;
                        n.outgoing = p - n.p;
                        n.incoming = -n.outgoing;
                        n.smooth = true;
                    } else {
                        n.p = p;
                    }
                }
            }
            Drag::Pen(i) => {
                let n = &mut s.nodes[i];
                n.exact_incoming = false;
                n.exact_outgoing = false;
                n.outgoing = (p - n.p) * if prepend { -1. } else { 1. };
                n.incoming = -n.outgoing;
                n.smooth = true;
            }
            Drag::Handle(i, out) => {
                let n = &mut s.nodes[i];
                let v = p - n.p;
                if out {
                    n.exact_outgoing = false;
                    n.outgoing = v;
                    if n.smooth && !alt {
                        let len = n.incoming.hypot();
                        n.incoming = if v.hypot() > 1e-8 {
                            -v / v.hypot() * len
                        } else {
                            Vec2::ZERO
                        };
                    }
                } else {
                    n.exact_incoming = false;
                    n.incoming = v;
                    if n.smooth && !alt {
                        let len = n.outgoing.hypot();
                        n.outgoing = if v.hypot() > 1e-8 {
                            -v / v.hypot() * len
                        } else {
                            Vec2::ZERO
                        };
                    }
                }
            }
            Drag::Width(target, left) => {
                if let Some((c, n)) = outline::width_location(s, target) {
                    if let Some(w) = s.width_mut(target) {
                        let value = ((p - c).dot(n) * if left { 1. } else { -1. } - grip_offset)
                            .clamp(0., 1000.);
                        let base = original
                            .as_ref()
                            .and_then(|s| s.width(target))
                            .unwrap_or(*w);
                        *w = base;
                        if alt {
                            if left {
                                w.left = value;
                            } else {
                                w.right = value;
                            }
                        } else {
                            let delta = value - if left { base.left } else { base.right };
                            w.left = (base.left + delta).clamp(0., 1000.);
                            w.right = (base.right + delta).clamp(0., 1000.);
                        }
                    }
                }
            }
            Drag::WidthPosition(id) => {
                if let Some((i, t, _)) = outline::nearest(s, p) {
                    let segment = s.nodes[i].id;
                    let blocked = s
                        .widths
                        .iter()
                        .any(|k| k.id != id && k.segment == segment && (k.t - t).abs() < 0.005);
                    if !blocked {
                        if let Some(k) = s.widths.iter_mut().find(|k| k.id == id) {
                            k.segment = segment;
                            k.t = t.clamp(0.001, 0.999);
                        }
                    }
                }
            }
            Drag::Cap(end) => {
                let (i, t) = if end {
                    (s.nodes.len() - 2, 1.)
                } else {
                    (0, 0.)
                };
                let v = p - s.eval(i, t);
                // A click at the endpoint only reveals the control. Rotation
                // starts after moving away, avoiding atan2(0, 0) resetting it.
                if v.hypot() < cap_dead_zone {
                    return;
                }
                let angle = v.y.atan2(v.x).to_degrees();
                let angle = (if shift {
                    (angle / step).round() * step
                } else {
                    angle
                })
                .rem_euclid(180.);
                if end {
                    s.end_angle = angle;
                    s.end_cap_locked = true;
                } else {
                    s.start_angle = angle;
                    s.start_cap_locked = true;
                }
            }
            Drag::Move(start) => {
                if let Some(old) = original {
                    *s = old.transformed(kurbo::Affine::translate(p - start), false);
                }
            }
            Drag::Primitive(..)
            | Drag::Guide(..)
            | Drag::Pan(..)
            | Drag::Marquee(..)
            | Drag::Nodes(..)
            | Drag::Scale { .. }
            | Drag::Rotate { .. }
            | Drag::Corner { .. } => {}
        }
    }
    fn double_click(&mut self, p: Point) {
        if self.preview || self.tool != Tool::Direct {
            return;
        }
        let Some(s) = self.stroke() else {
            return;
        };
        let Some((i, t, d)) = outline::nearest(s, p) else {
            return;
        };
        if d > 8. / self.zoom as f64 || !(0.002..0.998).contains(&t) {
            return;
        }
        let id = self.doc.id();
        self.stroke_mut().unwrap().split(i, t, id);
        self.node_selected = Some(i + 1);
        self.width_selected = None;
    }
    fn delete(&mut self) {
        if !self.selected_nodes.is_empty() {
            let nodes = std::mem::take(&mut self.selected_nodes);
            for id in nodes {
                if let Some((stroke_id, index)) = self
                    .doc
                    .strokes
                    .iter()
                    .find_map(|s| s.nodes.iter().position(|n| n.id == id).map(|i| (s.id, i)))
                {
                    self.selected = Some(stroke_id);
                    self.node_selected = Some(index);
                    self.width_selected = None;
                    self.delete();
                }
            }
            return;
        }
        if let Some(WidthTarget::Key(id)) = self.width_selected {
            if let Some(s) = self.stroke_mut() {
                s.widths.retain(|k| k.id != id);
            }
            self.width_selected = None;
        } else if self.node_selected.is_some() {
            let i = self.node_selected.unwrap();
            if let Some(s) = self.stroke_mut() {
                if i < s.nodes.len() {
                    let previous = if s.closed {
                        Some((i + s.nodes.len() - 1) % s.nodes.len())
                    } else {
                        i.checked_sub(1)
                    };
                    let affected = [Some(s.nodes[i].id), previous.map(|j| s.nodes[j].id)];
                    let positions: Vec<_> = s
                        .widths
                        .iter()
                        .filter(|k| affected.contains(&Some(k.segment)))
                        .filter_map(|k| s.segment_index(k.segment).map(|j| (k.id, s.eval(j, k.t))))
                        .collect();
                    s.nodes.remove(i);
                    s.corners.retain(|c| s.nodes.iter().any(|n| n.id == c.node));
                    if s.nodes.len() < 2 {
                        s.widths.clear();
                    } else {
                        for (id, p) in positions {
                            if let Some((j, t, _)) = outline::nearest(s, p) {
                                let segment = s.nodes[j].id;
                                if let Some(k) = s.widths.iter_mut().find(|k| k.id == id) {
                                    k.segment = segment;
                                    k.t = t.clamp(0.001, 0.999);
                                }
                            }
                        }
                    }
                }
            }
            self.node_selected = None;
        } else if let Some(id) = self.selected {
            self.doc.strokes.retain(|s| s.id != id);
            self.clean_selection();
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        // Read modifiers attached to each key event. The current modifier state
        // may already be released when a fast shortcut arrives in one frame.
        let keys: Vec<_> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        let text_focus = ctx.wants_keyboard_input();
        for (key, m) in keys {
            if m.command || m.ctrl {
                match key {
                    egui::Key::S => {
                        self.save(m.shift);
                    }
                    egui::Key::O => self.request(Pending::Open),
                    egui::Key::Z if !text_focus => {
                        if m.shift {
                            self.redo();
                        } else {
                            self.undo();
                        }
                    }
                    egui::Key::Y if !text_focus => self.redo(),
                    egui::Key::R if !text_focus => self.rulers = !self.rulers,
                    egui::Key::Quote if !text_focus => {
                        if m.shift {
                            self.doc.snap_grid = !self.doc.snap_grid;
                        } else {
                            self.grid = !self.grid;
                        }
                    }
                    egui::Key::Semicolon if !text_focus => {
                        if m.alt {
                            self.lock_guides = !self.lock_guides;
                        } else {
                            self.show_guides = !self.show_guides;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if text_focus {
                continue;
            }
            match key {
                egui::Key::P => self.set_tool(Tool::Pen),
                egui::Key::M => self.set_tool(Tool::Rectangle),
                egui::Key::L => self.set_tool(Tool::Ellipse),
                egui::Key::A => self.set_tool(Tool::Direct),
                egui::Key::V => self.set_tool(Tool::Select),
                egui::Key::Tab => self.preview = !self.preview,
                egui::Key::Enter => {
                    self.commit();
                    self.drawing = false;
                    self.tool = Tool::Select;
                }
                egui::Key::Escape => {
                    if self.drag.is_some() {
                        self.cancel_drag();
                    } else {
                        self.drawing = false;
                        self.tool = Tool::Select;
                    }
                }
                egui::Key::Delete | egui::Key::Backspace => {
                    let old = self.doc.clone();
                    self.delete();
                    self.record(old);
                }
                _ => {}
            }
        }
    }
    fn canvas_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.canvas_rect = rect;
        if response.hovered() {
            let delta: f32 = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|event| {
                        if let egui::Event::MouseWheel { unit, delta, .. } = event {
                            Some(
                                delta.y
                                    * match unit {
                                        egui::MouseWheelUnit::Point => 1.,
                                        egui::MouseWheelUnit::Line => 40.,
                                        egui::MouseWheelUnit::Page => 200.,
                                    },
                            )
                        } else {
                            None
                        }
                    })
                    .sum()
            });
            if delta != 0. {
                if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                    let before = self.world(p, rect);
                    self.zoom = (self.zoom * (delta * 0.002).exp()).clamp(0.1, 12.);
                    self.pan =
                        p - rect.min - EVec2::new(before.x as f32, before.y as f32) * self.zoom;
                }
            }
        }
        if self.pending.is_none() && !self.tool_changed {
            if let Some(p) = ctx.input(|i| i.pointer.interact_pos()) {
                if response.hovered()
                    && ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle))
                {
                    let origin = ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
                    self.drag = Some(Drag::Pan(origin, self.pan));
                } else if response.hovered() && ctx.input(|i| i.pointer.primary_pressed()) {
                    if self.transaction.is_none() {
                        self.transaction = Some(self.doc.clone());
                    }
                    let origin = ctx.input(|i| i.pointer.press_origin()).unwrap_or(p);
                    if self.rulers
                        && !self.preview
                        && (origin.x < rect.left() + 18. || origin.y < rect.top() + 18.)
                    {
                        let vertical = origin.x < rect.left() + 18.;
                        let w = self.world(origin, rect);
                        self.doc.guides.push(Guide {
                            vertical,
                            position: if vertical { w.x } else { w.y },
                        });
                        self.show_guides = true;
                        self.drag = Some(Drag::Guide(self.doc.guides.len() - 1));
                    } else {
                        self.press_with_shift(
                            self.world(origin, rect),
                            origin,
                            ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command)),
                            ctx.input(|i| i.key_down(egui::Key::Space)),
                            ctx.input(|i| i.modifiers.shift),
                            ctx.input(|i| i.modifiers.alt),
                        );
                    }
                }
                if ctx.input(|i| {
                    i.pointer.primary_down() || i.pointer.button_down(egui::PointerButton::Middle)
                }) && self.drag.is_some()
                    && ctx.input(|i| {
                        i.pointer
                            .press_origin()
                            .is_some_and(|origin| origin.distance(p) > 2.)
                    })
                {
                    self.drag_with_modifiers(
                        self.world(p, rect),
                        p,
                        ctx.input(|i| i.modifiers.alt),
                        ctx.input(|i| i.modifiers.shift),
                    );
                }
                if response.double_clicked()
                    && !ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command))
                    && !matches!(self.drag, Some(Drag::Cap(_)))
                {
                    self.double_click(self.world(p, rect));
                }
            }
        }
        self.cache();
        if self.tool == Tool::Select && !self.preview && response.hovered() {
            let hit = self.drag.or_else(|| {
                response
                    .hover_pos()
                    .and_then(|p| self.transform_hit(self.world(p, rect)))
            });
            if let Some(drag) = hit {
                let cursor = match drag {
                    Drag::Scale { axes, .. } if axes.x == 0. => egui::CursorIcon::ResizeVertical,
                    Drag::Scale { axes, .. } if axes.y == 0. => egui::CursorIcon::ResizeHorizontal,
                    Drag::Scale { anchor, handle, .. }
                        if (handle.x - anchor.x) * (handle.y - anchor.y) < 0. =>
                    {
                        egui::CursorIcon::ResizeNeSw
                    }
                    Drag::Scale { .. } => egui::CursorIcon::ResizeNwSe,
                    Drag::Rotate { .. } => egui::CursorIcon::Grab,
                    _ => egui::CursorIcon::Default,
                };
                ctx.set_cursor_icon(cursor);
            }
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., PAPER);
        if self.grid && !self.preview {
            let base = self.doc.grid_spacing as f32 * self.zoom;
            let step = base * (8.0 / base).ceil().max(1.0);
            let origin = self.screen(Point::ZERO, rect);
            let mut x = rect.left() + (origin.x - rect.left()).rem_euclid(step);
            while x < rect.right() {
                painter.line_segment(
                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                    EStroke::new(0.5, Color32::from_gray(32)),
                );
                x += step;
            }
            let mut y = rect.top() + (origin.y - rect.top()).rem_euclid(step);
            while y < rect.bottom() {
                painter.line_segment(
                    [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                    EStroke::new(0.5, Color32::from_gray(32)),
                );
                y += step;
            }
        }
        if self.show_guides && !self.preview {
            for guide in &self.doc.guides {
                let p = self.screen(
                    if guide.vertical {
                        Point::new(guide.position, 0.)
                    } else {
                        Point::new(0., guide.position)
                    },
                    rect,
                );
                let ends = if guide.vertical {
                    [Pos2::new(p.x, rect.top()), Pos2::new(p.x, rect.bottom())]
                } else {
                    [Pos2::new(rect.left(), p.y), Pos2::new(rect.right(), p.y)]
                };
                painter.line_segment(ends, EStroke::new(0.65, Color32::from_gray(100)));
            }
        }
        for s in self.doc.strokes.iter().filter(|s| s.visible) {
            if let Some((_, Ok(o))) = self.cache.get(&s.id) {
                let wire = !self.preview
                    && (self.outline_view == OutlineView::Always
                        || (self.outline_view == OutlineView::Editing
                            && (self.selected == Some(s.id)
                                || s.nodes.iter().any(|n| self.selected_nodes.contains(&n.id)))
                            && self
                                .drag
                                .is_some_and(|d| !matches!(d, Drag::Pan(..) | Drag::Marquee(..)))));
                if wire {
                    for [a, b] in &o.boundary {
                        painter.line_segment(
                            [self.screen(*a, rect), self.screen(*b, rect)],
                            EStroke::new(0.7, INK),
                        );
                    }
                } else {
                    for layer in &o.layers {
                        let [r, g, b] = layer.color;
                        let color = Color32::from_rgb(r, g, b);
                        painter.add(egui::Shape::mesh(egui::Mesh {
                            indices: layer.outline.indices.clone(),
                            vertices: layer
                                .outline
                                .vertices
                                .iter()
                                .map(|p| egui::epaint::Vertex {
                                    pos: self.screen(*p, rect),
                                    uv: egui::epaint::WHITE_UV,
                                    color,
                                })
                                .collect(),
                            ..Default::default()
                        }));
                        for [a, b] in &layer.outline.boundary {
                            painter.line_segment(
                                [self.screen(*a, rect), self.screen(*b, rect)],
                                EStroke::new(0.7, color),
                            );
                        }
                    }
                }
                if !self.preview && self.tool == Tool::Direct && self.selected == Some(s.id) {
                    if let Some(contours) = &s.contours {
                        for (i, c) in contours.iter().enumerate() {
                            for (j, p) in c.iter().enumerate() {
                                let selected =
                                    matches!(self.drag, Some(Drag::Contour(a,b)) if a==i && b==j);
                                painter.rect_filled(
                                    egui::Rect::from_center_size(
                                        self.screen(*p, rect),
                                        EVec2::splat(if selected { 5. } else { 3. }),
                                    ),
                                    0.,
                                    if selected { ACCENT } else { GUIDE },
                                );
                            }
                        }
                    }
                }
                if !self.preview && self.selected == Some(s.id) {
                    painter.add(egui::Shape::line(
                        o.samples.iter().map(|p| self.screen(p.p, rect)).collect(),
                        EStroke::new(0.65, GUIDE),
                    ));
                }
            }
        }
        if !self.preview {
            if let Some(Drag::Marquee(a, b)) = self.drag {
                let bounds = egui::Rect::from_two_pos(self.screen(a, rect), self.screen(b, rect));
                painter.rect_filled(bounds, 0., Color32::from_white_alpha(12));
                painter.rect_stroke(bounds, 0., EStroke::new(0.7, INK), egui::StrokeKind::Middle);
            }
            if self.tool == Tool::Select {
                if let (Some(bounds), Some(s)) = (self.selection_bounds(), self.stroke()) {
                    let rotate = kurbo::Affine::rotate(s.frame_angle);
                    let screen = |x: f64, y: f64| self.screen(rotate * Point::new(x, y), rect);
                    let corners = vec![
                        screen(bounds.x0, bounds.y0),
                        screen(bounds.x1, bounds.y0),
                        screen(bounds.x1, bounds.y1),
                        screen(bounds.x0, bounds.y1),
                    ];
                    if s.frame_angle.abs() < 1e-9 {
                        painter.rect_stroke(
                            egui::Rect::from_two_pos(corners[0], corners[2]),
                            0.,
                            EStroke::new(0.7, ACCENT),
                            egui::StrokeKind::Middle,
                        );
                    } else {
                        painter.add(egui::Shape::closed_line(corners, EStroke::new(0.7, ACCENT)));
                    }
                    for (x, y) in [
                        (0., 0.),
                        (0.5, 0.),
                        (1., 0.),
                        (1., 0.5),
                        (1., 1.),
                        (0.5, 1.),
                        (0., 1.),
                        (0., 0.5),
                    ] {
                        let p = screen(
                            bounds.x0 + bounds.width() * x,
                            bounds.y0 + bounds.height() * y,
                        );
                        let marker = egui::Rect::from_center_size(p, EVec2::splat(5.));
                        painter.rect_filled(marker, 0., PAPER);
                        painter.rect_stroke(
                            marker,
                            0.,
                            EStroke::new(0.7, ACCENT),
                            egui::StrokeKind::Middle,
                        );
                    }
                    let c = bounds.center();
                    let top = screen(c.x, bounds.y0);
                    let h = screen(c.x, bounds.y0 - 28. / self.zoom as f64);
                    painter.line_segment([top, h], EStroke::new(0.7, ACCENT));
                    painter.circle_filled(h, 4., PAPER);
                    painter.circle_stroke(h, 4., EStroke::new(0.8, ACCENT));
                }
            }
        }
        if !self.preview && self.tool == Tool::Pen && self.drawing && self.drag.is_none() {
            if let (Some(s), Some(pos)) = (self.stroke(), response.hover_pos()) {
                if let Some(n) = if self.extend_start {
                    s.nodes.first()
                } else {
                    s.nodes.last()
                } {
                    let q = self.world(pos, rect);
                    let h = n.p
                        + if self.extend_start {
                            n.incoming
                        } else {
                            n.outgoing
                        };
                    let curve = (0..=40)
                        .map(|step| {
                            let t = step as f64 / 40.;
                            let u = 1. - t;
                            let p = n.p.to_vec2() * u.powi(3)
                                + h.to_vec2() * (3. * u * u * t)
                                + q.to_vec2() * (3. * u * t * t + t.powi(3));
                            self.screen(p.to_point(), rect)
                        })
                        .collect();
                    painter.add(egui::Shape::line(curve, EStroke::new(0.65, GUIDE)));
                }
            }
        }
        if !self.preview
            && (self.tool != Tool::Select || ctx.input(|i| i.modifiers.command || i.modifiers.ctrl))
        {
            for s in self.doc.strokes.iter().filter(|s| {
                s.visible
                    && (Some(s.id) == self.selected
                        || s.nodes.iter().any(|n| self.selected_nodes.contains(&n.id)))
            }) {
                // A exposes the complete set of editing controls.
                for (i, n) in s.nodes.iter().enumerate() {
                    let c = self.screen(n.p, rect);
                    for (out, v) in [(false, n.incoming), (true, n.outgoing)] {
                        if Some(s.id) != self.selected {
                            continue;
                        }
                        let pen_tip = self.tool == Tool::Pen
                            && self.drawing
                            && i == if self.extend_start {
                                0
                            } else {
                                s.nodes.len() - 1
                            };
                        if !s.closed
                            && !pen_tip
                            && ((out && i + 1 == s.nodes.len()) || (!out && i == 0))
                        {
                            continue;
                        }
                        if v.hypot() > 0.01 {
                            let h = self.screen(n.p + v, rect);
                            painter.line_segment([c, h], EStroke::new(0.65, GUIDE));
                            painter.circle_filled(h, 4., PAPER);
                            painter.circle_stroke(
                                h,
                                4.,
                                EStroke::new(
                                    1.,
                                    if self.handle_selected == Some((n.id, out)) {
                                        ACCENT
                                    } else {
                                        INK
                                    },
                                ),
                            );
                        }
                    }
                    let r = egui::Rect::from_center_size(c, EVec2::splat(5.5));
                    painter.rect_filled(
                        r,
                        0.,
                        if (Some(s.id) == self.selected && self.node_selected == Some(i))
                            || self.selected_nodes.contains(&n.id)
                        {
                            ACCENT
                        } else {
                            PAPER
                        },
                    );
                    painter.rect_stroke(
                        r,
                        0.,
                        EStroke::new(
                            0.7,
                            if (Some(s.id) == self.selected && self.node_selected == Some(i))
                                || self.selected_nodes.contains(&n.id)
                            {
                                ACCENT
                            } else {
                                GUIDE
                            },
                        ),
                        egui::StrokeKind::Middle,
                    );
                    if self.tool == Tool::Direct
                        && ((Some(s.id) == self.selected
                            && (self.node_selected == Some(i) || self.corner_all))
                            || self.selected_nodes.contains(&n.id))
                    {
                        if let Some((p, _, _)) = crate::corners::widget(s, i, self.zoom as f64) {
                            let h = self.screen(p, rect);
                            let corner = s.corners.iter().find(|c| c.node == n.id);
                            let at_limit = corner.is_some_and(|c| {
                                c.radius > 0.
                                    && c.radius >= crate::corners::radius_limit(s, i, c.kind) - 1e-6
                            });
                            let color = if at_limit {
                                Color32::from_rgb(255, 65, 65)
                            } else {
                                ACCENT
                            };
                            painter.circle_filled(h, 4., PAPER);
                            painter.circle_stroke(h, 4., EStroke::new(0.8, color));
                            painter.circle_filled(h, 1.2, color);
                        }
                    }
                }
                for target in s.width_targets().into_iter().filter(|target| {
                    Some(s.id) == self.selected
                        && (self.width_selected == Some(*target)
                            || (self.show_widths && self.tool != Tool::Select)
                            || ctx.input(|i| i.modifiers.command || i.modifiers.ctrl))
                }) {
                    if let Some((c, n)) = outline::width_location(s, target) {
                        let w = s.width(target).unwrap();
                        let a = self.screen(c + n * (w.left + 10. / self.zoom as f64), rect);
                        let b = self.screen(c - n * (w.right + 10. / self.zoom as f64), rect);
                        painter.line_segment([a, b], EStroke::new(0.65, GUIDE));
                        for p in [a, b] {
                            painter.circle_filled(p, 3., PAPER);
                            painter.circle_stroke(
                                p,
                                3.,
                                EStroke::new(
                                    0.8,
                                    if self.width_selected == Some(target) {
                                        ACCENT
                                    } else {
                                        INK
                                    },
                                ),
                            );
                        }
                        if matches!(target, WidthTarget::Key(_)) {
                            painter.circle_filled(
                                self.screen(c, rect),
                                2.5,
                                if self.width_selected == Some(target) {
                                    ACCENT
                                } else {
                                    GUIDE
                                },
                            );
                        }
                    }
                }
                if Some(s.id) == self.selected
                    && !s.closed
                    && s.nodes.len() > 1
                    && (ctx.input(|i| i.modifiers.command || i.modifiers.ctrl)
                        || matches!(self.drag, Some(Drag::Cap(_))))
                {
                    for end in [false, true] {
                        let c = if end {
                            s.nodes.last().unwrap().p
                        } else {
                            s.nodes[0].p
                        };
                        let d = outline::cap_direction(s, end);
                        let h = self.screen(c + d * (42. / self.zoom as f64), rect);
                        if !matches!(self.drag,Some(Drag::Cap(e)) if e==end)
                            && !ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
                                let a = self.screen(c, rect);
                                let v = h - a;
                                let t = ((p - a).dot(v) / v.length_sq().max(1.)).clamp(0., 1.);
                                p.distance(a + v * t) < 18.
                            })
                        {
                            continue;
                        }
                        painter.line_segment([self.screen(c, rect), h], EStroke::new(0.6, GUIDE));
                        painter.circle_filled(h, 4., PAPER);
                        painter.circle_stroke(
                            h,
                            4.,
                            EStroke::new(
                                1.,
                                if matches!(self.drag,Some(Drag::Cap(e)) if e==end) {
                                    ACCENT
                                } else {
                                    INK
                                },
                            ),
                        );
                        if ctx.input(|i| (i.modifiers.ctrl || i.modifiers.command))
                            || matches!(self.drag,Some(Drag::Cap(e)) if e==end)
                        {
                            for tick in 0..(180. / self.doc.angle_step).ceil() as usize {
                                let a = (tick as f64 * self.doc.angle_step).to_radians();
                                let n = Vec2::new(a.cos(), a.sin());
                                painter.line_segment(
                                    [
                                        self.screen(c + n * (48. / self.zoom as f64), rect),
                                        self.screen(c + n * (52. / self.zoom as f64), rect),
                                    ],
                                    EStroke::new(0.5, GUIDE),
                                );
                            }
                            painter.text(
                                h + EVec2::new(8., -8.),
                                egui::Align2::LEFT_BOTTOM,
                                format!("{:.1}°", outline::cap_angle(s, end)),
                                egui::FontId::proportional(10.),
                                INK,
                            );
                        }
                    }
                }
            }
        }
        if self.rulers && !self.preview {
            let ink = Color32::from_gray(120);
            let bg = Color32::from_gray(24);
            painter.rect_filled(
                egui::Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 18.)),
                0.,
                bg,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(rect.min, Pos2::new(rect.left() + 18., rect.bottom())),
                0.,
                bg,
            );
            let spacing = self.doc.grid_spacing
                * (55. / (self.doc.grid_spacing * self.zoom as f64))
                    .ceil()
                    .max(1.);
            let top = self.world(rect.min, rect);
            let bottom = self.world(rect.max, rect);
            let mut x = (top.x / spacing).ceil() * spacing;
            while x <= bottom.x {
                let p = self.screen(Point::new(x, 0.), rect);
                painter.text(
                    Pos2::new(p.x + 2., rect.top() + 2.),
                    egui::Align2::LEFT_TOP,
                    format!("{x:.0}"),
                    egui::FontId::proportional(9.),
                    ink,
                );
                painter.line_segment(
                    [
                        Pos2::new(p.x, rect.top() + 13.),
                        Pos2::new(p.x, rect.top() + 18.),
                    ],
                    EStroke::new(0.5, ink),
                );
                x += spacing;
            }
            let mut y = (top.y / spacing).ceil() * spacing;
            while y <= bottom.y {
                let p = self.screen(Point::new(0., y), rect);
                painter.text(
                    Pos2::new(rect.left() + 1., p.y + 2.),
                    egui::Align2::LEFT_TOP,
                    format!("{y:.0}"),
                    egui::FontId::proportional(8.),
                    ink,
                );
                painter.line_segment(
                    [
                        Pos2::new(rect.left() + 13., p.y),
                        Pos2::new(rect.left() + 18., p.y),
                    ],
                    EStroke::new(0.5, ink),
                );
                y += spacing;
            }
        }
        if self.tool == Tool::Pen
            && !self.preview
            && !ctx.input(|i| i.modifiers.ctrl || i.modifiers.command)
        {
            if let Some(pos) = response.hover_pos() {
                if let Some((_, _, _, remove)) = self.pen_edit_target(self.world(pos, rect)) {
                    painter.text(
                        pos + EVec2::new(13., -12.),
                        egui::Align2::LEFT_BOTTOM,
                        if remove { "−" } else { "+" },
                        egui::FontId::proportional(20.),
                        INK,
                    );
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
            }
        }
        painter.text(
            rect.max - EVec2::new(12., 12.),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.0}%", self.zoom * 100.),
            egui::FontId::proportional(10.),
            ACCENT,
        );
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(Pending::Close);
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.shortcuts(ctx);
        let before = self.doc.clone();
        if ctx.input(|i| i.pointer.any_pressed()) && self.transaction.is_none() {
            self.transaction = Some(before.clone());
        }
        self.top(ctx);

        egui::TopBottomPanel::bottom("status")
            .exact_height(24.)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new("●").color(GUIDE));
                    ui.small(&self.status);
                    if let Some(error) = self.cache.values().find_map(|(_, v)| v.as_ref().err()) {
                        ui.colored_label(Color32::GRAY, error);
                    }
                });
            });
        let mut dock = std::mem::replace(&mut self.dock, Self::default_dock());
        egui_dock::DockArea::new(&mut dock)
            .style(crate::shared_ui::dock_style(ctx, Self::theme()))
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show(ctx, &mut Viewer { editor: self });
        self.dock = dock;
        self.confirmation(ctx);
        self.tool_changed = false;
        if ctx.input(|i| i.pointer.any_released()) {
            self.commit();
        } else if self.transaction.is_none() {
            self.record(before);
        }
        let name = self
            .file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or("Untitled".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — PathWeaver",
            name,
            if self.dirty() { " *" } else { "" }
        )));
    }
}
impl Default for Editor {
    fn default() -> Self {
        let doc = Document::default();
        let selected = doc.strokes.first().map(|s| s.id);
        Self {
            dock: Self::default_dock(),
            canvas_rect: egui::Rect::NOTHING,
            saved: doc.clone(),
            doc,
            file: None,
            undo: vec![],
            redo: vec![],
            transaction: None,
            selected,
            width_selected: None,
            node_selected: None,
            handle_selected: None,
            selected_nodes: vec![],
            boolean_selection: vec![],
            show_widths: true,
            tool_changed: false,
            bend_segments: false,
            scale_widths: true,
            corner_all: false,
            corner_kind: CornerKind::Circular,
            tool: Tool::Pen,
            drawing: false,
            extend_start: false,
            resume_endpoint: false,
            drag: None,
            zoom: 1.,
            pan: EVec2::new(40., 80.),
            preview: false,
            outline_view: OutlineView::Fill,
            grid: true,
            show_guides: true,
            rulers: true,
            lock_guides: true,
            cache: HashMap::new(),
            status: "".into(),
            pending: None,
            allow_close: false,
        }
    }
}
struct Viewer<'a> {
    editor: &'a mut Editor,
}
impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Panel;
    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        match tab {
            Panel::Canvas => "キャンバス",
            Panel::Project => "プロジェクト",
            Panel::Properties => "プロパティ",
            Panel::Grid => "グリッド",
            Panel::Layers => "レイヤー",
        }
        .into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        match tab {
            Panel::Canvas => self.editor.canvas_ui(ui),
            Panel::Project => self.editor.project_ui(ui),
            Panel::Properties => self.editor.properties_ui(ui),
            Panel::Layers => self.editor.layers_ui(ui),
            Panel::Grid => {
                self.editor.grid_ui(ui);
            }
        }
    }
}
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example_editor() -> Editor {
        let doc = Document::example();
        Editor {
            selected: Some(doc.strokes[0].id),
            saved: doc.clone(),
            doc,
            tool: Tool::Direct,
            ..Editor::default()
        }
    }
    fn frame(
        ctx: &egui::Context,
        e: &mut Editor,
        time: f64,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    EVec2::new(1380., 900.),
                )),
                time: Some(time),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| e.ui(ctx),
        )
    }
    #[test]
    fn expanded_shape_moves_edits_roundtrips_and_undo_restores_centerline() {
        let (_, mut e) = setup();
        let before = e.doc.clone();
        let shape = outline::build(&e.doc.strokes[0]).unwrap().contours;
        e.expand_selected();
        e.record(before.clone());
        assert_eq!(e.doc.strokes[0].contours.as_ref().unwrap(), &shape);
        assert!(e.doc.strokes[0].nodes.is_empty());
        e.doc.validate().unwrap();
        e.doc.strokes[0].fill = Some([255, 112, 24]);
        e.doc.strokes[0].edge = Some([0, 0, 0]);
        e.doc.strokes[0].edge_width = 4.;
        let svg = outline::svg(&e.doc).unwrap();
        assert!(svg.contains("#ff7018"));
        assert!(svg.contains("#000000"));
        let read: Document = serde_json::from_str(&serde_json::to_string(&e.doc).unwrap()).unwrap();
        read.validate().unwrap();
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Move(Point::ZERO));
        e.drag_to(Point::new(17., -23.), Pos2::ZERO, false);
        assert!(
            e.doc.strokes[0].contours.as_ref().unwrap()[0][0]
                .distance(shape[0][0] + Vec2::new(17., -23.))
                < 1e-8
        );
        e.cancel_drag();
        e.set_tool(Tool::Direct);
        let p = e.doc.strokes[0].contours.as_ref().unwrap()[0][0];
        e.press(p, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Contour(..))));
        e.drag_to(p + Vec2::new(2., 3.), Pos2::ZERO, false);
        assert!(
            e.doc.strokes[0].contours.as_ref().unwrap()[0][0].distance(p + Vec2::new(2., 3.))
                < 1e-8
        );
        e.cancel_drag();
        e.undo();
        assert_eq!(e.doc, before);
    }
    #[test]
    fn pathfinder_replaces_only_operands_and_undo_restores_them() {
        let (_, mut e) = setup();
        let mut second = e.doc.strokes[0].transformed(kurbo::Affine::translate((120., 0.)), true);
        second.id = e.doc.id();
        for n in &mut second.nodes {
            n.id = e.doc.id();
        }
        second.widths.clear();
        e.boolean_selection = vec![e.doc.strokes[0].id, second.id];
        e.doc.strokes.push(second);
        let before = e.doc.clone();
        e.combine_selected(i_overlay::core::overlay_rule::OverlayRule::Union);
        e.record(before.clone());
        assert_eq!(e.doc.strokes.len(), 1);
        assert!(e.doc.strokes[0].contours.is_some());
        e.doc.validate().unwrap();
        e.undo();
        assert_eq!(e.doc, before);
    }
    fn setup() -> (egui::Context, Editor) {
        let ctx = egui::Context::default();
        let mut e = example_editor();
        frame(&ctx, &mut e, 0., vec![], egui::Modifiers::NONE);
        frame(&ctx, &mut e, 0.02, vec![], egui::Modifiers::NONE);
        (ctx, e)
    }
    fn button(
        p: Pos2,
        pressed: bool,
        button: egui::PointerButton,
        m: egui::Modifiers,
    ) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            pressed,
            button,
            modifiers: m,
        }
    }
    fn ctrl() -> egui::Modifiers {
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        }
    }
    #[test]
    fn rulers_create_guides_with_one_undo_and_closed_handles_are_editable() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let m = egui::Modifiers::NONE;
        let p = Pos2::new(e.canvas_rect.center().x, e.canvas_rect.top() + 5.);
        let q = p + EVec2::new(0., 100.);
        frame(
            &ctx,
            &mut e,
            1.,
            vec![
                egui::Event::PointerMoved(p),
                button(p, true, egui::PointerButton::Primary, m),
            ],
            m,
        );
        frame(&ctx, &mut e, 1.1, vec![egui::Event::PointerMoved(q)], m);
        frame(
            &ctx,
            &mut e,
            1.2,
            vec![button(q, false, egui::PointerButton::Primary, m)],
            m,
        );
        assert_eq!(e.doc.guides.len(), 1);
        assert!(!e.doc.guides[0].vertical);
        assert!((e.doc.guides[0].position - e.world(q, e.canvas_rect).y).abs() < 1e-6);
        e.undo();
        assert_eq!(e.doc, before);
        e.set_tool(Tool::Ellipse);
        e.press(Point::new(50., 50.), Pos2::ZERO, false, false);
        e.drag_to(Point::new(250., 200.), Pos2::ZERO, false);
        e.commit();
        e.set_tool(Tool::Direct);
        let s = e.stroke().unwrap();
        let handle = s.nodes[0].p + s.nodes[0].incoming;
        e.press(handle, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Handle(0, false))));
    }
    #[test]
    fn expansion_keeps_distinct_fill_and_stroke_colors() {
        let mut e = Editor::default();
        e.set_tool(Tool::Rectangle);
        e.press(Point::new(20., 20.), Pos2::ZERO, false, false);
        e.drag_to(Point::new(120., 120.), Pos2::ZERO, false);
        e.commit();
        let s = e.stroke_mut().unwrap();
        s.fill = Some([12, 34, 56]);
        s.edge = Some([255, 112, 24]);
        s.set_line_width(10.);
        let before = outline::svg(&e.doc).unwrap();
        e.expand_selected();
        assert_eq!(e.doc.strokes.len(), 2);
        assert_eq!(outline::svg(&e.doc).unwrap(), before);
        e.doc.validate().unwrap();
    }
    #[test]
    fn shape_style_fixture() {
        let mut fixtures = vec![];
        for (index, tool) in [
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Rectangle,
            Tool::Ellipse,
        ]
        .into_iter()
        .enumerate()
        {
            let mut e = Editor::default();
            e.set_tool(tool);
            let x = 40. + (index % 2) as f64 * 280.;
            let y = 40. + (index / 2) as f64 * 220.;
            e.press(Point::new(x, y), Pos2::ZERO, false, false);
            e.drag_to(Point::new(x + 200., y + 150.), Pos2::ZERO, false);
            e.commit();
            let s = e.stroke_mut().unwrap();
            s.edge = Some([224, 224, 224]);
            s.set_line_width(12.);
            s.fill = if index < 2 { None } else { Some([75, 75, 75]) };
            let o = outline::paint(s).unwrap();
            fixtures.push(serde_json::json!({"layers":o.layers.iter().map(|l|serde_json::json!({"color":l.color,"vertices":l.outline.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":l.outline.indices})).collect::<Vec<_>>()}));
        }
        if let Ok(path) = std::env::var("PATHWEAVER_SHAPE_FIXTURE") {
            std::fs::write(path, serde_json::to_string(&fixtures).unwrap()).unwrap();
        }
    }
    #[test]
    fn primitives_have_four_editable_anchors_and_modifier_geometry() {
        for ellipse in [false, true] {
            let mut e = Editor::default();
            e.set_tool(if ellipse {
                Tool::Ellipse
            } else {
                Tool::Rectangle
            });
            let old = e.doc.clone();
            e.transaction = Some(old.clone());
            e.press(Point::new(100., 100.), Pos2::ZERO, false, false);
            e.cache(); // zero-size pointer-down must be harmless
            e.drag_with_modifiers(Point::new(180., 140.), Pos2::ZERO, true, true);
            e.commit();
            let s = e.stroke().unwrap();
            assert!(s.closed);
            assert_eq!(s.nodes.len(), 4);
            let bounds = s.nodes.iter().fold(
                (Point::new(1e9, 1e9), Point::new(-1e9, -1e9)),
                |(mut a, mut b), n| {
                    a.x = a.x.min(n.p.x);
                    a.y = a.y.min(n.p.y);
                    b.x = b.x.max(n.p.x);
                    b.y = b.y.max(n.p.y);
                    (a, b)
                },
            );
            assert_eq!(bounds, (Point::new(20., 20.), Point::new(180., 180.)));
            assert_eq!(s.nodes[0].outgoing.hypot() > 0., ellipse);
            assert!(outline::paint(s).unwrap().vertices.len() > 3);
            e.doc.validate().unwrap();
            e.undo();
            assert_eq!(e.doc, old);
        }
    }
    #[test]
    fn grid_and_guides_snap_creation_movement_and_save() {
        let mut e = Editor::default();
        e.doc.grid_spacing = 20.;
        e.doc.snap_grid = true;
        e.set_tool(Tool::Rectangle);
        e.press(Point::new(13., 29.), Pos2::ZERO, false, false);
        e.drag_with_modifiers(Point::new(94., 87.), Pos2::ZERO, false, false);
        e.commit();
        assert_eq!(e.stroke().unwrap().nodes[0].p, Point::new(20., 20.));
        assert_eq!(e.stroke().unwrap().nodes[2].p, Point::new(100., 80.));
        e.doc.guides.push(Guide {
            vertical: true,
            position: 113.,
        });
        e.doc.guides.push(Guide {
            vertical: false,
            position: 57.,
        });
        assert_eq!(e.snap_point(Point::new(110., 60.)), Point::new(113., 57.));
        e.doc.snap_grid = false;
        e.doc.snap_guides = false;
        assert_eq!(e.snap_point(Point::new(110., 60.)), Point::new(110., 60.));
        e.doc.snap_grid = true;
        e.set_tool(Tool::Select);
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Move(Point::new(51., 51.)));
        e.drag_to(Point::new(82., 80.), Pos2::ZERO, false);
        e.commit();
        assert_eq!(e.stroke().unwrap().nodes[0].p, Point::new(60., 40.));
        let mut restored: Document =
            serde_json::from_str(&serde_json::to_string(&e.doc).unwrap()).unwrap();
        restored.upgrade().unwrap();
        assert_eq!(restored, e.doc);
        let svg = outline::svg(&e.doc).unwrap();
        assert!(!svg.contains("guide"));
        e.lock_guides = false;
        e.press(Point::new(113., 300.), Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Guide(0))));
        e.drag_to(Point::new(140., 300.), Pos2::ZERO, false);
        assert_eq!(e.doc.guides[0].position, 140.);
    }
    #[test]
    fn closed_path_last_segment_can_split_bend_and_round_all_corners() {
        let mut e = Editor::default();
        e.set_tool(Tool::Rectangle);
        e.press(Point::new(10., 10.), Pos2::ZERO, false, false);
        e.drag_to(Point::new(210., 160.), Pos2::ZERO, false);
        e.commit();
        let s = e.stroke().unwrap().clone();
        let hit = outline::nearest(&s, Point::new(10., 80.)).unwrap();
        assert_eq!(hit.0, 3);
        assert!((0..4).all(|i| super::super::corners::eligible(&s, i)));
        e.corner_all = true;
        let ids = e.corner_targets();
        assert_eq!(ids.len(), 4);
        e.set_corner_radii(&ids, 20.);
        let rounded = super::super::corners::rounded(e.stroke().unwrap());
        assert!(rounded.closed);
        assert!(!outline::paint(e.stroke().unwrap())
            .unwrap()
            .indices
            .is_empty());
        let mut split = s.clone();
        split.split(3, 0.4, 999);
        for j in 0..=100 {
            let t = j as f64 / 100.;
            let p = if t < 0.4 {
                split.eval(3, t / 0.4)
            } else {
                split.eval(4, (t - 0.4) / 0.6)
            };
            assert!(p.distance(s.eval(3, t)) < 1e-7);
        }
        e.doc.strokes[0] = s;
        e.selected = Some(e.doc.strokes[0].id);
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Segment(3, 0.5, Point::new(10., 80.)));
        e.drag_to(Point::new(-20., 80.), Pos2::ZERO, false);
        assert!(e.stroke().unwrap().nodes[0].incoming.hypot() > 0.);
    }
    #[test]
    fn m_and_l_only_switch_tools_and_pen_click_closes_path() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        for (key, tool) in [
            (egui::Key::M, Tool::Rectangle),
            (egui::Key::L, Tool::Ellipse),
        ] {
            frame(
                &ctx,
                &mut e,
                0.5,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                egui::Modifiers::NONE,
            );
            assert!(e.tool == tool);
            assert_eq!(e.doc, before);
        }
        let mut e = Editor::default();
        for p in [
            Point::new(0., 0.),
            Point::new(100., 0.),
            Point::new(100., 100.),
            Point::new(0., 0.),
        ] {
            e.press(p, Pos2::ZERO, false, false);
            e.commit();
        }
        assert_eq!(e.stroke().unwrap().nodes.len(), 3);
        assert!(e.stroke().unwrap().closed);
        assert!(!e.drawing);
    }
    #[test]
    fn click_corner_does_not_follow_next_pen_handle() {
        let mut e = Editor::default();
        let first = Point::new(50., 240.);
        let second = Point::new(280., 120.);
        e.press(first, Pos2::ZERO, false, false);
        e.commit();
        let original = e.stroke().unwrap().nodes[0].clone();
        e.press(second, Pos2::ZERO, false, false);
        let baseline = outline::build(e.stroke().unwrap()).unwrap().ring[0];
        let expected = (second - first).normalize();
        for v in [
            Vec2::new(160., 30.),
            Vec2::new(-90., 180.),
            Vec2::new(15., -210.),
            Vec2::ZERO,
            Vec2::new(0.001, 0.),
        ] {
            e.drag_to(second + v, Pos2::ZERO, false);
            let s = e.stroke().unwrap();
            assert_eq!(s.nodes[0], original);
            assert!((s.tangent(0, 0.) - expected).hypot() < 1e-6);
            assert!(outline::build(s).unwrap().ring[0].distance(baseline) < 1e-5);
            assert_eq!(s.nodes[0].outgoing, Vec2::ZERO);
            if v.hypot() > 0.1 {
                assert!((s.tangent(0, 1.) - v.normalize()).hypot() < 1e-6);
            }
        }
        e.commit();
        let read: Document = serde_json::from_str(&serde_json::to_string(&e.doc).unwrap()).unwrap();
        assert!((read.strokes[0].tangent(0, 0.) - expected).hypot() < 1e-6);
    }
    #[test]
    fn first_pen_drag_renders_both_handles_and_controls_next_curve() {
        fn has_circle(shapes: &[egui::epaint::ClippedShape], p: Pos2) -> bool {
            fn contains(shape: &egui::Shape, p: Pos2) -> bool {
                match shape {
                    egui::Shape::Circle(c) => c.center.distance(p) < 0.1,
                    egui::Shape::Vec(v) => v.iter().any(|s| contains(s, p)),
                    _ => false,
                }
            }
            shapes.iter().any(|s| contains(&s.shape, p))
        }
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        let m = egui::Modifiers::NONE;
        frame(&ctx, &mut e, 0., vec![], m);
        frame(&ctx, &mut e, 0.02, vec![], m);
        let p = e.canvas_rect.center();
        let h = p + EVec2::new(75., -100.);
        frame(
            &ctx,
            &mut e,
            0.1,
            vec![
                egui::Event::PointerMoved(p),
                button(p, true, egui::PointerButton::Primary, m),
            ],
            m,
        );
        // Holding without motion must not place another anchor or start a timer action.
        frame(&ctx, &mut e, 0.8, vec![], m);
        let output = frame(&ctx, &mut e, 1., vec![egui::Event::PointerMoved(h)], m);
        assert_eq!(e.stroke().unwrap().nodes.len(), 1);
        assert!(has_circle(&output.shapes, h));
        assert!(has_circle(&output.shapes, p - (h - p)));
        let shorter = p + EVec2::new(30., -40.);
        frame(
            &ctx,
            &mut e,
            1.1,
            vec![egui::Event::PointerMoved(shorter)],
            m,
        );
        let first = e.stroke().unwrap().nodes[0].clone();
        assert!((first.outgoing - Vec2::new(30., -40.)).hypot() < 1e-6);
        let output = frame(
            &ctx,
            &mut e,
            1.2,
            vec![button(shorter, false, egui::PointerButton::Primary, m)],
            m,
        );
        assert!(has_circle(&output.shapes, shorter));
        let next = p + EVec2::new(220., 0.);
        frame(
            &ctx,
            &mut e,
            1.5,
            vec![
                egui::Event::PointerMoved(next),
                button(next, true, egui::PointerButton::Primary, m),
            ],
            m,
        );
        frame(
            &ctx,
            &mut e,
            1.6,
            vec![button(next, false, egui::PointerButton::Primary, m)],
            m,
        );
        let s = e.stroke().unwrap();
        assert_eq!(s.nodes.len(), 2);
        assert_eq!(s.nodes[0], first);
        assert!((s.eval(0, 0.5).y - (first.p.y - 15.)).abs() < 1e-6);
        e.undo();
        assert_eq!(e.stroke().unwrap().nodes.len(), 1);
        e.undo();
        assert!(e.doc.strokes.is_empty());
    }
    #[test]
    fn startup_is_empty_and_pen_ready() {
        let e = Editor::default();
        assert!(e.doc.strokes.is_empty());
        assert!(e.selected.is_none());
        assert!(e.tool == Tool::Pen);
        assert!(!e.dirty());
    }
    #[test]
    fn pen_adds_and_removes_interior_anchors_without_new_paths() {
        let mut e = example_editor();
        e.set_tool(Tool::Pen);
        let old = e.stroke().unwrap().clone();
        let p = old.eval(0, 0.7);
        assert!(matches!(e.pen_edit_target(p), Some((_, _, _, false))));
        e.press(p, Pos2::ZERO, false, false);
        assert_eq!(e.doc.strokes.len(), 1);
        assert_eq!(e.stroke().unwrap().nodes.len(), 4);
        assert!(matches!(e.pen_edit_target(p), Some((_, 1, _, true))));
        e.press(p, Pos2::ZERO, false, false);
        assert_eq!(e.stroke().unwrap().nodes.len(), 3);
        e.doc.validate().unwrap();
    }
    #[test]
    fn direct_selection_ignores_fill_and_drags_center_curve() {
        let mut e = example_editor();
        e.cache();
        let s = e.stroke().unwrap();
        let p = s.eval(0, 0.7);
        let fill = p + outline::normal(s.tangent(0, 0.7)) * 20.;
        assert!(e.hit_shape(fill).is_some());
        let id = e.selected;
        e.press(fill, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Marquee(..))));
        e.selected = id;
        e.transaction = Some(e.doc.clone());
        e.bend_segments = true;
        e.press(p, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Segment(..))));
        e.drag_with_modifiers(p + Vec2::new(0., 20.), Pos2::ZERO, false, false);
        assert!(
            e.stroke()
                .unwrap()
                .eval(0, 0.7)
                .distance(p + Vec2::new(0., 20.))
                < 0.1
        );
    }
    #[test]
    fn marquee_selects_across_paths_moves_only_selected_nodes_and_undoes_once() {
        let mut e = example_editor();
        let mut second = e.doc.strokes[0].clone();
        second.id = e.doc.id();
        second.widths.clear();
        for n in &mut second.nodes {
            n.id = e.doc.id();
            n.p.y += 40.;
        }
        e.doc.strokes.push(second);
        let before = e.doc.clone();
        e.transaction = Some(before.clone());
        e.press(Point::new(100., 450.), Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Marquee(..))));
        e.drag_to(Point::new(500., 250.), Pos2::ZERO, false);
        assert_eq!(e.selected_nodes.len(), 4);
        e.commit();
        assert_eq!(e.doc, before);
        assert!(e.undo.is_empty());
        let p = e.doc.strokes[0].nodes[0].p;
        e.transaction = Some(before.clone());
        e.press(p, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Nodes(..))));
        let delta = Vec2::new(30., -20.);
        e.drag_to(p + delta, Pos2::ZERO, false);
        e.drag_to(p + delta, Pos2::ZERO, false);
        e.commit();
        for (s, old) in e.doc.strokes.iter().zip(&before.strokes) {
            for (i, (n, base)) in s.nodes.iter().zip(&old.nodes).enumerate() {
                assert_eq!(n.p, base.p + if i < 2 { delta } else { Vec2::ZERO });
                assert_eq!(n.incoming, base.incoming);
                assert_eq!(n.outgoing, base.outgoing);
            }
        }
        assert_eq!(e.undo.len(), 1);
        e.doc.validate().unwrap();
        e.undo();
        assert_eq!(e.doc, before);
    }
    #[test]
    fn always_visible_width_grips_work_without_ctrl() {
        for tool in [Tool::Direct, Tool::Pen] {
            let mut e = example_editor();
            e.tool = tool;
            let nodes = e.stroke().unwrap().nodes.clone();
            e.show_widths = true;
            let target = WidthTarget::Key(e.stroke().unwrap().widths[0].id);
            let (c, n) = outline::width_location(e.stroke().unwrap(), target).unwrap();
            let w = e.stroke().unwrap().width(target).unwrap();
            let grip = c + n * (w.left + 10.);
            e.press(grip, Pos2::ZERO, false, false);
            assert!(matches!(e.drag, Some(Drag::Width(..))));
            e.drag_to(grip + n * 20., Pos2::ZERO, false);
            assert!((e.stroke().unwrap().width(target).unwrap().left - w.left - 20.).abs() < 1e-8);
            assert_eq!(e.stroke().unwrap().nodes, nodes);
        }
    }
    #[test]
    fn selection_bounds_follow_move_and_marquee_is_drawn() {
        let (ctx, mut e) = setup();
        e.set_tool(Tool::Select);
        let out = frame(&ctx, &mut e, 0.1, vec![], egui::Modifiers::NONE);
        let has_box = |out: &egui::FullOutput| {
            out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.stroke.color == ACCENT && r.rect.width() > 100. && r.rect.height() > 100.))
        };
        assert!(has_box(&out));
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Move(Point::ZERO));
        e.drag_to(Point::new(30., 15.), Pos2::ZERO, false);
        assert!(has_box(&frame(
            &ctx,
            &mut e,
            0.2,
            vec![],
            egui::Modifiers::NONE
        )));
        e.set_tool(Tool::Direct);
        e.drag = Some(Drag::Marquee(Point::ZERO, Point::new(200., 200.)));
        let out = frame(&ctx, &mut e, 0.3, vec![], egui::Modifiers::NONE);
        assert!(out.shapes.iter().any(
            |s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == Color32::from_white_alpha(12))
        ));
    }
    #[test]
    fn bounding_box_scaling_rotation_width_policy_and_undo() {
        for scale_widths in [false, true] {
            let mut e = example_editor();
            e.set_tool(Tool::Select);
            e.scale_widths = scale_widths;
            e.cache();
            let before = e.doc.clone();
            let r = e.selection_bounds().unwrap();
            let h = Point::new(r.center().x, r.y1);
            e.transaction = Some(before.clone());
            e.press(h, Pos2::ZERO, false, false);
            assert!(matches!(e.drag,Some(Drag::Scale {axes,..}) if axes.x==0.));
            e.drag_with_modifiers(
                Point::new(h.x, r.y0 + r.height() * 0.5),
                Pos2::ZERO,
                false,
                false,
            );
            let after = e.stroke().unwrap();
            let old = &before.strokes[0];
            for (n, b) in after.nodes.iter().zip(&old.nodes) {
                assert!((n.p.x - b.p.x).abs() < 1e-8);
                assert!((n.p.y - (r.y0 + (b.p.y - r.y0) * 0.5)).abs() < 1e-8);
            }
            if !scale_widths {
                assert_eq!(after.start_width, old.start_width);
            } else {
                assert!(after.start_width.left < old.start_width.left);
            }
            e.commit();
            e.doc.validate().unwrap();
            e.undo();
            assert_eq!(e.doc, before);
        }
        let mut e = example_editor();
        e.set_tool(Tool::Select);
        e.cache();
        let original = e.doc.clone();
        let r = e.selection_bounds().unwrap();
        let c = r.center();
        let start = Point::new(c.x, r.y0 - 28.);
        e.transaction = Some(original.clone());
        e.press(start, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Rotate { .. })));
        let v = start - c;
        e.drag_with_modifiers(c + Vec2::new(-v.y, v.x), Pos2::ZERO, false, true);
        for (n, b) in e
            .stroke()
            .unwrap()
            .nodes
            .iter()
            .zip(&original.strokes[0].nodes)
        {
            let v = b.p - c;
            assert!(n.p.distance(c + Vec2::new(-v.y, v.x)) < 1e-6);
        }
        assert!((e.stroke().unwrap().start_angle - 90.).abs() < 1e-6);
        e.cancel_drag();
        assert_eq!(e.doc, original);
    }
    #[test]
    fn shift_click_toggles_vertices_and_live_corner_scope() {
        let mut e = example_editor();
        for n in &mut e.doc.strokes[0].nodes {
            n.incoming = Vec2::ZERO;
            n.outgoing = Vec2::ZERO;
        }
        let a = e.doc.strokes[0].nodes[0].p;
        let b = e.doc.strokes[0].nodes[1].p;
        e.press(a, Pos2::ZERO, false, false);
        e.commit();
        e.press_with_shift(b, Pos2::ZERO, false, false, true, false);
        assert_eq!(e.selected_nodes.len(), 2);
        e.press_with_shift(a, Pos2::ZERO, false, false, true, false);
        assert_eq!(e.selected_nodes, vec![e.doc.strokes[0].nodes[1].id]);
        let targets = e.corner_targets();
        assert_eq!(targets.len(), 1);
        let before = e.doc.clone();
        e.transaction = Some(before.clone());
        e.set_corners(&targets, 30., CornerKind::Clothoid);
        e.commit();
        e.doc.validate().unwrap();
        assert_eq!(e.doc.strokes[0].nodes, before.strokes[0].nodes);
        assert_ne!(
            outline::svg(&e.doc).unwrap(),
            outline::svg(&before).unwrap()
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.selected_nodes.clear();
        e.node_selected = None;
        e.corner_all = true;
        assert_eq!(e.corner_targets(), targets);
    }
    #[test]
    fn switching_to_pen_does_not_place_a_point_even_with_same_frame_pointer_press() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let p = e.canvas_rect.center();
        frame(
            &ctx,
            &mut e,
            0.1,
            vec![
                egui::Event::Key {
                    key: egui::Key::P,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerMoved(p),
                button(p, true, egui::PointerButton::Primary, egui::Modifiers::NONE),
            ],
            egui::Modifiers::NONE,
        );
        assert!(e.tool == Tool::Pen);
        assert_eq!(e.doc, before);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![button(
                p,
                false,
                egui::PointerButton::Primary,
                egui::Modifiers::NONE,
            )],
            egui::Modifiers::NONE,
        );
        assert_eq!(e.doc, before);
        assert!(e.show_widths);
    }
    #[test]
    fn centerline_only_bends_with_alt_at_drag_start() {
        let mut e = example_editor();
        let before = e.doc.clone();
        let p = e.stroke().unwrap().eval(0, 0.7);
        e.press_with_shift(p, Pos2::ZERO, false, false, false, false);
        assert!(e.drag.is_none());
        e.drag_to(p + Vec2::new(0., 30.), Pos2::ZERO, false);
        assert_eq!(e.doc, before);
        e.transaction = Some(before.clone());
        e.press_with_shift(p, Pos2::ZERO, false, false, false, true);
        assert!(matches!(e.drag, Some(Drag::Segment(..))));
        e.drag_to(p + Vec2::new(0., 30.), Pos2::ZERO, true);
        assert_ne!(e.doc, before);
        e.cancel_drag();
        assert_eq!(e.doc, before);
    }
    #[test]
    fn rotated_bounds_remain_rotated_and_scale_on_their_own_axes() {
        let mut e = example_editor();
        e.set_tool(Tool::Select);
        e.cache();
        let center = e.selection_bounds().unwrap().center();
        let angle = std::f64::consts::FRAC_PI_4;
        let matrix = kurbo::Affine::translate(center.to_vec2())
            * kurbo::Affine::rotate(angle)
            * kurbo::Affine::translate(-center.to_vec2());
        e.doc.strokes[0] = e.doc.strokes[0].transformed(matrix, true);
        e.cache();
        assert!((e.stroke().unwrap().frame_angle - angle).abs() < 1e-8);
        let r = e.selection_bounds().unwrap();
        let rotate = kurbo::Affine::rotate(angle);
        let h = Point::new(r.x1, r.center().y);
        let before = e.doc.clone();
        e.transaction = Some(before.clone());
        e.press(rotate * h, Pos2::ZERO, false, false);
        assert!(matches!(e.drag,Some(Drag::Scale {rotation,..}) if (rotation-angle).abs()<1e-8));
        e.drag_to(
            rotate * Point::new(r.x0 + r.width() * 1.5, h.y),
            Pos2::ZERO,
            false,
        );
        for (n, b) in e
            .stroke()
            .unwrap()
            .nodes
            .iter()
            .zip(&before.strokes[0].nodes)
        {
            let old = kurbo::Affine::rotate(-angle) * b.p;
            let new = kurbo::Affine::rotate(-angle) * n.p;
            assert!((new.y - old.y).abs() < 1e-6);
            assert!((new.x - (r.x0 + (old.x - r.x0) * 1.5)).abs() < 1e-6);
        }
        assert!((e.stroke().unwrap().frame_angle - angle).abs() < 1e-8);
        e.doc.validate().unwrap();
        let restored: Document =
            serde_json::from_str(&serde_json::to_string(&e.doc).unwrap()).unwrap();
        restored.validate().unwrap();
        assert!((restored.strokes[0].frame_angle - angle).abs() < 1e-8);
        for (a, b) in restored.strokes[0]
            .nodes
            .iter()
            .zip(&e.doc.strokes[0].nodes)
        {
            assert!(a.p.distance(b.p) < 1e-9);
        }
    }
    #[test]
    fn corner_widgets_on_multiple_paths_share_radius_without_changing_styles() {
        let mut e = example_editor();
        for n in &mut e.doc.strokes[0].nodes {
            n.incoming = Vec2::ZERO;
            n.outgoing = Vec2::ZERO;
        }
        let mut second = e.doc.strokes[0].clone();
        second.id = e.doc.id();
        second.widths.clear();
        for n in &mut second.nodes {
            n.id = e.doc.id();
            n.p.y += 130.;
        }
        let ids = vec![e.doc.strokes[0].nodes[1].id, second.nodes[1].id];
        e.doc.strokes.push(second);
        e.selected_nodes = ids.clone();
        e.set_corners(&[ids[0]], 10., CornerKind::Circular);
        e.set_corners(&[ids[1]], 20., CornerKind::Clothoid);
        let before = e.doc.clone();
        let (h, dir, factor) = crate::corners::widget(&e.doc.strokes[1], 1, e.zoom as f64).unwrap();
        e.transaction = Some(before.clone());
        e.press(h, Pos2::ZERO, false, false);
        assert!(matches!(e.drag,Some(Drag::Corner {node,..}) if node==ids[1]));
        assert_eq!(e.doc, before);
        e.drag_to(h + dir * factor * 5., Pos2::ZERO, false);
        e.commit();
        for s in &e.doc.strokes {
            assert!((s.corners[0].radius - 25.).abs() < 1e-6);
        }
        assert_eq!(e.doc.strokes[0].corners[0].kind, CornerKind::Circular);
        assert_eq!(e.doc.strokes[1].corners[0].kind, CornerKind::Clothoid);
        assert_eq!(e.undo.len(), 1);
        e.undo();
        assert_eq!(e.doc, before);
        e.set_corner_radii(&ids, 10000.);
        assert_eq!(
            e.doc.strokes[0].corners[0].radius,
            e.doc.strokes[1].corners[0].radius
        );
        for s in &e.doc.strokes {
            assert!(
                s.corners[0].radius <= crate::corners::radius_limit(s, 1, s.corners[0].kind) + 1e-6
            );
        }
    }
    #[test]
    fn only_selected_anchor_and_handle_are_orange_and_widget_matches_hit_position() {
        let (ctx, mut e) = setup();
        e.node_selected = Some(1);
        let n = e.doc.strokes[0].nodes[1].clone();
        e.handle_selected = Some((n.id, true));
        let output = frame(&ctx, &mut e, 0.1, vec![], egui::Modifiers::NONE);
        for (i, n) in e.doc.strokes[0].nodes.iter().enumerate() {
            let p = e.screen(n.p, e.canvas_rect);
            let color = if i == 1 { ACCENT } else { GUIDE };
            assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.rect.center().distance(p)<0.1 && r.stroke.color==color)));
        }
        let h = e.screen(n.p + n.outgoing, e.canvas_rect);
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c) if c.center.distance(h)<0.1 && c.stroke.color==ACCENT)));
        for n in &mut e.doc.strokes[0].nodes {
            n.incoming = Vec2::ZERO;
            n.outgoing = Vec2::ZERO;
        }
        let (world, _, _) = crate::corners::widget(&e.doc.strokes[0], 1, e.zoom as f64).unwrap();
        let h = e.screen(world, e.canvas_rect);
        let output = frame(&ctx, &mut e, 0.2, vec![], egui::Modifiers::NONE);
        assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Circle(c) if c.center.distance(h)<0.1 && c.radius==4.)));
        e.press(world, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Corner { .. })));
    }
    #[test]
    fn wheel_zooms_at_pointer_with_or_without_ctrl() {
        for modifiers in [egui::Modifiers::NONE, ctrl()] {
            let (ctx, mut e) = setup();
            let p = e.canvas_rect.center();
            frame(
                &ctx,
                &mut e,
                0.1,
                vec![egui::Event::PointerMoved(p)],
                modifiers,
            );
            let before = e.world(p, e.canvas_rect);
            frame(
                &ctx,
                &mut e,
                0.2,
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: EVec2::new(0., 1.),
                    modifiers,
                }],
                modifiers,
            );
            assert!(e.zoom > 1.);
            assert!(before.distance(e.world(p, e.canvas_rect)) < 0.001);
        }
    }
    #[test]
    fn cut_world_angle_and_geometry_survive_endpoint_edits() {
        for angle in [0., 15., 90., 123.4] {
            let mut e = example_editor();
            e.stroke_mut().unwrap().start_angle = angle;
            e.stroke_mut().unwrap().start_cap_locked = true;
            for drag in [Drag::Node(0), Drag::Handle(0, true)] {
                e.drag = Some(drag);
                e.drag_with_modifiers(Point::new(210., 190.), Pos2::ZERO, false, false);
                let s = e.stroke().unwrap();
                assert!((outline::cap_angle(s, false) - angle).abs() < 1e-8);
                let o = outline::build(s).unwrap();
                let d = outline::cap_direction(s, false);
                for p in [o.ring[0], *o.ring.last().unwrap()] {
                    let v = p - s.nodes[0].p;
                    assert!((v.x * d.y - v.y * d.x).abs() < 0.001);
                }
            }
        }
    }
    #[test]
    fn pen_resumes_both_ends_and_undo_preserves_existing_curve() {
        for prepend in [false, true] {
            let mut e = example_editor();
            let before = e.doc.clone();
            let old = e.stroke().unwrap().clone();
            let endpoint = if prepend {
                old.nodes[0].p
            } else {
                old.nodes.last().unwrap().p
            };
            e.set_tool(Tool::Pen);
            e.press(endpoint, Pos2::ZERO, false, false);
            assert_eq!(e.doc, before);
            assert!(e.drawing);
            e.transaction = Some(before.clone());
            e.press(endpoint + Vec2::new(80., 70.), Pos2::ZERO, false, false);
            e.commit();
            assert_eq!(e.doc.strokes.len(), 1);
            let s = e.stroke().unwrap();
            assert_eq!(s.nodes.len(), old.nodes.len() + 1);
            for i in 0..old.nodes.len() - 1 {
                for t in [0., 0.2, 0.7, 1.] {
                    assert!(s.eval(i + usize::from(prepend), t).distance(old.eval(i, t)) < 1e-8);
                }
            }
            for k in &old.widths {
                assert!(s.widths.contains(k));
            }
            assert_eq!(s.start_angle, old.start_angle);
            assert_eq!(s.end_angle, old.end_angle);
            e.doc.validate().unwrap();
            e.undo();
            assert_eq!(e.doc, before);
        }
    }
    #[test]
    fn selection_and_direct_modes_never_draw_on_empty_canvas() {
        let mut e = example_editor();
        let before = e.doc.clone();
        for tool in [Tool::Select, Tool::Direct] {
            e.set_tool(tool);
            e.press(Point::new(-500., -500.), Pos2::ZERO, false, false);
            assert_eq!(e.doc, before);
        }
        e.selected = Some(before.strokes[0].id);
        e.cache();
        e.set_tool(Tool::Select);
        e.press(before.strokes[0].nodes[1].p, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Move(_))));
    }
    #[test]
    fn rotation_is_free_until_shift_and_uses_saved_increment() {
        let mut e = example_editor();
        e.doc.angle_step = 10.;
        let c = e.stroke().unwrap().nodes[0].p;
        let a = 23.4_f64.to_radians();
        e.press(c, Pos2::ZERO, true, false);
        e.drag_with_modifiers(
            c + Vec2::new(a.cos(), a.sin()) * 80.,
            Pos2::ZERO,
            false,
            false,
        );
        assert!((e.stroke().unwrap().start_angle - 23.4).abs() < 1e-8);
        e.drag_with_modifiers(
            c + Vec2::new(a.cos(), a.sin()) * 80.,
            Pos2::ZERO,
            false,
            true,
        );
        assert!((e.stroke().unwrap().start_angle - 20.).abs() < 1e-8);
        let json = serde_json::to_string(&e.doc).unwrap();
        let loaded: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.angle_step, 10.);
    }
    #[test]
    fn alt_width_is_temporary_and_normal_drag_preserves_asymmetry() {
        let mut e = example_editor();
        let target = WidthTarget::Start;
        let (c, n) = outline::width_location(e.stroke().unwrap(), target).unwrap();
        let before = e.stroke().unwrap().start_width;
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Width(target, true));
        e.drag_to(c + n * (before.left + 19.), Pos2::ZERO, true);
        e.commit();
        assert_eq!(e.stroke().unwrap().start_width.right, before.right);
        e.transaction = Some(e.doc.clone());
        e.drag = Some(Drag::Width(target, true));
        e.drag_to(c + n * (before.left + 25.), Pos2::ZERO, false);
        let w = e.stroke().unwrap().start_width;
        assert!((w.right - before.right - 6.).abs() < 1e-8);
        assert!((w.left - w.right - 9.).abs() < 1e-8);
    }
    #[test]
    fn all_handles_are_editable_without_changing_tool() {
        let mut e = example_editor();
        let s = e.stroke().unwrap();
        let p = s.nodes[1].p;
        let h = s.nodes[1].p + s.nodes[1].outgoing;
        let target = WidthTarget::Key(s.widths[0].id);
        let (c, n) = outline::width_location(s, target).unwrap();
        let w = s.width(target).unwrap().left;
        e.press(p, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Node(1))));
        e.press(h, Pos2::ZERO, false, false);
        assert!(matches!(e.drag, Some(Drag::Handle(1, true))));
        e.press(c + n * (w + 10.), Pos2::ZERO, true, false);
        assert!(matches!(e.drag, Some(Drag::Width(..))));
        assert!(e.tool == Tool::Direct);
    }
    #[test]
    fn ctrl_width_grip_resizes_without_rotating() {
        let mut e = example_editor();
        let s = e.stroke().unwrap();
        let target = WidthTarget::Key(s.widths[0].id);
        let (c, n) = outline::width_location(s, target).unwrap();
        let w = s.width(target).unwrap();
        e.transaction = Some(e.doc.clone());
        e.press(c + n * (w.left + 10.), Pos2::ZERO, true, false);
        assert!(matches!(e.drag, Some(Drag::Width(..))));
        e.drag_with_modifiers(c + n * (w.left + 30.), Pos2::ZERO, false, false);
        let after = e.stroke().unwrap().width(target).unwrap();
        assert!((after.left - w.left - 20.).abs() < 1e-8);
        assert_eq!(after.angle, w.angle);
    }
    #[test]
    fn world_caps_survive_anchor_and_tangent_edits() {
        let mut e = example_editor();
        e.stroke_mut().unwrap().start_angle = 90.;
        e.stroke_mut().unwrap().start_cap_locked = true;
        e.drag = Some(Drag::Handle(0, true));
        let p = e.stroke().unwrap().nodes[0].p + Vec2::new(-70., 180.);
        e.drag_to(p, Pos2::ZERO, false);
        e.cache();
        assert!((outline::cap_angle(e.stroke().unwrap(), false) - 90.).abs() < 1e-8);
        e.drag = Some(Drag::Node(0));
        e.drag_to(Point::new(50., 400.), Pos2::ZERO, false);
        e.cache();
        assert!((outline::cap_angle(e.stroke().unwrap(), false) - 90.).abs() < 1e-8);
        assert!(outline::build(e.stroke().unwrap()).is_ok());
    }
    #[test]
    fn editing_ordinary_endpoints_does_not_enable_custom_angles() {
        let mut e = example_editor();
        e.stroke_mut().unwrap().start_cap_locked = false;
        e.stroke_mut().unwrap().end_cap_locked = false;
        for drag in [
            Drag::Node(0),
            Drag::Handle(0, true),
            Drag::Segment(0, 0.5, Point::ZERO),
        ] {
            e.drag = Some(drag);
            e.drag_to(Point::new(150., 240.), Pos2::ZERO, false);
            let s = e.stroke().unwrap();
            assert!(!s.start_cap_locked && !s.end_cap_locked);
            assert!(outline::cap_direction(s, false).dot(s.tangent(0, 0.)).abs() < 1e-8);
        }
        e.drag = Some(Drag::Cap(false));
        let p = e.stroke().unwrap().nodes[0].p;
        e.drag_to(p + Vec2::new(60., 0.), Pos2::ZERO, false);
        assert!(e.stroke().unwrap().start_cap_locked);
        assert!(!e.stroke().unwrap().end_cap_locked);
    }
    #[test]
    fn horizontal_and_vertical_caps_available_after_edit() {
        let mut e = example_editor();
        e.stroke_mut().unwrap().nodes[0].outgoing = Vec2::new(160., 50.);
        let c = e.stroke().unwrap().nodes[0].p;
        for (v, angle) in [(Vec2::new(60., 0.), 0.), (Vec2::new(0., 60.), 90.)] {
            e.press(c, Pos2::ZERO, true, false);
            e.drag_to(c + v, Pos2::ZERO, false);
            e.cache();
            assert!((outline::cap_angle(e.stroke().unwrap(), false) - angle).abs() < 1e-8);
        }
    }
    #[test]
    fn ctrl_click_path_adds_width_without_mode_switch() {
        let mut e = example_editor();
        let p = e.stroke().unwrap().eval(0, 0.75);
        e.press(p, Pos2::ZERO, true, false);
        assert_eq!(e.stroke().unwrap().widths.len(), 3);
        assert!(e.tool == Tool::Direct);
    }
    #[test]
    fn middle_button_pans_canvas_without_edit() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let pan = e.pan;
        let p = e.canvas_rect.center();
        let q = p + EVec2::new(40., 30.);
        let m = egui::Modifiers::NONE;
        frame(&ctx, &mut e, 0.1, vec![egui::Event::PointerMoved(p)], m);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![button(p, true, egui::PointerButton::Middle, m)],
            m,
        );
        frame(&ctx, &mut e, 0.3, vec![egui::Event::PointerMoved(q)], m);
        frame(
            &ctx,
            &mut e,
            0.4,
            vec![button(q, false, egui::PointerButton::Middle, m)],
            m,
        );
        assert_eq!(e.pan, pan + EVec2::new(40., 30.));
        assert_eq!(e.doc, before);
        assert!(e.undo.is_empty());
    }
    #[test]
    fn pointer_width_drag_is_one_undo() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let s = e.stroke().unwrap();
        let target = WidthTarget::Key(s.widths[0].id);
        let (c, n) = outline::width_location(s, target).unwrap();
        let w = s.width(target).unwrap().left;
        let p = e.screen(c + n * (w + 10.), e.canvas_rect);
        let q = e.screen(c + n * (w + 30.), e.canvas_rect);
        e.width_selected = Some(target);
        let m = egui::Modifiers::NONE;
        frame(&ctx, &mut e, 0.1, vec![egui::Event::PointerMoved(p)], m);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![button(p, true, egui::PointerButton::Primary, m)],
            m,
        );
        assert!(matches!(e.drag, Some(Drag::Width(..))));
        frame(&ctx, &mut e, 0.3, vec![egui::Event::PointerMoved(q)], m);
        frame(
            &ctx,
            &mut e,
            0.4,
            vec![button(q, false, egui::PointerButton::Primary, m)],
            m,
        );
        assert!(e.stroke().unwrap().width(target).unwrap().left > w + 15.);
        assert_eq!(e.undo.len(), 1);
        frame(
            &ctx,
            &mut e,
            0.5,
            vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: ctrl(),
            }],
            ctrl(),
        );
        assert_eq!(e.doc, before, "keyboard undo after a canvas drag");
    }
    #[test]
    fn ctrl_endpoint_pointer_rotation_preserves_nodes() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let c = e.stroke().unwrap().nodes[0].p;
        let p = e.screen(c, e.canvas_rect);
        let q = e.screen(c + Vec2::new(0., 65.), e.canvas_rect);
        let m = ctrl();
        frame(&ctx, &mut e, 0.1, vec![egui::Event::PointerMoved(p)], m);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![button(p, true, egui::PointerButton::Primary, m)],
            m,
        );
        assert_eq!(e.doc, before);
        assert!(matches!(e.drag, Some(Drag::Cap(false))));
        frame(
            &ctx,
            &mut e,
            0.3,
            vec![egui::Event::PointerMoved(q)],
            egui::Modifiers::NONE,
        );
        frame(
            &ctx,
            &mut e,
            0.4,
            vec![button(
                q,
                false,
                egui::PointerButton::Primary,
                egui::Modifiers::NONE,
            )],
            egui::Modifiers::NONE,
        );
        assert_eq!(e.stroke().unwrap().start_angle, 90.);
        assert_eq!(e.stroke().unwrap().nodes, before.strokes[0].nodes);
        assert_eq!(e.undo.len(), 1);
    }
    #[test]
    fn ctrl_rotation_grip_enables_only_dragged_end_in_all_tools() {
        for tool in [Tool::Pen, Tool::Select, Tool::Direct] {
            for end in [false, true] {
                let mut e = example_editor();
                e.tool = tool;
                let s = e.stroke_mut().unwrap();
                s.start_cap_locked = false;
                s.end_cap_locked = false;
                let nodes = s.nodes.clone();
                let c = if end {
                    s.nodes.last().unwrap().p
                } else {
                    s.nodes[0].p
                };
                let h = c + outline::cap_direction(s, end) * 42.;
                e.press(h, Pos2::ZERO, true, false);
                assert!(matches!(e.drag, Some(Drag::Cap(value)) if value == end));
                assert!(
                    !e.stroke().unwrap().start_cap_locked && !e.stroke().unwrap().end_cap_locked
                );
                let angle = 37_f64.to_radians();
                let p = c + Vec2::new(angle.cos(), angle.sin()) * 60.;
                e.drag_with_modifiers(p, Pos2::ZERO, false, true);
                let s = e.stroke().unwrap();
                assert!((outline::cap_angle(s, end) - 30.).abs() < 1e-8);
                assert_eq!(s.start_cap_locked, !end);
                assert_eq!(s.end_cap_locked, end);
                assert_eq!(s.nodes, nodes);
            }
        }
    }
    #[test]
    fn alt_width_drag_only_changes_one_side() {
        let mut e = example_editor();
        let target = WidthTarget::Start;
        let s = e.stroke().unwrap();
        let (c, n) = outline::width_location(s, target).unwrap();
        let right = s.start_width.right;
        e.drag = Some(Drag::Width(target, true));
        e.drag_to(c + n * 70., Pos2::ZERO, true);
        assert_eq!(e.stroke().unwrap().start_width.right, right);
        assert!(e.stroke().unwrap().start_width.linked);
    }
    #[test]
    fn save_roundtrip_keeps_world_caps_and_width_rotation() {
        let mut e = example_editor();
        e.doc.strokes[0].start_angle = 90.;
        e.doc.strokes[0].start_cap_locked = true;
        e.doc.strokes[0].widths[0].width.angle = 30.;
        let path = std::env::temp_dir().join(format!(
            "pathweaver-test-{}-{}.pathweaver",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        e.file = Some(path.clone());
        assert!(e.save(false));
        e.doc.strokes[0].end_angle = 45.;
        assert!(e.save(false));
        let d: Document = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(d, e.doc);
        d.validate().unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cancel_drag_restores_document() {
        let mut e = example_editor();
        let old = e.doc.clone();
        e.transaction = Some(old.clone());
        e.drag = Some(Drag::Node(0));
        e.drag_to(Point::ZERO, Pos2::ZERO, false);
        e.cancel_drag();
        assert_eq!(e.doc, old);
    }
    #[test]
    fn default_layout_uses_original_dock_regions() {
        let (_, e) = setup();
        assert!(e.canvas_rect.width() > 500.);
        assert_eq!(e.dock.iter_all_tabs().count(), 5);
        assert!(e.cache.values().all(|(_, v)| v.is_ok()));
    }
    #[test]
    fn outline_display_hides_fill_only_during_edit_and_does_not_change_export() {
        let (ctx, mut e) = setup();
        let before = outline::svg(&e.doc).unwrap();
        let has_fill = |out: &egui::FullOutput| {
            out.shapes.iter().any(|s|
            matches!(&s.shape, egui::Shape::Mesh(m) if !m.vertices.is_empty() && m.vertices.iter().all(|v| v.color == INK)))
        };
        let render = |e: &mut Editor, time| frame(&ctx, e, time, vec![], egui::Modifiers::NONE);
        assert!(has_fill(&render(&mut e, 0.1)));
        e.outline_view = OutlineView::Always;
        assert!(!has_fill(&render(&mut e, 0.2)));
        e.zoom = 3.;
        let out = render(&mut e, 0.3);
        assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. } if stroke.color == INK && stroke.width == 0.7)));
        e.outline_view = OutlineView::Editing;
        assert!(has_fill(&render(&mut e, 0.4)));
        e.drag = Some(Drag::Node(0));
        assert!(!has_fill(&render(&mut e, 0.5)));
        e.drag = None;
        assert!(has_fill(&render(&mut e, 0.6)));
        assert_eq!(outline::svg(&e.doc).unwrap(), before);
        assert!(!e.dirty());
    }
    #[test]
    fn clicking_a_grip_does_not_change_width_or_dirty_state() {
        let (ctx, mut e) = setup();
        let before = e.doc.clone();
        let s = e.stroke().unwrap();
        let target = WidthTarget::Key(s.widths[0].id);
        let (c, n) = outline::width_location(s, target).unwrap();
        let p = e.screen(c + n * (s.width(target).unwrap().left + 10.), e.canvas_rect)
            + EVec2::new(1., 1.);
        e.width_selected = Some(target);
        let m = egui::Modifiers::NONE;
        frame(&ctx, &mut e, 0.1, vec![egui::Event::PointerMoved(p)], m);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![button(p, true, egui::PointerButton::Primary, m)],
            m,
        );
        frame(
            &ctx,
            &mut e,
            0.3,
            vec![button(p, false, egui::PointerButton::Primary, m)],
            m,
        );
        assert_eq!(e.doc, before);
        assert!(!e.dirty());
        assert!(e.undo.is_empty());
    }
    #[test]
    fn batched_press_and_motion_uses_press_origin() {
        let (ctx, mut e) = setup();
        let s = e.stroke().unwrap();
        let target = WidthTarget::Key(s.widths[0].id);
        let (c, n) = outline::width_location(s, target).unwrap();
        let w = s.width(target).unwrap().left;
        let p = e.screen(c + n * (w + 10.), e.canvas_rect);
        let q = e.screen(c + n * (w + 35.), e.canvas_rect);
        e.width_selected = Some(target);
        let m = egui::Modifiers::NONE;
        frame(&ctx, &mut e, 0.1, vec![egui::Event::PointerMoved(p)], m);
        frame(
            &ctx,
            &mut e,
            0.2,
            vec![
                button(p, true, egui::PointerButton::Primary, m),
                egui::Event::PointerMoved(q),
            ],
            m,
        );
        frame(
            &ctx,
            &mut e,
            0.3,
            vec![button(q, false, egui::PointerButton::Primary, m)],
            m,
        );
        assert!(e.stroke().unwrap().width(target).unwrap().left > w + 20.);
        assert_eq!(e.undo.len(), 1);
    }
    #[test]
    fn legacy_projects_keep_the_visible_cap_direction() {
        let mut d = Document::example();
        d.version = 1;
        d.strokes[0].start_angle = -30.;
        d.strokes[0].end_angle = 30.;
        let t = d.strokes[0].tangent(0, 0.);
        let a = (-30.0_f64).to_radians();
        let expected = outline::normal(t) * a.cos() + t * a.sin();
        let mut value = serde_json::to_value(&d).unwrap();
        value["strokes"][0]
            .as_object_mut()
            .unwrap()
            .remove("start_cap_locked");
        value["strokes"][0]["widths"][0]["width"]
            .as_object_mut()
            .unwrap()
            .remove("angle");
        let mut restored: Document = serde_json::from_value(value).unwrap();
        restored.upgrade().unwrap();
        assert_eq!(restored.version, 3);
        assert!(
            outline::cap_direction(&restored.strokes[0], false)
                .dot(expected)
                .abs()
                > 0.999999
        );
        assert_eq!(restored.strokes[0].widths[0].width.angle, 0.);
    }
}
