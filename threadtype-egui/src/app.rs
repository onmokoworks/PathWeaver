use crate::export::document_to_svg;
use crate::geometry::{
    bevel_ring, clamp_outside_pegs,
    materialize_chain, materialize_face, materialize_face_with_obstacles,
    peg_signed_distance, pegs_penetrated_by_line, segments_penetrate_pegs, signed_area,
    thread_to_wrap_elements, union_polygon_ring_sets, update_thread_ignoring,
    Peg, Seg, Thread,
};
use crate::model::{
    resolve_peg, CustomPeg, Document, Element, Face, GridSettings, PegRef, Port, ScaleAnchor,
    StartAnchor, CURRENT_DOCUMENT_VERSION,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use egui::{
    FontFamily, FontId, TextStyle,
    Color32, Id, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke, Vec2,
};
use kurbo::Point;

const CLOSE_RADIUS: f64 = 16.0;
const START_SNAP: f64 = 12.0;
const START_PROX: f64 = 40.0;
/// 閉じ操作だけで開始円へ大きく巻き付くことを防ぐ上限。
/// これを超える場合は選択した開始点を結び目としてその場で閉じる。
const MAX_AUTO_CLOSE_SWEEP: f64 = std::f64::consts::PI / 2.0;
// 角丸四角は張力経路と形状変更時の再捕捉仕様が未確定のため、製品UIでは休眠。
// 実験コードは残し、円の挙動を固めた後に別ブランチで再開できるようにする。
const ENABLE_EXPERIMENTAL_SQUARE_PEGS: bool = false;
const PEG_CLEARANCE: f64 = 1.0;

fn safe_grid_radius_limits(grid: &GridSettings) -> (f64, f64) {
    let main_max = (grid.spacing * 0.5 - grid.catch - PEG_CLEARANCE).clamp(0.0, 100.0);
    let sub_to_sub = grid.spacing * 0.5 - grid.catch - PEG_CLEARANCE;
    let main_to_sub = grid.spacing / std::f64::consts::SQRT_2
        - grid.radius
        - grid.catch * 2.0
        - PEG_CLEARANCE;
    let sub_max = sub_to_sub.min(main_to_sub).clamp(0.0, 60.0);
    (main_max, sub_max)
}

#[derive(Clone, Copy, Debug)]
struct ViewTransform {
    offset: Vec2,
    zoom: f32,
}

impl ViewTransform {
    fn clamp_zoom(z: f32) -> f32 {
        z.clamp(0.2, 4.0)
    }
}

#[allow(dead_code)] // 未使用トークンはUI移行中の将来用
struct Theme {
    canvas: Color32,
    panel: Color32,
    bg1: Color32,
    bg2: Color32,
    border: Color32,
    text1: Color32,
    text2: Color32,
    text3: Color32,
    accent: Color32,
    accent_bg: Color32,
    warning: Color32,
    error: Color32,
    bg: Color32,
    peg_stroke: Color32,
    peg_fill: Color32,
    catch: Color32,
    face_fill: Color32,
    thread: Color32,
    anchor: Color32,
    close_ok: Color32,
    close_ng: Color32,
    select: Color32,
    start: Color32,
    start_highlight: Color32,
    cmd_green: Color32,
    start_text: Color32,
}

impl Theme {
    fn dark() -> Self {
        Self {
            canvas: Color32::from_rgb(20, 21, 23),
            panel: Color32::from_rgb(30, 30, 30),
            bg1: Color32::from_rgb(37, 37, 37),
            bg2: Color32::from_rgb(45, 45, 45),
            border: Color32::from_rgb(255, 255, 255).gamma_multiply(0.08),
            text1: Color32::from_rgb(245, 245, 247),
            text2: Color32::from_rgb(161, 161, 166),
            text3: Color32::from_rgb(99, 99, 102),
            accent: Color32::from_rgb(10, 132, 255),
            accent_bg: Color32::from_rgb(38, 54, 82),
            warning: Color32::from_rgb(255, 160, 10),
            error: Color32::from_rgb(255, 69, 58),
            bg: Color32::from_rgb(45, 45, 45),
            peg_stroke: Color32::from_rgb(110, 110, 120),
            peg_fill: Color32::from_rgb(40, 40, 44),
            catch: Color32::from_rgb(72, 72, 80),
            face_fill: Color32::from_rgb(61, 61, 64),
            thread: Color32::from_rgb(232, 232, 232),
            anchor: Color32::from_rgb(232, 232, 232),
            close_ok: Color32::from_rgb(10, 132, 255),
            close_ng: Color32::from_rgb(255, 69, 58),
            select: Color32::from_rgb(10, 132, 255),
            start: Color32::from_rgb(160, 160, 184),
            start_highlight: Color32::from_rgb(255, 255, 255),
            cmd_green: Color32::from_rgb(255, 159, 10),
            start_text: Color32::from_rgb(245, 245, 247),
        }
    }

    fn light() -> Self {
        Self {
            canvas: Color32::from_rgb(232, 232, 232),
            panel: Color32::from_rgb(255, 255, 255),
            bg1: Color32::from_rgb(247, 247, 247),
            bg2: Color32::from_rgb(239, 239, 239),
            border: Color32::from_rgb(0, 0, 0).gamma_multiply(0.071),
            text1: Color32::from_rgb(29, 29, 31),
            text2: Color32::from_rgb(110, 110, 115),
            text3: Color32::from_rgb(174, 174, 178),
            accent: Color32::from_rgb(0, 122, 255),
            accent_bg: Color32::from_rgb(219, 234, 254),
            warning: Color32::from_rgb(255, 159, 10),
            error: Color32::from_rgb(255, 59, 48),
            bg: Color32::from_rgb(239, 239, 239),
            peg_stroke: Color32::from_rgb(130, 130, 142),
            peg_fill: Color32::from_rgb(224, 224, 228),
            catch: Color32::from_rgb(190, 190, 202),
            face_fill: Color32::from_rgb(212, 212, 216),
            thread: Color32::from_rgb(22, 22, 22),
            anchor: Color32::from_rgb(22, 22, 22),
            close_ok: Color32::from_rgb(0, 122, 255),
            close_ng: Color32::from_rgb(255, 59, 48),
            select: Color32::from_rgb(0, 122, 255),
            start: Color32::from_rgb(102, 102, 124),
            start_highlight: Color32::from_rgb(24, 24, 24),
            cmd_green: Color32::from_rgb(255, 159, 10),
            start_text: Color32::from_rgb(29, 29, 31),
        }
    }
}

impl Theme {
    fn apply_to_context(&self, ctx: &egui::Context, dark: bool) {
        let mut visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.widgets.inactive.bg_fill = self.bg1;
        visuals.widgets.active.bg_fill = self.accent_bg;
        visuals.widgets.hovered.bg_fill = self.bg2;
        visuals.widgets.noninteractive.bg_fill = self.bg2;
        visuals.widgets.inactive.fg_stroke.color = self.text1;
        visuals.widgets.active.fg_stroke.color = self.text1;
        visuals.widgets.hovered.fg_stroke.color = self.text1;
        visuals.widgets.noninteractive.fg_stroke.color = self.text2;
        visuals.selection.bg_fill = self.accent_bg;
        visuals.selection.stroke.color = self.select;
        visuals.widgets.inactive.rounding = 2.0.into();
        visuals.widgets.active.rounding = 2.0.into();
        visuals.widgets.hovered.rounding = 2.0.into();
        visuals.widgets.noninteractive.rounding = 2.0.into();
        visuals.window_rounding = 4.0.into();
        visuals.menu_rounding = 3.0.into();
        visuals.window_fill = self.panel;
        visuals.panel_fill = self.panel;
        visuals.extreme_bg_color = self.bg1;
        visuals.faint_bg_color = self.bg2;
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = egui::epaint::Shadow {
            offset: Vec2::new(0.0, 3.0),
            blur: 14.0,
            spread: 0.0,
            color: Color32::from_black_alpha(if dark { 90 } else { 36 }),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: Vec2::new(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(if dark { 80 } else { 28 }),
        };
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
        ]
        .into_iter()
        .collect();
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(10.0, 2.0);
        style.spacing.interact_size.y = 22.0;
        ctx.set_style(style);
    }
}

const SC_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const SC_REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const SC_REDO2: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Y);

#[derive(PartialEq, Clone, Copy)]
enum Tool {
    Draw,
    Select,
    DirectSelect,
    CustomPeg,
}

#[derive(Clone, Copy)]
enum AlignAxis {
    Left,
    CenterX,
    Right,
    Top,
    CenterY,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum DockTab {
    Canvas,
    Layers,
    Circles,
    Properties,
    Grid,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Face(usize),
    Peg(u64),
    PegRef(PegRef),
}

enum ActiveDrag {
    Custom {
        start: Vec<(u64, Point)>,
        anchor: Point,
        /// Direct SelectでGrid/Sub参照をCustomへ変換した時点で、位置が
        /// 元に戻ってもドキュメント自体は変更済みであることを示す。
        converted: bool,
    },
}

/// G キーによる任意円のグラブ移動（Blender風）。クリックで確定・Escで取消
struct GrabState {
    base: Document,
    origins: Vec<(u64, Point)>,
    anchor: Point,
}

/// 選択ツールでの面のスナップ移動ドラッグ
struct ActiveFaceDrag {
    /// ドラッグ開始時点のドキュメント（履歴スナップショット兼、毎フレームの適用基準）
    base: Document,
    anchor: Point,
    indices: Vec<usize>,
    applied: (i32, i32),
}

struct ActiveDraw {
    th: Thread,
    start: StartAnchor,
    start_point: Point,
    elements: Vec<Element>,
    armed: bool,
    ignored: Vec<usize>,
    /// Undo直後の同一円への即時再捕捉だけを防ぐ。円の外へ出たら解除する。
    undo_guard: Option<usize>,
    /// Cmd+ZのCommandキー解放を「貫通確定」と誤認しないための抑制。
    suppress_cmd_ignore_until_release: bool,
    /// 前フレームで Command が押されていたか（離した瞬間の無視確定に使う）
    cmd_was_down: bool,
}

struct FaceRender {
    /// 外周リング（輪郭ストローク・当たり判定・プレビュー用）
    points: Vec<Point>,
    /// 塗り用の全リング（マージ済みは穴を含む。通常面は points 1本のみ）
    rings: Vec<Vec<Point>>,
    invalid: bool,
}

pub struct App {
    doc: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    pegs: Vec<Peg>,
    peg_refs: Vec<PegRef>,
    pegs_key: (u64, u64, u64, u32, u32, u64),
    canvas_size: Vec2,
    canvas_center: Point,
    view: ViewTransform,
    custom_radius: f64,
    active_custom_drag: Option<ActiveDrag>,
    face_drag: Option<ActiveFaceDrag>,
    grab: Option<GrabState>,
    view_overlay_timer: f32,
    show_circles: bool,
    dark_theme: bool,
    dock: DockState<DockTab>,
    drag_snapshot: Option<Document>,
    grid_edit_orig: Option<Document>,
    /// 任意円編集中の (id)
    radius_edit_orig: Option<u64>,
    bevel_edit_orig: Option<Document>,
    face_name_snapshot: Option<Document>,
    draw: Option<ActiveDraw>,
    selection: Vec<Selection>,
    face_editor: Option<usize>,
    tool: Tool,
    status: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_cjk_fonts(&cc.egui_ctx);
        let mut app = App {
            doc: Document::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            pegs: Vec::new(),
            peg_refs: Vec::new(),
            pegs_key: (0, 0, 0, 0, 0, 0),
            canvas_size: Vec2::new(1024.0, 768.0),
            canvas_center: Point::new(512.0, 384.0),
            view: ViewTransform {
                offset: Vec2::ZERO,
                zoom: 1.0,
            },
            custom_radius: 40.0,
            active_custom_drag: None,
            face_drag: None,
            grab: None,
            view_overlay_timer: 0.0,
            show_circles: true,
            dark_theme: true,
            dock: App::default_dock_layout(),
            drag_snapshot: None,
            grid_edit_orig: None,
            radius_edit_orig: None,
            bevel_edit_orig: None,
            face_name_snapshot: None,
            draw: None,
            selection: Vec::new(),
            face_editor: None,
            tool: Tool::Draw,
            status: "クリックで線を引き始め、始点に戻ってクリックで面を閉じます".to_owned(),
        };
        app.apply_theme(&cc.egui_ctx);
        app
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let colors = self.theme_colors();
        colors.apply_to_context(ctx, self.dark_theme);
    }

    fn enforce_safe_grid_radii(&mut self) -> bool {
        let (main_max, _) = safe_grid_radius_limits(&self.doc.grid);
        let mut changed = false;
        if self.doc.grid.radius > main_max {
            self.doc.grid.radius = main_max;
            changed = true;
        }
        let (_, sub_max) = safe_grid_radius_limits(&self.doc.grid);
        if self.doc.grid.subs_enabled && self.doc.grid.sub_radius > sub_max {
            self.doc.grid.sub_radius = sub_max;
            changed = true;
        }
        changed
    }

    fn default_dock_layout() -> DockState<DockTab> {
        let mut dock = DockState::new(vec![DockTab::Canvas]);
        // 右カラム: 上からグリッド（大きめ・既定表示）→ 円 → プロパティ → レイヤー（一番下）
        let [_, grid] = dock
            .main_surface_mut()
            .split_right(NodeIndex::root(), 0.80, vec![DockTab::Grid]);
        let [_, circles] = dock
            .main_surface_mut()
            .split_below(grid, 0.42, vec![DockTab::Circles]);
        let [_, props] = dock
            .main_surface_mut()
            .split_below(circles, 0.5, vec![DockTab::Properties]);
        dock.main_surface_mut()
            .split_below(props, 0.5, vec![DockTab::Layers]);
        dock
    }

    /// 描画中の Cmd+Z: 最後の巻き付きを解除する。同一円への即時再捕捉だけ
    /// 一時的に防ぎ、カーソルが円の外へ出れば通常の捕捉へ戻す。
    fn undo_last_catch(&mut self) {
        if let Some(active) = self.draw.as_mut() {
            // Commandキーの解放は、巻き付きが無い場合でもCmd+Z操作の一部。
            // 貫通確定として解釈しない。
            active.suppress_cmd_ignore_until_release = true;
            if let Some(w) = active.th.wraps.pop() {
                active.undo_guard = Some(w.peg);
                self.status = "直前の引っ掛かりを解除しました".to_owned();
            } else {
                self.status = "解除できる引っ掛かりがありません".to_owned();
            }
        }
    }


    /// 面をグリッドセル単位で平行移動する。Grid/Sub 参照は (row,col) を
    /// ずらし、Pin と参照先の任意円は同じ距離だけ移動する（スナップ移動）
    fn apply_face_translation(&mut self, base: &Document, indices: &[usize], drow: i32, dcol: i32) {
        self.doc = base.clone();
        if drow == 0 && dcol == 0 {
            return;
        }
        let spacing = self.doc.grid.spacing;
        let dx = dcol as f64 * spacing;
        let dy = drow as f64 * spacing;
        let mut moved_customs: Vec<u64> = Vec::new();
        for &i in indices {
            let Some(face) = self.doc.faces.get_mut(i) else { continue };
            let shift_ref = |peg: &mut PegRef, moved: &mut Vec<u64>| match peg {
                PegRef::Grid { row, col } | PegRef::Sub { row, col } => {
                    *row += drow;
                    *col += dcol;
                }
                PegRef::Custom { id } => {
                    if !moved.contains(id) {
                        moved.push(*id);
                    }
                }
            };
            shift_ref(&mut face.start.peg, &mut moved_customs);
            for el in &mut face.elements {
                match el {
                    Element::Wrap { peg, .. } => shift_ref(peg, &mut moved_customs),
                    Element::Pin { pos } => {
                        pos.x += dx;
                        pos.y += dy;
                    }
                }
            }
            if let Some(baked) = &mut face.baked {
                for ring in &mut baked.rings {
                    for point in ring {
                        point.x += dx;
                        point.y += dy;
                    }
                }
            }
        }
        for id in moved_customs {
            if let Some(peg) = self.doc.custom_pegs.iter_mut().find(|p| p.id == id) {
                peg.center.x += dx;
                peg.center.y += dy;
            }
        }
    }

    /// レイヤー行の小さな形状プレビュー
    fn face_preview_widget(&mut self, ui: &mut egui::Ui, face_idx: usize, size: f32) {
        let (resp, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click());
        let rect = resp.rect;
        let Some(face) = self.doc.faces.get(face_idx) else { return };
        let resolver =
            |r: &PegRef| resolve_peg(&self.doc.grid, &self.doc.custom_pegs, self.canvas_center, self.doc.peg_appearance, r);
        if let Some(segs) = materialize_face(face, &resolver) {
            let mut pts = Vec::new();
            self.flatten_segments_for_appearance(&segs, &mut pts, 0.4);
            if pts.len() >= 2 {
                let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
                let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
                for p in &pts {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                let span = (max_x - min_x).max(max_y - min_y).max(1.0);
                let scale = (f64::from(size) - 6.0) / span;
                let offset_x = (f64::from(size) - (max_x - min_x) * scale) * 0.5;
                let offset_y = (f64::from(size) - (max_y - min_y) * scale) * 0.5;
                let local: Vec<Pos2> = pts
                    .iter()
                    .map(|p| {
                        Pos2::new(
                            rect.min.x + (offset_x + (p.x - min_x) * scale) as f32,
                            rect.min.y + (offset_y + (p.y - min_y) * scale) as f32,
                        )
                    })
                    .collect();
                let color = ui.visuals().widgets.noninteractive.fg_stroke.color;
                painter.add(egui::Shape::closed_line(local, Stroke::new(1.0, color)));
            }
        }
        if resp.clicked() {
            self.set_single_selection(Some(Selection::Face(face_idx)));
        }
    }

    fn push_history(&mut self) {
        let snap = self.doc.clone();
        self.push_history_snapshot(snap);
    }

    /// 変更「前」の状態を undo に積む。ドラッグ編集のように mutate 済みの状態から
    /// 履歴を確定するときは、変更前を復元したスナップショットを渡すこと
    fn push_history_snapshot(&mut self, snap: Document) {
        self.undo.push(snap);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn is_selected(&self, target: Selection) -> bool {
        self.selection.contains(&target)
    }

    fn selected_pegs(&self) -> Vec<u64> {
        self.selection
            .iter()
            .filter_map(|sel| match sel {
                Selection::Peg(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn apply_click_selection(&mut self, target: Selection, shift: bool) {
        if shift {
            self.toggle_selection(target);
        } else {
            self.set_single_selection(Some(target));
        }
    }

    fn set_single_selection(&mut self, next: Option<Selection>) {
        self.selection.clear();
        if let Some(next) = next {
            self.selection.push(next);
        }
    }

    fn toggle_selection(&mut self, target: Selection) {
        if let Some(i) = self.selection.iter().position(|sel| *sel == target) {
            self.selection.remove(i);
        } else {
            self.selection.push(target);
        }
    }

    fn sync_selection<F>(&mut self, filter: F)
    where
        F: FnMut(&Selection) -> bool,
    {
        self.selection.retain(filter);
    }

    fn reindex_faces_after_deletion(&mut self, removed: &[usize]) {
        if removed.is_empty() {
            return;
        }
        let removed: Vec<usize> = {
            let mut copy = removed.to_vec();
            copy.sort_unstable();
            copy
        };
        self.selection.retain(|sel| match sel {
            Selection::Face(index) => !removed.contains(index),
            _ => true,
        });

        for sel in &mut self.selection {
            if let Selection::Face(index) = sel {
                let remove_before = removed.iter().take_while(|r| **r < *index).count();
                if *index >= remove_before {
                    *index -= remove_before;
                }
            }
        }
    }

    fn clear_selection(&mut self) {
        self.selection.clear();
    }

    fn begin_drag(&mut self, ids: &[u64], anchor: Point, converted: bool) {
        let mut start = Vec::new();
        for id in ids {
            if let Some(peg) = self.doc.custom_pegs.iter().find(|peg| peg.id == *id) {
                start.push((*id, peg.center));
            }
        }
        if !start.is_empty() {
            self.active_custom_drag = Some(ActiveDrag::Custom {
                start,
                anchor,
                converted,
            });
        }
    }

    /// 操作途中で既にドキュメントへ反映された変更を開始時点へ戻す。
    /// Escape・ツール切替・Undoの入口を一本化し、古いドラッグ状態が
    /// 次フレームでUndo結果を上書きしないようにする。
    fn cancel_active_manipulation(&mut self) -> bool {
        let mut canceled = false;
        if let Some(grab) = self.grab.take() {
            self.doc = grab.base;
            canceled = true;
        }
        if let Some(face_drag) = self.face_drag.take() {
            self.doc = face_drag.base;
            canceled = true;
        }
        if self.active_custom_drag.take().is_some() {
            if let Some(snapshot) = self.drag_snapshot.take() {
                self.doc = snapshot;
            }
            canceled = true;
        }
        if canceled {
            self.radius_edit_orig = None;
            self.rebuild_pegs(self.canvas_size);
        }
        canceled
    }

    fn do_undo(&mut self) {
        self.cancel_active_manipulation();
        if let Some(d) = self.undo.pop() {
            self.redo.push(self.doc.clone());
            self.doc = d;
            self.draw = None;
            self.clear_selection();
            self.status = "元に戻しました".to_owned();
        }
    }

    fn do_redo(&mut self) {
        self.cancel_active_manipulation();
        if let Some(d) = self.redo.pop() {
            self.undo.push(self.doc.clone());
            self.doc = d;
            self.draw = None;
            self.clear_selection();
            self.status = "やり直しました".to_owned();
        }
    }

    fn delete_selected(&mut self) {
        if self.draw.is_some() {
            self.status = "描画中は削除できません".to_owned();
            return;
        }
        let selections = self.selection.clone();
        if selections.is_empty() {
            self.status = "削除対象がありません".to_owned();
            return;
        }

        let mut face_indices = Vec::new();
        let mut peg_ids = Vec::new();
        let mut peg_refs = Vec::new();
        let mut skip_count = 0usize;
        for sel in selections {
            match sel {
                Selection::Face(i) => {
                    if i < self.doc.faces.len() && !face_indices.contains(&i) {
                        face_indices.push(i);
                    }
                }
                Selection::Peg(id) => {
                    if self.custom_peg_in_use(id) {
                        skip_count += 1;
                    } else if !peg_ids.contains(&id) {
                        peg_ids.push(id);
                    }
                }
                Selection::PegRef(peg_ref) => {
                    let used_as_start = self.doc.faces.iter().any(|face| face.start.peg == peg_ref);
                    if used_as_start {
                        skip_count += 1;
                    } else if !self.doc.disabled_pegs.contains(&peg_ref) && !peg_refs.contains(&peg_ref) {
                        peg_refs.push(peg_ref);
                    }
                }
            }
        }

        if face_indices.is_empty() && peg_ids.is_empty() && peg_refs.is_empty() {
            if skip_count > 0 {
                self.status = "参照中の円は削除できません".to_owned();
            } else {
                self.status = "削除対象がありません".to_owned();
            }
            return;
        }

        self.push_history();
        face_indices.sort_unstable_by(|a, b| b.cmp(a));
        for face_idx in &face_indices {
            self.doc.faces.remove(*face_idx);
        }
        for id in &peg_ids {
            if let Some(idx) = self.doc.custom_pegs.iter().position(|peg| peg.id == *id) {
                self.doc.custom_pegs.remove(idx);
            }
        }
        for peg_ref in &peg_refs {
            self.doc.disabled_pegs.push(*peg_ref);
            for face in &mut self.doc.faces {
                face.elements.retain(|element| !matches!(element, Element::Wrap { peg, .. } if peg == peg_ref));
            }
        }

        if !peg_ids.is_empty() {
            self.doc.next_peg_id = self.next_peg_id();
        }
        self.reindex_faces_after_deletion(&face_indices);
        let faces_len = self.doc.faces.len();
        self.sync_selection(|sel| match sel {
            Selection::Peg(id) => !peg_ids.contains(id),
            Selection::Face(i) => *i < faces_len,
            Selection::PegRef(peg_ref) => !peg_refs.contains(peg_ref),
        });
        self.status = if skip_count > 0 {
            "一部の項目は参照中のため削除しませんでした".to_owned()
        } else if peg_ids.is_empty() && face_indices.is_empty() && peg_refs.is_empty() {
            "削除対象がありません".to_owned()
        } else {
            if peg_refs.is_empty() { "選択項目を削除しました".to_owned() } else { "選択したグリッド円を無効化し、糸を張り直しました".to_owned() }
        };
        if !peg_refs.is_empty() {
            self.rebuild_pegs(self.canvas_size);
        }
    }

    fn clear_glyph(&mut self) {
        if !self.doc.faces.is_empty() || self.draw.is_some() {
            self.push_history();
            self.doc.faces.clear();
            self.draw = None;
            self.clear_selection();
            self.status = "グリフをクリアしました".to_owned();
        }
    }

    fn next_peg_id(&self) -> u64 {
        self.doc
            .custom_pegs
            .iter()
            .map(|peg| peg.id + 1)
            .max()
            .unwrap_or(0)
    }

    fn world_unit(&self, px: f64) -> f64 {
        if self.view.zoom == 0.0 {
            return px;
        }
        px / f64::from(self.view.zoom)
    }

    fn to_screen(&self, origin: Pos2, p: Point) -> Pos2 {
        Pos2::new(
            origin.x + p.x as f32 * self.view.zoom + self.view.offset.x,
            origin.y + p.y as f32 * self.view.zoom + self.view.offset.y,
        )
    }

    fn to_world(&self, origin: Pos2, p: Pos2) -> Point {
        Point::new(
            (p.x - origin.x - self.view.offset.x) as f64 / f64::from(self.view.zoom),
            (p.y - origin.y - self.view.offset.y) as f64 / f64::from(self.view.zoom),
        )
    }

    fn set_zoom(&mut self, origin: Pos2, cursor: Pos2, zoom: f32) -> bool {
        let world = self.to_world(origin, cursor);
        let zoom = ViewTransform::clamp_zoom(zoom);
        let world_x = (world.x as f32) * zoom;
        let world_y = (world.y as f32) * zoom;
        self.view.offset = Vec2::new(cursor.x - origin.x - world_x, cursor.y - origin.y - world_y);
        if (self.view.zoom - zoom).abs() < f32::EPSILON {
            return false;
        }
        self.view.zoom = zoom;
        true
    }

    fn pan_view(&mut self, delta: Vec2) -> bool {
        if delta.x == 0.0 && delta.y == 0.0 {
            return false;
        }
        self.view.offset += delta;
        true
    }

    fn custom_peg_in_use(&self, id: u64) -> bool {
        self.doc.faces.iter().any(|face| {
            if matches!(face.start.peg, PegRef::Custom { id: used } if used == id) {
                return true;
            }
            face.elements.iter().any(|el| match *el {
                Element::Wrap { peg: PegRef::Custom { id: used }, .. } if used == id => true,
                _ => false,
            })
        })
    }

    fn face_references_source(&self, source: &PegRef) -> bool {
        self.doc
            .faces
            .iter()
            .any(|face| self.face_references_peg(face, source))
    }

    fn replace_peg_ref_in_faces(&mut self, source: PegRef, target: PegRef) {
        for face in &mut self.doc.faces {
            if face.start.peg == source {
                face.start.peg = target;
            }
            for el in &mut face.elements {
                if let Element::Wrap { peg, .. } = el {
                    if *peg == source {
                        *peg = target;
                    }
                }
            }
        }
    }

    fn direct_select_drag_target(
        &mut self,
        source: PegRef,
        source_center: Point,
        source_radius: f64,
    ) -> u64 {
        if let PegRef::Custom { id } = source {
            return id;
        }
        let id = self.doc.next_peg_id;
        self.doc.custom_pegs.push(CustomPeg {
            id,
            center: source_center,
            radius: source_radius,
            visible: true,
        });
        self.replace_peg_ref_in_faces(source, PegRef::Custom { id });
        self.doc.next_peg_id = self.next_peg_id();
        id
    }

    fn peg_at_cursor(&self, point: Point) -> Option<(PegRef, Point)> {
        self.pegs
            .iter()
            .zip(self.peg_refs.iter())
            .filter(|(_, peg_ref)| match peg_ref {
                PegRef::Custom { id } => self
                    .doc
                    .custom_pegs
                    .iter()
                    .find(|custom| custom.id == *id)
                    .is_none_or(|custom| custom.visible),
                _ => true,
            })
            .rfind(|(peg, _)| peg_signed_distance(peg, point) <= self.doc.grid.catch)
            .map(|(peg, peg_ref)| (*peg_ref, peg.center))
    }

    fn wrap_elements_from_thread(&self, th: &Thread, end: Point) -> Option<Vec<Element>> {
        thread_to_wrap_elements(th, &self.pegs, &self.peg_refs, end)
    }

    fn has_penetration_on_lines(&self, segs: &[Seg]) -> bool {
        let mut points = Vec::new();
        if segs.is_empty() {
            return false;
        }
        self.flatten_segments_for_appearance(segs, &mut points, 0.4);
        self.has_self_intersections(&points)
    }

    fn has_self_intersections(&self, points: &[Point]) -> bool {
        if points.len() < 4 {
            return false;
        }
        let repeated_end = points.first().zip(points.last()).is_some_and(|(a, b)| a.distance(*b) < 1e-8);
        let vertex_count = points.len() - usize::from(repeated_end);
        if vertex_count < 3 {
            return false;
        }
        for i in 0..vertex_count {
            let a1 = points[i];
            let a2 = points[(i + 1) % vertex_count];
            for j in (i + 1)..vertex_count {
                // 同じ頂点を共有する隣接辺と、閉路の先頭/末尾辺は交差扱いしない。
                if j == i + 1 || (i == 0 && j == vertex_count - 1) {
                    continue;
                }
                let b1 = points[j];
                let b2 = points[(j + 1) % vertex_count];
                if segments_intersect(a1, a2, b1, b2) {
                    return true;
                }
            }
        }
        false
    }

    fn command_line_penetrates(&self, active: &ActiveDraw) -> bool {
        let head = active.th.head(&self.pegs);
        let exclude = active.th.wraps.last().map(|w| w.peg);
        let mut ignored = active.ignored.clone();
        if let Some(guard) = active.undo_guard {
            if !ignored.contains(&guard) {
                ignored.push(guard);
            }
        }
        !pegs_penetrated_by_line(head, active.th.tip, &self.pegs, exclude, &ignored).is_empty()
    }

    fn engine_pegs(
        grid: &GridSettings,
        size: Vec2,
        canvas_center: Point,
        customs: &[CustomPeg],
        appearance: crate::model::PegAppearance,
        disabled: &[PegRef],
    ) -> (Vec<Peg>, Vec<PegRef>) {
        let origin = match grid.scale_anchor {
            ScaleAnchor::TopLeft => Point::new(110.0, 110.0),
            ScaleAnchor::Center => Point::new(
                canvas_center.x - 3.0 * grid.spacing,
                canvas_center.y - 2.0 * grid.spacing,
            ),
        };
        let radius = grid.radius.max(grid.sub_radius) + grid.catch;
        let min_col = (((-radius) - origin.x) / grid.spacing).floor() as i32 - 1;
        let max_col = (((size.x as f64 + radius) - origin.x) / grid.spacing).ceil() as i32 + 1;
        let min_row = (((-radius) - origin.y) / grid.spacing).floor() as i32 - 1;
        let max_row = (((size.y as f64 + radius) - origin.y) / grid.spacing).ceil() as i32 + 1;

        let mut pegs = Vec::new();
        let mut refs = Vec::new();
        for row in min_row..=max_row {
            for col in min_col..=max_col {
                let grid_ref = PegRef::Grid { row, col };
                if !disabled.contains(&grid_ref) {
                if let Some(peg) = resolve_peg(grid, customs, canvas_center, appearance, &grid_ref) {
                    pegs.push(peg);
                    refs.push(grid_ref);
                }
                }
                if grid.subs_enabled {
                    let sub_ref = PegRef::Sub { row, col };
                    if !disabled.contains(&sub_ref) {
                    if let Some(peg) =
                        resolve_peg(grid, customs, canvas_center, appearance, &sub_ref)
                    {
                        pegs.push(peg);
                        refs.push(sub_ref);
                    }
                    }
                }
            }
        }
        for custom in customs {
            if let Some(peg) =
                resolve_peg(grid, customs, canvas_center, appearance, &PegRef::Custom { id: custom.id })
            {
                pegs.push(peg);
                refs.push(PegRef::Custom { id: custom.id });
            }
        }
        (pegs, refs)
    }

    fn peg_ref_key(
        grid: &GridSettings,
        size: Vec2,
        customs: &[CustomPeg],
        appearance: crate::model::PegAppearance,
        disabled: &[PegRef],
    ) -> (u64, u64, u64, u32, u32, u64) {
        let mut custom_hash = 0u64;
        for custom in customs {
            custom_hash ^= custom.id.wrapping_mul(7);
            custom_hash ^= custom.center.x.to_bits();
            custom_hash ^= custom.center.y.to_bits();
            custom_hash ^= custom.radius.to_bits();
        }
        custom_hash ^= grid.sub_radius.to_bits();
        custom_hash ^= u64::from(grid.subs_enabled as u8);
        custom_hash ^= u64::from(grid.scale_link as u8);
        custom_hash ^= appearance.rounding.to_bits();
        custom_hash ^= match appearance.shape {
            crate::model::PegShape::Circle => 0,
            crate::model::PegShape::RoundedSquare => 0x9e37_79b9,
        };
        for peg_ref in disabled {
            custom_hash ^= match *peg_ref {
                PegRef::Grid { row, col } => (row as u64).wrapping_mul(31) ^ (col as u64).wrapping_mul(131),
                PegRef::Sub { row, col } => 0x51b5 ^ (row as u64).wrapping_mul(37) ^ (col as u64).wrapping_mul(137),
                PegRef::Custom { id } => 0xa11c ^ id.wrapping_mul(149),
            };
        }
        custom_hash ^= match grid.scale_anchor {
            ScaleAnchor::TopLeft => 0,
            ScaleAnchor::Center => 1,
        } as u64;
        (
            grid.spacing.to_bits(),
            grid.radius.to_bits(),
            grid.catch.to_bits(),
            size.x.round() as u32,
            size.y.round() as u32,
            custom_hash,
        )
    }

    fn rebuild_pegs(&mut self, size: Vec2) {
        let key = Self::peg_ref_key(&self.doc.grid, size, &self.doc.custom_pegs, self.doc.peg_appearance, &self.doc.disabled_pegs);
        if key == self.pegs_key {
            return;
        }
        let (pegs, refs) =
            Self::engine_pegs(&self.doc.grid, size, self.canvas_center, &self.doc.custom_pegs, self.doc.peg_appearance, &self.doc.disabled_pegs);
        self.pegs = pegs;
        self.peg_refs = refs;
        self.pegs_key = key;
        if self.draw.is_some() {
            self.draw = None;
            self.status = "グリッド/空間変更のため描画中の線をキャンセルしました".to_owned();
        }
    }

    fn nearest_grid_peg_for_start(
        &self,
        pos: Point,
    ) -> Option<(Peg, PegRef)> {
        let mut best: Option<(f64, Peg, PegRef)> = None;
        let prox = self.world_unit(START_PROX);
        for (peg, peg_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
            let d = pos.distance(peg.center);
            if d <= peg.radius + prox {
                let dist = d;
                match best {
                    Some((bd, _, _)) if dist >= bd => {}
                    _ => best = Some((dist, *peg, *peg_ref)),
                }
            }
        }
        best.map(|(_, peg, peg_ref)| (peg, peg_ref))
    }

    fn start_anchor_candidates(&self, cursor: Point) -> Option<Vec<(Point, StartAnchor)>> {
        let Some((peg, peg_ref)) = self.nearest_grid_peg_for_start(cursor) else {
            return None;
        };
        // 十字4点 + 斜め45度4点の計8方向
        let mut out = Vec::with_capacity(Port::ALL.len());
        for port in Port::ALL {
            out.push((
                crate::geometry::on_peg(&peg, port.angle()),
                StartAnchor { peg: peg_ref, port },
            ));
        }
        Some(out)
    }

    fn pick_start_anchor(
        &self,
        cursor: Point,
    ) -> Option<(Point, StartAnchor)> {
        let candidates = self.start_anchor_candidates(cursor)?;
        let snap = self.world_unit(START_SNAP);
        candidates
            .into_iter()
            .filter(|(p, _)| p.distance(cursor) <= snap)
            .min_by(|(a, _), (b, _)| {
                a.distance(cursor)
                    .partial_cmp(&b.distance(cursor))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    fn draw_start_markers(
        &self,
        ui_pos: Option<Point>,
        painter: &egui::Painter,
        origin: Pos2,
    ) {
        let Some(pos) = ui_pos else {
            return;
        };
        let Some(candidates) = self.start_anchor_candidates(pos) else {
            return;
        };
        let colors = self.theme_colors();
        for (point, _anchor) in candidates {
            let screen = self.to_screen(origin, point);
            let color = if point.distance(pos) <= self.world_unit(START_SNAP) {
                colors.start_highlight
            } else {
                colors.start
            };
            painter.circle_filled(screen, 3.0, color);
            painter.circle_stroke(screen, 3.0, Stroke::new(1.0, colors.bg));
        }
    }

    fn can_place_custom_peg(&self, center: Point, radius: f64, ignore: &[u64]) -> bool {
        let mut clear = true;
        let r = radius + self.doc.grid.catch;
        for (peg, peg_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
            if let PegRef::Custom { id: used } = *peg_ref {
                if ignore.contains(&used) {
                    continue;
                }
            }
            if center.distance(peg.center) <= (peg.radius + r) {
                clear = false;
                break;
            }
        }
        clear
    }

    /// 任意円がグリッド円・サブ円のどれかと重なっているか（他の任意円は除外）
    fn custom_peg_overlaps_grid(&self, peg: &Peg) -> bool {
        for (other, other_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
            if matches!(other_ref, PegRef::Custom { .. }) {
                continue;
            }
            if peg.center.distance(other.center) < peg.radius + other.radius {
                return true;
            }
        }
        false
    }

    fn can_place_custom_pegs(&self, moved: &[(u64, Point, f64)], ignore: &[u64]) -> bool {
        for (id, center, radius) in moved {
            if !self.can_place_custom_peg(*center, *radius, ignore) {
                return false;
            }
            let r = *radius + self.doc.grid.catch;
            for (other_id, other_center, other_radius) in moved {
                if other_id <= id {
                    continue;
                }
                let other_r = *other_radius + self.doc.grid.catch;
                if center.distance(*other_center) <= (r + other_r) {
                    return false;
                }
            }
        }
        true
    }

    fn materialized_faces(&self) -> Vec<(usize, FaceRender)> {
        let resolver = |r: &PegRef| {
            resolve_peg(&self.doc.grid, &self.doc.custom_pegs, self.canvas_center, self.doc.peg_appearance, r)
        };
        let mut out = Vec::new();
        for (i, face) in self.doc.faces.iter().enumerate() {
            if !face.visible {
                continue;
            }
            if let Some(baked) = &face.baked {
                // マージ済み: 焼き込みリングにベベルを適用して描く
                let rings: Vec<Vec<Point>> = baked
                    .rings
                    .iter()
                    .map(|r| bevel_ring(r, baked.bevel))
                    .filter(|r| r.len() >= 3)
                    .collect();
                if rings.is_empty() {
                    continue;
                }
                // 外周 = 面積が最大のリング
                let points = rings
                    .iter()
                    .max_by(|a, b| {
                        signed_area(a)
                            .abs()
                            .partial_cmp(&signed_area(b).abs())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .cloned()
                    .unwrap_or_default();
                out.push((i, FaceRender { points, rings, invalid: false }));
                continue;
            }
            let obstacles: Vec<(PegRef, Peg)> = self
                .peg_refs
                .iter()
                .copied()
                .zip(self.pegs.iter().copied())
                .filter(|(peg_ref, _)| !face.ignored_pegs.contains(peg_ref))
                .collect();
            if let Some(segments) = materialize_face_with_obstacles(face, &resolver, &obstacles) {
                let mut points = Vec::new();
                // 分割しすぎるとフェザリングで弧が細く見えるため画面基準の弦誤差で展開
                let tol = (0.35 / f64::from(self.view.zoom)).max(0.01);
                self.flatten_segments_for_appearance(&segments, &mut points, tol);
                // NonZero 塗りが常に合体になるよう、向きは出力時に正へ揃える
                if signed_area(&points) < 0.0 {
                    points.reverse();
                }
                let invalid = self.has_self_intersections(&points)
                    || self.face_has_overwrapped_peg(face)
                    || segments_penetrate_pegs(
                        &segments,
                        &obstacles.iter().map(|(_, peg)| *peg).collect::<Vec<_>>(),
                    );
                out.push((
                    i,
                    FaceRender {
                        rings: vec![points.clone()],
                        points,
                        invalid,
                    },
                ));
            }
        }
        out
    }

    fn face_bounds(render: &FaceRender) -> Option<(f64, f64, f64, f64)> {
        if render.points.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for point in &render.points {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        Some((min_x, min_y, max_x, max_y))
    }

    fn flatten_segments_for_appearance(&self, segs: &[Seg], out: &mut Vec<Point>, tol: f64) {
        for seg in segs {
            match *seg {
                Seg::Line { from, to } => {
                    if out.is_empty() { out.push(from); }
                    out.push(to);
                }
                Seg::Arc { center, radius, start, end } => {
                    let peg = self.pegs.iter().find(|peg| peg.center.distance(center) < 0.01 && (peg.radius - radius).abs() < 0.01).copied();
                    // tol は弧上の点間距離ではなく、円弧と弦の最大誤差として扱う。
                    // 極端な短線分の連続はアンチエイリアスで円弧だけ薄く見えるため、
                    // サジッタから必要角度を逆算して直線部と濃度を揃える。
                    let safe_tol = tol.max(0.01).min(radius.max(0.01));
                    let step_angle = (2.0 * (1.0 - safe_tol / radius.max(0.01)).acos())
                        .clamp(0.01, std::f64::consts::PI / 2.0);
                    let steps = (((end - start).abs() / step_angle).ceil() as usize).clamp(2, 160);
                    for i in 0..=steps {
                        let angle = start + (end - start) * (i as f64 / steps as f64);
                        let point = peg.as_ref().map_or_else(
                            || crate::geometry::on_circle(center, radius, angle),
                            |peg| crate::geometry::on_peg(peg, angle),
                        );
                        if out.last().is_none_or(|last| last.distance(point) > tol * 0.25) { out.push(point); }
                    }
                }
            }
        }
    }

    /// 選択した面をグリッド単位で整列する。既存の面表現を保つため、
    /// Illustratorの整列結果に最も近いセル移動へスナップする。
    fn align_selected_faces(&mut self, axis: AlignAxis) {
        let indices: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|selection| match selection {
                Selection::Face(index) => Some(*index),
                _ => None,
            })
            .collect();
        if indices.len() < 2 {
            self.status = "整列するレイヤーを2つ以上選んでください".to_owned();
            return;
        }
        let rendered = self.materialized_faces();
        let Some(reference) = rendered.iter().find(|(index, _)| *index == indices[0]).and_then(|(_, render)| Self::face_bounds(render)) else {
            return;
        };
        let target = match axis {
            AlignAxis::Left => reference.0,
            AlignAxis::CenterX => (reference.0 + reference.2) * 0.5,
            AlignAxis::Right => reference.2,
            AlignAxis::Top => reference.1,
            AlignAxis::CenterY => (reference.1 + reference.3) * 0.5,
            AlignAxis::Bottom => reference.3,
        };
        let snapshot = self.doc.clone();
        let spacing = self.doc.grid.spacing.max(1.0);
        let mut moved = false;
        for index in indices.iter().skip(1) {
            let Some(current) = self
                .materialized_faces()
                .into_iter()
                .find(|(render_index, _)| render_index == index)
                .and_then(|(_, render)| Self::face_bounds(&render))
            else {
                continue;
            };
            let current_value = match axis {
                AlignAxis::Left => current.0,
                AlignAxis::CenterX => (current.0 + current.2) * 0.5,
                AlignAxis::Right => current.2,
                AlignAxis::Top => current.1,
                AlignAxis::CenterY => (current.1 + current.3) * 0.5,
                AlignAxis::Bottom => current.3,
            };
            let delta = target - current_value;
            let (drow, dcol) = match axis {
                AlignAxis::Left | AlignAxis::CenterX | AlignAxis::Right => (0, (delta / spacing).round() as i32),
                AlignAxis::Top | AlignAxis::CenterY | AlignAxis::Bottom => ((delta / spacing).round() as i32, 0),
            };
            if drow != 0 || dcol != 0 {
                let base = self.doc.clone();
                self.apply_face_translation(&base, &[*index], drow, dcol);
                moved = true;
            }
        }
        if moved {
            self.push_history_snapshot(snapshot);
            self.rebuild_pegs(self.canvas_size);
            self.status = "レイヤーを整列しました（グリッド単位）".to_owned();
        } else {
            self.status = "整列対象はすでに同じ位置です".to_owned();
        }
    }

    /// 選択中の複数レイヤーをユニオンして1枚の焼き込みレイヤーにまとめる
    fn merge_selected(&mut self) {
        let indices: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|s| match s {
                Selection::Face(i) => Some(*i),
                _ => None,
            })
            .collect();
        if indices.len() < 2 {
            self.status = "マージするレイヤーを2つ以上選んでください".to_owned();
            return;
        }
        let resolver =
            |r: &PegRef| resolve_peg(&self.doc.grid, &self.doc.custom_pegs, self.canvas_center, self.doc.peg_appearance, r);
        let obstacles: Vec<(PegRef, Peg)> = self
            .peg_refs
            .iter()
            .copied()
            .zip(self.pegs.iter().copied())
            .collect();
        let mut shapes: Vec<Vec<Vec<Point>>> = Vec::new();
        for &i in &indices {
            if let Some(face) = self.doc.faces.get(i) {
                let rings = if let Some(baked) = &face.baked {
                    baked
                        .rings
                        .iter()
                        .map(|ring| bevel_ring(ring, baked.bevel))
                        .filter(|ring| ring.len() >= 3)
                        .collect()
                } else if let Some(segs) = materialize_face_with_obstacles(face, &resolver, &obstacles) {
                    let mut pts = Vec::new();
                    self.flatten_segments_for_appearance(&segs, &mut pts, 0.2);
                    vec![pts]
                } else {
                    continue;
                };
                if !rings.is_empty() {
                    shapes.push(rings);
                }
            }
        }
        let rings = union_polygon_ring_sets(&shapes);
        if rings.is_empty() {
            self.status = "マージできませんでした".to_owned();
            return;
        }
        self.push_history();
        // 元レイヤーを削除（大きい添字から）
        let mut sorted = indices.clone();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        for i in sorted {
            if i < self.doc.faces.len() {
                self.doc.faces.remove(i);
            }
        }
        let n = self.doc.faces.len() + 1;
        self.doc.faces.push(Face {
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::Top },
            elements: Vec::new(),
            name: format!("マージ {n}"),
            visible: true,
            baked: Some(crate::model::BakedShape { rings, bevel: 0.0 }),
            ignored_pegs: Vec::new(),
        });
        self.set_single_selection(Some(Selection::Face(self.doc.faces.len() - 1)));
        self.status = "レイヤーをマージしました".to_owned();
    }

    fn face_has_overwrapped_peg(&self, face: &Face) -> bool {
        face.elements.iter().any(|el| {
            if let Element::Wrap { turns, .. } = *el {
                turns > 2
            } else {
                false
            }
        })
    }

    fn face_references_peg(&self, face: &Face, ref_peg: &PegRef) -> bool {
        if face.start.peg == *ref_peg {
            return true;
        }
        face.elements.iter().any(|el| match *el {
            Element::Wrap { peg, .. } => peg == *ref_peg,
            _ => false,
        })
    }

    fn theme_colors(&self) -> Theme {
        if self.dark_theme {
            Theme::dark()
        } else {
            Theme::light()
        }
    }

    fn save_json(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name("glyph.json")
            .save_file()
        {
            let json = serde_json::to_string_pretty(&self.doc).unwrap_or_default();
            match std::fs::write(&path, json) {
                Ok(()) => self.status = format!("JSONを保存しました: {}", path.display()),
                Err(e) => self.status = format!("保存に失敗しました: {e}"),
            }
        }
    }

    fn load_json(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<Document>(&s).map_err(|e| e.to_string()))
            {
                Ok(mut doc) if doc.version <= CURRENT_DOCUMENT_VERSION => {
                    self.push_history();
                    doc.version = CURRENT_DOCUMENT_VERSION;
                    self.doc = doc;
                    self.doc.next_peg_id = self.next_peg_id();
                    self.draw = None;
                    self.clear_selection();
                    self.status = format!("JSONを読み込みました: {}", path.display());
                }
                Ok(doc) => {
                    self.status = format!(
                        "このファイルは新しいバージョンです（ファイル: v{} / アプリ: v{}）",
                        doc.version, CURRENT_DOCUMENT_VERSION
                    );
                }
                Err(e) => self.status = format!("読み込みに失敗しました: {e}"),
            }
        }
    }

    fn export_svg(&mut self) {
        if self.doc.faces.is_empty() {
            self.status = "書き出す面がありません".to_owned();
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_file_name("glyph.svg")
            .save_file()
        {
            match std::fs::write(&path, document_to_svg(&self.doc, self.canvas_center)) {
                Ok(()) => self.status = format!("SVGを書き出しました: {}", path.display()),
                Err(e) => self.status = format!("書き出しに失敗しました: {e}"),
            }
        }
    }

    fn hit_test_face(&self, point: Point, rendered: &[(usize, FaceRender)]) -> Option<Selection> {
        if let Some(custom) = self
            .doc
            .custom_pegs
            .iter()
            .rev()
            .find(|custom| custom.visible && resolve_peg(&self.doc.grid, &self.doc.custom_pegs, self.canvas_center, self.doc.peg_appearance, &PegRef::Custom { id: custom.id }).is_some_and(|peg| peg_signed_distance(&peg, point) <= self.doc.grid.catch))
        {
            return Some(Selection::Peg(custom.id));
        }
        rendered
            .iter()
            .rev()
            .find_map(|(i, render)| {
                // 全リングをeven-oddで判定する。これにより独立した小さい成分も
                // 選択でき、穴の内部は面として誤選択しない。
                if render
                    .rings
                    .iter()
                    .filter(|ring| point_in_polygon(point, ring))
                    .count()
                    % 2
                    == 1
                {
                    Some(Selection::Face(*i))
                } else {
                    None
                }
            })
    }

    fn render_tab_content(&mut self, ui: &mut egui::Ui, tab: DockTab) {
        match tab {
            DockTab::Canvas => self.canvas(ui),
            DockTab::Layers => self.render_layers_tab(ui),
            DockTab::Circles => self.render_circles_tab(ui),
            DockTab::Properties => self.render_properties_tab(ui),
            DockTab::Grid => self.render_grid_tab(ui),
        }
    }

    /// フローティングのツールパレット。上端の掴みバーをドラッグで自由に移動でき、
    /// egui_dock のタブ幅制約を受けずに細いまま保てる（Illustrator のツールバー相当）
    fn tool_palette(&mut self, ctx: &egui::Context) {
        let colors = self.theme_colors();
        egui::Area::new(egui::Id::new("tool-palette"))
            .movable(true)
            .default_pos(Pos2::new(52.0, 44.0))
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(colors.panel)
                    .stroke(Stroke::new(1.0, colors.border))
                    .rounding(4.0)
                    .shadow(egui::epaint::Shadow {
                        offset: Vec2::new(0.0, 2.0),
                        blur: 10.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(if self.dark_theme { 90 } else { 30 }),
                    })
                    .inner_margin(egui::Margin::symmetric(4.0, 4.0))
                    .show(ui, |ui| {
                        ui.set_width(30.0);
                        ui.vertical_centered(|ui| {
                            // 掴みバー（横棒）
                            let (bar, _) =
                                ui.allocate_exact_size(Vec2::new(18.0, 8.0), Sense::hover());
                            for k in 0..2 {
                                let y = bar.center().y - 1.5 + k as f32 * 3.0;
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(bar.left() + 2.0, y),
                                        Pos2::new(bar.right() - 2.0, y),
                                    ],
                                    Stroke::new(1.0, colors.text3),
                                );
                            }
                            ui.add_space(2.0);
                            for (tool, tooltip) in [
                                (Tool::Select, "選択 (V)\nレイヤー・任意円をクリックで選択、ドラッグで移動"),
                                (Tool::DirectSelect, "ダイレクト選択 (A)\n構成する円を個別に選択・移動"),
                                (Tool::Draw, "描画 (P)\n十字点から線を引き、円に引っ掛けて始点で閉じる"),
                                (Tool::CustomPeg, "任意円 (C)\nクリックで円を配置（緑=配置可 / 赤=不可）"),
                            ] {
                                if tool_icon_button(ui, self.tool == tool, tool, tooltip).clicked() {
                                    self.tool = tool;
                                }
                                ui.add_space(2.0);
                            }
                        });
                    });
            });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            // ツールは左のドッキング可能な「ツール」パネルへ。元に戻す/やり直すは
            // Cmd+Z / Cmd+Shift+Z に集約したのでバーからは省く
            let sel_faces = self
                .selection
                .iter()
                .filter(|s| matches!(s, Selection::Face(_)))
                .count();
            if ui
                .add_enabled(sel_faces >= 2, egui::Button::new("マージ"))
                .on_hover_text("選択した2つ以上のレイヤーをパス結合する")
                .clicked()
            {
                self.merge_selected();
            }
            if self.tool == Tool::CustomPeg {
                ui.separator();
                ui.add_sized(
                    [190.0, 22.0],
                    egui::Slider::new(&mut self.custom_radius, 4.0..=120.0)
                        .text("新規円 半径"),
                );
            }
            ui.separator();
            if ui.button("SVG書き出し").clicked() {
                self.export_svg();
            }
            if ui.button("保存").clicked() {
                self.save_json();
            }
            if ui.button("読み込み").clicked() {
                self.load_json();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("パネル", |ui| {
                    for (tab, label) in [
                        (DockTab::Layers, "レイヤー"),
                        (DockTab::Circles, "円"),
                        (DockTab::Properties, "プロパティ"),
                        (DockTab::Grid, "グリッド"),
                    ] {
                        let open = self.dock.find_tab(&tab).is_some();
                        if ui.add_enabled(!open, egui::Button::new(label)).clicked() {
                            self.dock.add_window(vec![tab]);
                            ui.close_menu();
                        }
                    }
                });
                if theme_icon_button(ui, self.dark_theme).clicked() {
                    self.dark_theme = !self.dark_theme;
                }
                if icon_toggle_button(ui, self.show_circles, IconKind::Eye, "円を表示 (H)")
                    .clicked()
                {
                    self.show_circles = !self.show_circles;
                }
            });
        });
    }

    /// 選択ツール時だけ表示する、Illustrator風のフローティング整列パレット。
    fn alignment_palette(&mut self, ctx: &egui::Context) {
        if self.tool != Tool::Select
            || self.selection.iter().filter(|s| matches!(s, Selection::Face(_))).count() < 2
        {
            return;
        }
        let mut clicked = None;
        egui::Area::new(egui::Id::new("alignment-palette"))
            .movable(true)
            .default_pos(Pos2::new(94.0, 44.0))
            .constrain(true)
            .show(ctx, |ui| {
                let colors = self.theme_colors();
                egui::Frame::none()
                    .fill(colors.panel)
                    .stroke(Stroke::new(1.0, colors.border))
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(5.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (axis, tip) in [
                                (AlignAxis::Left, "左揃え"),
                                (AlignAxis::CenterX, "左右中央揃え"),
                                (AlignAxis::Right, "右揃え"),
                                (AlignAxis::Top, "上揃え"),
                                (AlignAxis::CenterY, "上下中央揃え"),
                                (AlignAxis::Bottom, "下揃え"),
                            ] {
                                if align_icon_button(ui, axis, tip).clicked() {
                                    clicked = Some(axis);
                                }
                            }
                        });
                    });
            });
        if let Some(axis) = clicked {
            self.align_selected_faces(axis);
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.label(egui::RichText::new(self.status.clone()).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{:.0}%", f64::from(self.view.zoom) * 100.0))
                        .small()
                        .monospace(),
                );
            });
        });
    }

    fn begin_face_edit(&mut self, face_idx: usize) {
        if face_idx >= self.doc.faces.len() {
            return;
        }
        self.face_name_snapshot = Some(self.doc.clone());
        self.face_editor = Some(face_idx);
    }

    fn commit_face_name_edit(&mut self) {
        if let Some(snap) = self.face_name_snapshot.take() {
            self.push_history_snapshot(snap);
        }
        self.face_editor = None;
    }

    fn render_layers_tab(&mut self, ui: &mut egui::Ui) {
        let shift = ui.input(|i| i.modifiers.shift);
        let mut remove_face: Option<usize> = None;

        ui.add_space(8.0);
        egui::ScrollArea::vertical().id_salt("layer-list").show(ui, |ui| {
            for i in (0..self.doc.faces.len()).rev() {
                let name = if self.doc.faces[i].name.is_empty() {
                    format!("レイヤー {}", i + 1)
                } else {
                    self.doc.faces[i].name.clone()
                };
                let selected = self.is_selected(Selection::Face(i));
                let editing = self.face_editor == Some(i);
                row_frame(ui, selected, |ui| {
                    let visible = self.doc.faces[i].visible;
                    if eye_button(ui, visible).clicked() {
                        let snapshot = self.doc.clone();
                        self.doc.faces[i].visible = !visible;
                        self.push_history_snapshot(snapshot);
                    }
                    self.face_preview_widget(ui, i, 20.0);
                    // × を右端に固定し、名前は残り幅いっぱいに広げる
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if delete_button(ui).clicked() {
                            remove_face = Some(i);
                        }
                        let w = ui.available_width();
                        if editing {
                            let resp = ui.add_sized(
                                [w, 22.0],
                                egui::TextEdit::singleline(&mut self.doc.faces[i].name),
                            );
                            if resp.lost_focus() {
                                self.commit_face_name_edit();
                            }
                            resp.request_focus();
                        } else {
                            // 選択ハイライトは行フレームが全幅で描くので、ここは
                            // 背景なしのクリック可能ラベルにする（二重の枠を避ける）
                            let txt = if selected {
                                egui::RichText::new(name).color(ui.visuals().strong_text_color())
                            } else {
                                egui::RichText::new(name)
                            };
                            let resp = ui.add_sized(
                                [w, 22.0],
                                egui::Label::new(txt).sense(Sense::click()).selectable(false),
                            );
                            if resp.clicked() {
                                if shift {
                                    self.toggle_selection(Selection::Face(i));
                                } else {
                                    self.set_single_selection(Some(Selection::Face(i)));
                                }
                            }
                            if resp.double_clicked() {
                                self.begin_face_edit(i);
                            }
                        }
                    });
                });
            }
        });
        if let Some(i) = remove_face {
            self.set_single_selection(Some(Selection::Face(i)));
            self.delete_selected();
        }

    }

    fn render_circles_tab(&mut self, ui: &mut egui::Ui) {
        let shift = ui.input(|i| i.modifiers.shift);
        ui.add_space(2.0);
        let mut remove_custom: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("custom-list").show(ui, |ui| {
            for i in 0..self.doc.custom_pegs.len() {
                let id = self.doc.custom_pegs[i].id;
                let visible = self.doc.custom_pegs[i].visible;
                let selected = self.is_selected(Selection::Peg(id));
                let used = self.custom_peg_in_use(id);
                row_frame(ui, selected, |ui| {
                    if eye_button(ui, visible).clicked() {
                        let snapshot = self.doc.clone();
                        self.doc.custom_pegs[i].visible = !visible;
                        self.push_history_snapshot(snapshot);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if used {
                            delete_button(ui).on_hover_text("この円は面に使われています");
                        } else if delete_button(ui).clicked() {
                            remove_custom = Some(i);
                        }
                        let w = ui.available_width();
                        let txt = if selected {
                            egui::RichText::new(format!("円 #{id}"))
                                .color(ui.visuals().strong_text_color())
                        } else {
                            egui::RichText::new(format!("円 #{id}"))
                        };
                        let resp = ui.add_sized(
                            [w, 22.0],
                            egui::Label::new(txt).sense(Sense::click()).selectable(false),
                        );
                        if resp.clicked() {
                            if shift {
                                self.toggle_selection(Selection::Peg(id));
                            } else {
                                self.set_single_selection(Some(Selection::Peg(id)));
                            }
                        }
                    });
                });
            }
        });
        if let Some(i) = remove_custom {
            if let Some(peg) = self.doc.custom_pegs.get(i) {
                self.set_single_selection(Some(Selection::Peg(peg.id)));
            }
            self.delete_selected();
        }
    }

    fn render_properties_tab(&mut self, ui: &mut egui::Ui) {
        let face_indices: Vec<_> = self
            .selection
            .iter()
            .filter_map(|sel| if let Selection::Face(i) = sel { Some(*i) } else { None })
            .collect();
        let peg_indices: Vec<_> = self
            .selection
            .iter()
            .filter_map(|sel| if let Selection::Peg(id) = sel { Some(*id) } else { None })
            .collect();

        ui.add_space(2.0);
        if let Some(peg_id) = peg_indices.first().copied() {
            let Some(peg_idx) = self.doc.custom_pegs.iter().position(|peg| peg.id == peg_id) else {
                return;
            };
            section_header(ui, &format!("任意円 #{peg_id}"));
            let mut x = self.doc.custom_pegs[peg_idx].center.x;
            let mut y = self.doc.custom_pegs[peg_idx].center.y;
            let mut r = self.doc.custom_pegs[peg_idx].radius;
            let x_resp = scrub_value(ui, "X", &mut x, 0.5, -5000.0..=5000.0);
            let y_resp = scrub_value(ui, "Y", &mut y, 0.5, -5000.0..=5000.0);
            let r_resp = scrub_value(ui, "半径", &mut r, 0.5, 0.0..=120.0);

            if x_resp.changed() || y_resp.changed() || r_resp.changed() {
                let candidate_center = Point::new(x, y);
                if self.can_place_custom_peg(candidate_center, r, &[peg_id]) {
                    if self.radius_edit_orig.is_none() {
                        self.radius_edit_orig = Some(peg_id);
                        self.drag_snapshot = Some(self.doc.clone());
                    }
                    self.doc.custom_pegs[peg_idx].center = candidate_center;
                    self.doc.custom_pegs[peg_idx].radius = r;
                    self.rebuild_pegs(self.canvas_size);
                } else {
                    self.status = "円同士が重なる位置・半径には変更できません".to_owned();
                }
            }
            if (x_resp.lost_focus()
                || y_resp.lost_focus()
                || r_resp.lost_focus()
                || r_resp.drag_stopped())
                && self.radius_edit_orig.is_some()
            {
                if let Some(rid) = self.radius_edit_orig.take() {
                    if rid == peg_id {
                        if let Some(snap) = self.drag_snapshot.take() {
                            self.push_history_snapshot(snap);
                            self.status = "任意円を変更しました".to_owned();
                        }
                    }
                }
            }
            return;
        }

        if face_indices.is_empty() {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("選択なし").weak());
            });
            return;
        }
        if face_indices.len() > 1 {
            section_header(ui, &format!("レイヤー {}件", face_indices.len()));
            ui.horizontal(|ui| {
                if ui.button("すべて表示").clicked() {
                    let snapshot = self.doc.clone();
                    let mut changed = false;
                    for &face_idx in &face_indices {
                        if let Some(face) = self.doc.faces.get_mut(face_idx) {
                            changed |= !face.visible;
                            face.visible = true;
                        }
                    }
                    if changed { self.push_history_snapshot(snapshot); }
                    self.status = "選択中のレイヤーを表示".to_owned();
                }
                if ui.button("すべて非表示").clicked() {
                    let snapshot = self.doc.clone();
                    let mut changed = false;
                    for &face_idx in &face_indices {
                        if let Some(face) = self.doc.faces.get_mut(face_idx) {
                            changed |= face.visible;
                            face.visible = false;
                        }
                    }
                    if changed { self.push_history_snapshot(snapshot); }
                    self.status = "選択中のレイヤーを非表示".to_owned();
                }
            });
            return;
        }

        if let Some(&face_idx) = face_indices.first() {
            if face_idx >= self.doc.faces.len() {
                return;
            }
            let title = match self.doc.faces.get(face_idx) {
                Some(f) if !f.name.is_empty() => f.name.clone(),
                _ => format!("レイヤー {}", face_idx + 1),
            };
            section_header(ui, &title);
            // 名前（インライン編集）
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("名前").weak().small());
                if let Some(face) = self.doc.faces.get_mut(face_idx) {
                    ui.add(
                        egui::TextEdit::singleline(&mut face.name)
                            .hint_text("（無題）")
                            .desired_width(ui.available_width()),
                    );
                }
            });
            let visibility_snapshot = self.doc.clone();
            if let Some(face) = self.doc.faces.get_mut(face_idx) {
                if ui.checkbox(&mut face.visible, "表示").changed() {
                    self.push_history_snapshot(visibility_snapshot);
                }
            }
            // マージ済みレイヤーはベベル量を非破壊で調整できる
            let is_baked = self
                .doc
                .faces
                .get(face_idx)
                .map(|f| f.baked.is_some())
                .unwrap_or(false);
            if is_baked {
                ui.add_space(6.0);
                let mut bevel = self.doc.faces[face_idx].baked.as_ref().unwrap().bevel;
                let resp = ui.add(egui::Slider::new(&mut bevel, 0.0..=40.0).text("ベベル"));
                if resp.changed() {
                    if self.bevel_edit_orig.is_none() {
                        self.bevel_edit_orig = Some(self.doc.clone());
                    }
                    if let Some(b) = self.doc.faces[face_idx].baked.as_mut() {
                        b.bevel = bevel;
                    }
                }
                if resp.drag_stopped() {
                    if let Some(snap) = self.bevel_edit_orig.take() {
                        self.push_history_snapshot(snap);
                    }
                }
            }
        }
    }

    fn render_grid_tab(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme_colors();
        ui.vertical(|ui| {
            let before_doc = self.doc.clone();
            let before_grid = self.doc.grid;
            ui.add(egui::Slider::new(&mut self.doc.grid.spacing, 40.0..=320.0).text("間隔"));
            ui.add(egui::Slider::new(&mut self.doc.grid.catch, 0.0..=24.0).text("引っかかり範囲"));
            let (main_max, _) = safe_grid_radius_limits(&self.doc.grid);
            ui.add(egui::Slider::new(&mut self.doc.grid.radius, 0.0..=main_max).text("半径"));
            if ENABLE_EXPERIMENTAL_SQUARE_PEGS {
                ui.horizontal(|ui| {
                    ui.label("ピン形状");
                    ui.radio_value(&mut self.doc.peg_appearance.shape, crate::model::PegShape::Circle, "円");
                    ui.radio_value(&mut self.doc.peg_appearance.shape, crate::model::PegShape::RoundedSquare, "四角");
                });
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    ui.add(egui::Slider::new(&mut self.doc.peg_appearance.rounding, 0.0..=100.0).text("角のラウンド"));
                }
            }
            ui.checkbox(&mut self.doc.grid.subs_enabled, "サブ円を使う");
            if !self.doc.disabled_pegs.is_empty() {
                let count = self.doc.disabled_pegs.len();
                if ui.button(format!("無効化した円をすべて戻す ({count})")).clicked() {
                    self.push_history();
                    self.doc.disabled_pegs.clear();
                    self.rebuild_pegs(self.canvas_size);
                    self.status = "無効化したグリッド円を戻しました".to_owned();
                }
            }
            ui.label(egui::RichText::new("サブ円の半径").color(colors.text2));
            let (_, sub_max) = safe_grid_radius_limits(&self.doc.grid);
            ui.add(egui::Slider::new(&mut self.doc.grid.sub_radius, 0.0..=sub_max).text("サブ"));
            if self.doc.grid.subs_enabled {
                ui.label(
                    egui::RichText::new(format!("重ならない上限: {sub_max:.1}"))
                        .small()
                        .color(colors.text2),
                );
            }
            ui.checkbox(&mut self.doc.grid.scale_link, "間隔に合わせて円の大きさも変える");
            ui.label("拡縮の基準");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.doc.grid.scale_anchor, ScaleAnchor::TopLeft, "左上");
                ui.radio_value(&mut self.doc.grid.scale_anchor, ScaleAnchor::Center, "中央");
            });
            if self.doc.grid != before_grid && self.draw.is_some() {
                self.draw = None;
                self.status = "グリッド変更のため描画中の線をキャンセルしました".to_owned();
            }
            if self.doc.grid.scale_link && before_grid.spacing != 0.0 && self.doc.grid.spacing != before_grid.spacing {
                let scale = self.doc.grid.spacing / before_grid.spacing;
                self.doc.grid.radius = (before_grid.radius * scale).clamp(0.0, 100.0);
                self.doc.grid.sub_radius = (before_grid.sub_radius * scale).clamp(0.0, 60.0);
            }
            if self.enforce_safe_grid_radii() {
                self.status = "円同士が重ならない安全な半径へ自動調整しました".to_owned();
            }
            if self.doc.grid != before_grid && self.grid_edit_orig.is_none() {
                self.grid_edit_orig = Some(before_doc);
            }
            if self.grid_edit_orig.is_some() && !ui.input(|input| input.pointer.primary_down()) {
                if let Some(snapshot) = self.grid_edit_orig.take() {
                    self.push_history_snapshot(snapshot);
                }
            }
            ui.add_space(8.0);
            ui.separator();
            if ui.button("グリフをクリア").clicked() {
                self.clear_glyph();
            }
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let rect = response.rect;
        let origin = rect.min;
        let mut did_view_change = false;
        let smooth_scroll = response.ctx.input(|i| i.smooth_scroll_delta);
        let zoom_delta = response.ctx.input(|i| i.zoom_delta());
        let is_space_drag = response.ctx.input(|i| i.key_down(Key::Space));
        let is_command = response.ctx.input(|i| i.modifiers.command);
        self.canvas_size = rect.size();
        self.canvas_center = Point::new(self.canvas_size.x as f64 / 2.0, self.canvas_size.y as f64 / 2.0);

        self.rebuild_pegs(rect.size());
        let mut rendered = self.materialized_faces();
        let hover = response
            .hover_pos()
            .map(|p| self.to_world(origin, p));

        if is_space_drag && response.dragged() {
            did_view_change = self.pan_view(response.drag_delta()) || did_view_change;
        }
        let can_zoom = is_command || (zoom_delta - 1.0).abs() > f32::EPSILON;
        if can_zoom {
            if let Some(cursor) = response.hover_pos() {
                did_view_change = self.set_zoom(origin, cursor, self.view.zoom * zoom_delta) || did_view_change;
            }
        }
        if !can_zoom && smooth_scroll != Vec2::ZERO {
            did_view_change = self.pan_view(smooth_scroll) || did_view_change;
        }

        let colors = self.theme_colors();

        // --- G グラブ移動（Blender風）: 選択中の任意円をカーソルで動かし、
        //     クリックで確定 / Esc・右クリックで取消。グラブ中は通常入力を止める ---
        if self.grab.is_none()
            && !ui.ctx().wants_keyboard_input()
            && self.draw.is_none()
            && ui.input(|i| i.key_pressed(Key::G))
        {
            let ids = self.selected_pegs();
            if let Some(anchor) = hover {
                if !ids.is_empty() {
                    let origins: Vec<(u64, Point)> = ids
                        .iter()
                        .filter_map(|id| {
                            self.doc
                                .custom_pegs
                                .iter()
                                .find(|p| p.id == *id)
                                .map(|p| (*id, p.center))
                        })
                        .collect();
                    self.grab = Some(GrabState { base: self.doc.clone(), origins, anchor });
                    self.status = "移動中… クリックで確定 / Escで取消".to_owned();
                } else {
                    self.status = "移動する任意円を選んでください（G）".to_owned();
                }
            }
        }
        let grabbing = self.grab.is_some();
        if grabbing {
            let cancel =
                ui.input(|i| i.key_pressed(Key::Escape)) || response.secondary_clicked();
            let confirm = response.clicked() || ui.input(|i| i.key_pressed(Key::Enter));
            if let (Some(grab), Some(pos)) = (self.grab.as_ref(), hover) {
                let delta = pos - grab.anchor;
                let moves: Vec<(u64, Point, f64)> = grab
                    .origins
                    .iter()
                    .filter_map(|(id, origin)| {
                        self.doc.custom_pegs.iter().find(|peg| peg.id == *id)
                            .map(|peg| (*id, *origin + delta, peg.radius))
                    })
                    .collect();
                let moving_ids: Vec<u64> = moves.iter().map(|(id, _, _)| *id).collect();
                if self.can_place_custom_pegs(&moves, &moving_ids) {
                    for (id, np, _) in moves {
                        if let Some(p) = self.doc.custom_pegs.iter_mut().find(|p| p.id == id) {
                            p.center = np;
                        }
                    }
                    self.rebuild_pegs(self.canvas_size);
                }
            }
            if cancel {
                if let Some(grab) = self.grab.take() {
                    self.doc = grab.base;
                    self.rebuild_pegs(self.canvas_size);
                    self.status = "移動を取り消しました".to_owned();
                }
            } else if confirm {
                if let Some(grab) = self.grab.take() {
                    self.push_history_snapshot(grab.base);
                    self.status = "任意円を移動しました".to_owned();
                }
            }
        }

        if self.tool != Tool::Draw && self.draw.is_some() {
            self.draw = None;
            self.status = "描画モードを切り替えたため描画中の線をキャンセルしました".to_owned();
        }
        if self.tool != Tool::Select && self.tool != Tool::DirectSelect {
            self.cancel_active_manipulation();
        }

        if !grabbing && (ui.input(|i| i.key_pressed(Key::Escape)) || response.secondary_clicked()) {
            if self.draw.take().is_some() {
                self.status = "描画をキャンセルしました".to_owned();
            } else if self.cancel_active_manipulation() {
                self.status = "移動を取り消しました".to_owned();
            } else {
                self.clear_selection();
            }
            self.radius_edit_orig = None;
            self.drag_snapshot = None;
        }

        let mut active_preview: Option<Vec<Seg>> = None;
        let mut active_preview_is_closed = false;
        let mut active_close_allowed = false;
        let mut active_has_penetration = false;
        let mut request_pin: Option<Point> = None;
        let mut request_finish = false;
        let mut request_finish_face: Option<(Face, Vec<Seg>)> = None;
        let mut active_status: Option<String> = None;
        let mut rebuild_pegs = false;

        let peg_refs = self.peg_refs.clone();
        let resolved_pegs = self.pegs.clone();
        let grid = self.doc.grid;
        let customs = self.doc.custom_pegs.clone();
        let canvas_center = self.canvas_center;
        let appearance = self.doc.peg_appearance;
        let mut live_snapshot: Option<(StartAnchor, Point, bool, Thread, Vec<Element>, Vec<usize>)> = None;
        let mut pin_request = false;

        let resolver = move |peg_ref: &PegRef| -> Option<Peg> {
            resolve_peg(&grid, &customs, canvas_center, appearance, peg_ref)
        };

        if !grabbing && response.drag_started() {
            if let Some(pos) = hover {
                if self.tool == Tool::Select && self.peg_at_cursor(pos).is_none() && !is_space_drag {
                    // 面のスナップ移動を開始
                    if let Some(Selection::Face(hit)) = self.hit_test_face(pos, &rendered) {
                        let shift = ui.input(|i| i.modifiers.shift);
                        self.apply_click_selection(Selection::Face(hit), shift);
                        let indices: Vec<usize> = if self.is_selected(Selection::Face(hit)) {
                            self.selection
                                .iter()
                                .filter_map(|sel| match sel {
                                    Selection::Face(i) => Some(*i),
                                    _ => None,
                                })
                                .collect()
                        } else {
                            vec![hit]
                        };
                        if !indices.is_empty() {
                            self.face_drag = Some(ActiveFaceDrag {
                                base: self.doc.clone(),
                                anchor: pos,
                                indices,
                                applied: (0, 0),
                            });
                        }
                    }
                }
                if let Some((peg_ref, pos_ref)) = self.peg_at_cursor(pos) {
                    let radius = self
                        .peg_refs
                        .iter()
                        .zip(self.pegs.iter())
                        .find_map(|(pr, peg)| {
                            (peg_ref == *pr).then_some((peg.radius - self.doc.grid.catch).max(0.0))
                        });
                    if let Some(radius) = radius {
                        let shift = ui.input(|i| i.modifiers.shift);
                        match (self.tool, peg_ref) {
                        (Tool::Select, PegRef::Custom { id }) => {
                            if !self.drag_snapshot.is_some() {
                                self.drag_snapshot = Some(self.doc.clone());
                            }
                            let sel = Selection::Peg(id);
                            self.apply_click_selection(sel, shift);
                            let ids = if self.is_selected(sel) {
                                self.selected_pegs()
                            } else {
                                vec![id]
                            };
                            self.begin_drag(&ids, pos, false);
                        }
                        (Tool::DirectSelect, PegRef::Custom { id }) => {
                            if !self.drag_snapshot.is_some() {
                                self.drag_snapshot = Some(self.doc.clone());
                            }
                            let sel = Selection::Peg(id);
                            self.apply_click_selection(sel, false);
                            let ids = if self.is_selected(sel) {
                                self.selected_pegs()
                            } else {
                                vec![id]
                            };
                            self.begin_drag(&ids, pos, false);
                        }
                        (Tool::DirectSelect, source) => {
                            // 参照判定は変換「前」に行う。変換後は参照が新しい
                            // Custom id に置換されるため、後から判定すると常に
                            // false になりドラッグが成立しない
                            if self.face_references_source(&source) {
                                if self.drag_snapshot.is_none() {
                                    self.drag_snapshot = Some(self.doc.clone());
                                }
                                self.set_single_selection(Some(Selection::PegRef(source)));
                                let id = self.direct_select_drag_target(source, pos_ref, radius);
                                self.set_single_selection(Some(Selection::Peg(id)));
                                self.begin_drag(&[id], pos, true);
                                self.rebuild_pegs(self.canvas_size);
                            } else {
                                self.set_single_selection(Some(Selection::PegRef(source)));
                                self.status =
                                    "この円は面に使われていません（面が参照する円のみ動かせます）".to_owned();
                            }
                        }
                        _ => {}
                        }
                    }
                }
            }
        }

        if let (Some(fd), Some(pos)) = (self.face_drag.as_ref(), hover) {
            let spacing = self.doc.grid.spacing.max(1.0);
            let dcol = ((pos.x - fd.anchor.x) / spacing).round() as i32;
            let drow = ((pos.y - fd.anchor.y) / spacing).round() as i32;
            if (drow, dcol) != fd.applied {
                let base = fd.base.clone();
                let indices = fd.indices.clone();
                self.apply_face_translation(&base, &indices, drow, dcol);
                if let Some(fd) = self.face_drag.as_mut() {
                    fd.applied = (drow, dcol);
                }
                rebuild_pegs = true;
            }
        }
        if response.drag_stopped() {
            if let Some(fd) = self.face_drag.take() {
                if fd.applied != (0, 0) {
                    self.push_history_snapshot(fd.base);
                    self.status = "レイヤーを移動しました".to_owned();
                }
            }
        }

        if let (Some(drag), Some(pos)) = (self.active_custom_drag.as_ref(), hover) {
            {
                let ActiveDrag::Custom { start, anchor, .. } = drag;
                let delta = pos - *anchor;
                let mut moved = Vec::with_capacity(start.len());
                for (id, origin) in start.iter() {
                    if let Some(radius) = self
                        .doc
                        .custom_pegs
                        .iter()
                        .find(|peg| peg.id == *id)
                        .map(|peg| peg.radius)
                    {
                        moved.push((
                            *id,
                            Point {
                                x: origin.x + delta.x,
                                y: origin.y + delta.y,
                            },
                            radius,
                        ));
                    }
                }
                let moving_ids: Vec<u64> = moved.iter().map(|(id, ..)| *id).collect();
                let can_move = if moved.is_empty() {
                    false
                } else {
                    self.can_place_custom_pegs(&moved, &moving_ids)
                };
                if can_move {
                    let mut changed = false;
                    for (id, origin) in start.iter() {
                        if let Some(peg) = self.doc.custom_pegs.iter_mut().find(|peg| peg.id == *id) {
                            let target = Point {
                                x: origin.x + delta.x,
                                y: origin.y + delta.y,
                            };
                            if peg.center != target {
                                changed = true;
                                peg.center = target;
                            }
                        }
                    }
                    if changed {
                        rebuild_pegs = true;
                    }
                }
            }
        }

        if response.drag_stopped() {
            if let Some(drag) = self.active_custom_drag.take() {
                let changed = match drag {
                    ActiveDrag::Custom { start, converted, .. } => converted || start.iter().any(|(id, start_center)| {
                        self.doc
                            .custom_pegs
                            .iter()
                            .any(|peg| peg.id == *id && peg.center != *start_center)
                    }),
                };
                if changed {
                    if let Some(snap) = self.drag_snapshot.take() {
                        self.push_history_snapshot(snap);
                        self.status = if self.tool == Tool::DirectSelect {
                            "直接選択で編集しました".to_owned()
                        } else {
                            "任意円を移動しました".to_owned()
                        };
                    }
                } else {
                    // 動かなかった場合は古いスナップショットを残さない
                    self.drag_snapshot = None;
                }
            }
        }
        if did_view_change {
            response.ctx.request_repaint();
        }
        let overlay_timer = response.ctx.input(|i| i.unstable_dt);
        if did_view_change {
            self.view_overlay_timer = 1.0;
        } else if self.view_overlay_timer > 0.0 {
            self.view_overlay_timer = (self.view_overlay_timer - overlay_timer).max(0.0);
        }

        let close_radius = self.world_unit(CLOSE_RADIUS);
        if let Some(active) = self.draw.as_mut() {
            if let (Some(cursor), Some(guard)) = (hover, active.undo_guard) {
                let clear = resolved_pegs
                    .get(guard)
                    .is_none_or(|peg| peg_signed_distance(peg, cursor) > 1.0);
                if clear {
                    active.undo_guard = None;
                }
            }
            let mut capture_ignored = active.ignored.clone();
            if let Some(guard) = active.undo_guard {
                if !capture_ignored.contains(&guard) {
                    capture_ignored.push(guard);
                }
            }
            // Command を離した瞬間: そのとき貫通していた円を無視確定する。
            // （Pin を打たなくても「突っ切った」＝無視の意思表示として扱う）
            if active.cmd_was_down && !is_command {
                if active.suppress_cmd_ignore_until_release {
                    active.suppress_cmd_ignore_until_release = false;
                } else {
                    let head = active.th.head(&resolved_pegs);
                    let exclude = active.th.wraps.last().map(|w| w.peg);
                    let pierced = pegs_penetrated_by_line(
                        head,
                        active.th.tip,
                        &resolved_pegs,
                        exclude,
                        &active.ignored,
                    );
                    if !pierced.is_empty() {
                        for idx in pierced {
                            if !active.ignored.contains(&idx) {
                                active.ignored.push(idx);
                            }
                        }
                        self.status = "突っ切った円を無視として確定しました".to_owned();
                    }
                }
            }
            active.cmd_was_down = is_command;
            if let Some(cursor) = hover {
                if is_command {
                    // Command 押下中はソルバー停止（円を無視した直線プレビュー）。
                    // ここでソルバーを動かすと円に巻き付いてしまい、貫通が
                    // 発生せず「無視の確定」が機能しない
                    active.th.tip = clamp_outside_pegs(cursor, &resolved_pegs);
                } else {
                    // 無視を確定した円はストローク終了まで捕捉対象から外す
                    update_thread_ignoring(&mut active.th, cursor, &resolved_pegs, &capture_ignored);
                }
            }
            if active.th.tip.distance(active.start_point) > close_radius * 2.0 {
                active.armed = true;
            }
            live_snapshot = Some((
                active.start,
                active.start_point,
                active.armed,
                active.th.clone(),
                active.elements.clone(),
                active.ignored.clone(),
            ));
        } else if !grabbing
            && (response.clicked()
                || (self.tool == Tool::Draw
                    && response.drag_started()
                    && !is_space_drag
                    && !is_command))
        {
            if let Some(pos) = hover {
                match self.tool {
                    Tool::Draw => {
                        // クリックでもドラッグ開始でも、十字点から線を引き始められる
                        if let Some((start_point, start)) = self.pick_start_anchor(pos) {
                            self.draw = Some(ActiveDraw {
                                th: Thread::new(start_point),
                                start,
                                start_point,
                                elements: Vec::new(),
                                armed: false,
                                ignored: Vec::new(),
                                undo_guard: None,
                                suppress_cmd_ignore_until_release: false,
                                cmd_was_down: false,
                            });
                            self.clear_selection();
                            self.status = "描画中… 始点に戻ってクリックで閉じます".to_owned();
                        } else {
                            self.status = "開始点は円の十字点から選んでください".to_owned();
                        }
                    }
                    Tool::Select => {
                        let shift = ui.input(|i| i.modifiers.shift);
                        let hit = self.hit_test_face(pos, &rendered);
                        if let Some(hit) = hit {
                            self.apply_click_selection(hit, shift);
                            self.status = match hit {
                                Selection::Face(i) => format!("レイヤー {} を選択しました", i + 1),
                                Selection::Peg(_) => "任意円を選択しました".to_owned(),
                                Selection::PegRef(_) => "ペグを選択しました".to_owned(),
                            };
                        } else if !shift {
                            self.clear_selection();
                            self.status = "選択していません".to_owned();
                        }
                    }
                    Tool::DirectSelect => {
                        if let Some((peg_ref, _)) = self.peg_at_cursor(pos) {
                            let sel = match peg_ref {
                                PegRef::Custom { id } => Selection::Peg(id),
                                _ => Selection::PegRef(peg_ref),
                            };
                            self.apply_click_selection(sel, false);
                            self.status = match sel {
                                Selection::Peg(_) => "任意円を選択しました".to_owned(),
                                Selection::PegRef(_) => "ペグを選択しました".to_owned(),
                                _ => "レイヤーを選択しました".to_owned(),
                            };
                        } else {
                            self.clear_selection();
                            self.status = "選択していません".to_owned();
                        }
                    }
                    Tool::CustomPeg => {
                        if self.can_place_custom_peg(pos, self.custom_radius, &[]) {
                            self.push_history();
                            let id = self.doc.next_peg_id;
                            self.doc.custom_pegs.push(CustomPeg {
                                id,
                                center: pos,
                                radius: self.custom_radius,
                                visible: true,
                            });
                            self.doc.next_peg_id = self.next_peg_id();
                            self.status = "任意円を配置しました".to_owned();
                            self.clear_selection();
                            self.rebuild_pegs(self.canvas_size);
                        } else {
                            self.status = "この位置には配置できません（重なり）".to_owned();
                        };
                    }
                }
            }
        }

        if let Some((start, start_point, armed, th, base_elements, ignored)) = live_snapshot {
            let base_is_empty = base_elements.is_empty();
            let live_preview =
                thread_to_wrap_elements(&th, &resolved_pegs, &peg_refs, th.tip);
            let live_closed = thread_to_wrap_elements(&th, &resolved_pegs, &peg_refs, start_point);
            if let (Some(lp), Some(lc)) = (live_preview, live_closed) {
                let mut preview_elements = base_elements.clone();
                preview_elements.extend(lp);
                // プレビューは開いた経路（始点→…→先端）。閉じ線は描画しない
                active_preview =
                    materialize_chain(start_point, &preview_elements, &resolver, th.tip);

                let mut elements = base_elements.clone();
                elements.extend(lc);
                let trailing_start_wrap = th.wraps.last().filter(|wrap| {
                    peg_refs.get(wrap.peg).is_some_and(|peg_ref| *peg_ref == start.peg)
                });
                let preferred_start_s = if matches!(elements.last(), Some(Element::Wrap { peg, .. }) if *peg == start.peg) {
                    None
                } else {
                    let head = th.head(&resolved_pegs);
                    resolver(&start.peg).map(|start_peg| {
                        let d = start_point - head;
                        let to_center = start_peg.center - head;
                        if d.cross(to_center) >= 0.0 { 1.0 } else { -1.0 }
                    })
                };
                let mut variants = Vec::new();
                if let Some(s) = preferred_start_s {
                    for start_s in [s, -s] {
                        let mut candidate = elements.clone();
                        candidate.push(Element::Wrap {
                            peg: start.peg,
                            s: start_s,
                            turns: 0,
                            exit_offset: 0.0,
                            sweep_hint: std::f64::consts::PI,
                        });
                        variants.push(candidate);
                    }
                } else {
                    variants.push(elements.clone());
                    if let Some(Element::Wrap { s, .. }) = elements.last_mut() {
                        *s = -*s;
                        variants.push(elements.clone());
                    }
                }
                // 局所捕捉の巻き方向が閉路では交差接線になる場合に備え、
                // 各接触を1つずつ反転した候補も近い順に評価する。
                let seed_variants = variants.clone();
                if let Some(seed) = seed_variants.first() {
                    for index in 0..seed.len() {
                        if let Element::Wrap { .. } = seed[index] {
                            let mut candidate = seed.clone();
                            if let Element::Wrap { s, .. } = &mut candidate[index] {
                                *s = -*s;
                            }
                            variants.push(candidate);
                        }
                    }
                }

                let ignored_refs: Vec<PegRef> = ignored
                    .iter()
                    .filter_map(|index| peg_refs.get(*index).copied())
                    .collect();
                let obstacles: Vec<(PegRef, Peg)> = peg_refs
                    .iter()
                    .copied()
                    .zip(resolved_pegs.iter().copied())
                    .filter(|(peg_ref, _)| !ignored_refs.contains(peg_ref))
                    .collect();
                let obstacle_pegs: Vec<Peg> = obstacles.iter().map(|(_, peg)| *peg).collect();
                let mut first_candidate = None;
                let mut chosen = None;
                for candidate_elements in variants {
                    let face = Face {
                        start,
                        elements: candidate_elements,
                        visible: true,
                        name: String::new(),
                        baked: None,
                        ignored_pegs: ignored_refs.clone(),
                    };
                    let Some(segs) = materialize_face_with_obstacles(&face, &resolver, &obstacles) else { continue };
                    if first_candidate.is_none() {
                        first_candidate = Some((face.clone(), segs.clone()));
                    }
                    let start_peg = resolver(&start.peg);
                    let start_sweep = start_peg.and_then(|peg| {
                        segs.iter().find_map(|segment| match *segment {
                            Seg::Arc { center, start, end, .. }
                                if center.distance(peg.center) < 1e-8 => Some((end - start).abs()),
                            _ => None,
                        })
                    });
                    if start_sweep.is_some_and(|sweep| sweep <= MAX_AUTO_CLOSE_SWEEP)
                        && !self.has_penetration_on_lines(&segs)
                        && !segments_penetrate_pegs(&segs, &obstacle_pegs)
                    {
                        chosen = Some((face, segs));
                        break;
                    }
                }
                // 円へ自然に閉じる候補が90°を超えて回り込む場合だけ、開始点を
                // 結び目にした候補へフォールバックする。「末尾が開始円ではない」
                // だけで Pin 化すると通常の閉路まで再計算不能になるため、実際に
                // 解いた開始円弧の掃引量を基準にする。
                if chosen.is_none() {
                    let mut anchored_thread = th.clone();
                    if trailing_start_wrap.is_some() {
                        anchored_thread.wraps.pop();
                    }
                    if let Some(anchor_wraps) = thread_to_wrap_elements(
                        &anchored_thread,
                        &resolved_pegs,
                        &peg_refs,
                        start_point,
                    ) {
                        let mut anchored = base_elements.clone();
                        anchored.extend(anchor_wraps);
                        anchored.push(Element::Pin { pos: start_point });
                        let face = Face {
                            start,
                            elements: anchored,
                            visible: true,
                            name: String::new(),
                            baked: None,
                            ignored_pegs: ignored_refs.clone(),
                        };
                        if let Some(segs) = materialize_face_with_obstacles(&face, &resolver, &obstacles) {
                            if !self.has_penetration_on_lines(&segs)
                                && !segments_penetrate_pegs(&segs, &obstacle_pegs)
                            {
                                chosen = Some((face, segs));
                            }
                        }
                    }
                }
                if let Some((face, segs)) = chosen.or(first_candidate) {
                    let over_wrap = th.wraps.iter().any(|wrap| {
                        ((wrap.exit - wrap.entry) * wrap.s).abs() > std::f64::consts::PI * 4.0
                    });
                    active_has_penetration = over_wrap
                        || self.has_penetration_on_lines(&segs)
                        || segments_penetrate_pegs(&segs, &obstacle_pegs);
                    let in_close_radius = th.tip.distance(start_point) <= self.world_unit(CLOSE_RADIUS);
                    active_close_allowed = armed
                        && !segs.is_empty()
                        && !face.elements.is_empty()
                        && in_close_radius
                        && !active_has_penetration;
                    // 始点付近では、赤/緑判定と確定後に使う同一の閉路を表示する。
                    if in_close_radius {
                        active_preview = Some(segs.clone());
                        active_preview_is_closed = true;
                    }
                    request_finish_face = Some((face, segs));
                }
            }

            if response.clicked() {
                if is_command {
                    pin_request = true;
                } else if active_close_allowed {
                    request_finish = true;
                } else if th.wraps.is_empty() && base_is_empty {
                            active_status = Some("ペグに引っかけてから始点に戻ると閉じられます".to_owned());
                        } else if active_has_penetration {
                            active_status = Some("輪郭が自己交差しています".to_owned());
                        } else if !armed {
                            active_status = Some("少し離れてから始点に戻ってください".to_owned());
                        } else {
                            active_status = Some("現在の輪郭は閉じられない状態です".to_owned());
                        }
            }
        }

        if let Some(status) = active_status {
            self.status = status;
        }

        if pin_request {
            request_pin = hover.map(|pos| clamp_outside_pegs(pos, &resolved_pegs));
        }
        if request_finish {
            if let Some(active) = self.draw.take() {
                if let Some((face, segs)) = request_finish_face {
                    if face.elements.is_empty() {
                        self.status = "面の確定に失敗しました（参照先が失われています）".to_owned();
                        self.draw = Some(active);
                    } else if segs.is_empty() {
                        self.status = "面の確定に失敗しました（参照先が失われています）".to_owned();
                        self.draw = Some(active);
                    } else if active_has_penetration {
                        self.status = "現在の輪郭は閉じられない状態です".to_owned();
                        self.draw = Some(active);
                    } else {
                        let mut pts = Vec::new();
                        self.flatten_segments_for_appearance(&segs, &mut pts, 0.4);
                        if pts.len() < 3 {
                            self.status = "面積がないため確定できません".to_owned();
                            self.draw = Some(active);
                        } else {
                            let area = crate::geometry::signed_area(&pts);
                            if area.abs() < 1e-9 {
                                self.status = "面積がないため確定できません".to_owned();
                                self.draw = Some(active);
                            } else {
                                // 向きの正規化は描画/SVG出力時に行う（把持オフセット付きの
                                // パラメトリック表現は要素反転では同一曲線にならないため）
                                self.push_history();
                                self.doc.faces.push(face);
                                self.clear_selection();
                                self.draw = None;
                                self.status = "外周線を閉じてレイヤーを確定しました".to_owned();
                            }
                        }
                    }
                } else {
                    self.draw = Some(active);
                }
            }
        }
        if let Some(pos) = request_pin {
            if let Some(mut active) = self.draw.take() {
                let head = active.th.head(&resolved_pegs);
                let exclude = active.th.wraps.last().map(|w| w.peg);
                for idx in pegs_penetrated_by_line(
                    head,
                    pos,
                    &resolved_pegs,
                    exclude,
                    &active.ignored,
                ) {
                    if !active.ignored.contains(&idx) {
                        active.ignored.push(idx);
                    }
                }
                if let Some(mut elements) = self.wrap_elements_from_thread(&active.th, pos) {
                    active.elements.append(&mut elements);
                    active.elements.push(Element::Pin { pos });
                    active.th = Thread::new(pos);
                    active.armed = true;
                    self.draw = Some(active);
                    self.status = "Pin を追加しました".to_owned();
                } else {
                    self.draw = Some(active);
                }
            }
        }

        if rebuild_pegs {
            self.rebuild_pegs(self.canvas_size);
            rendered = self.materialized_faces();
        }

        if ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
            self.delete_selected();
        }

        painter.rect_filled(rect, 0.0, colors.bg);

        let selected_peg_refs: Vec<PegRef> = self
            .selection
            .iter()
            .filter_map(|sel| match sel {
                Selection::PegRef(peg_ref) => Some(*peg_ref),
                _ => None,
            })
            .collect();
        let selected_faces: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|sel| match sel {
                Selection::Face(i) => Some(*i),
                _ => None,
            })
            .collect();
        let mut selected_face_indices = Vec::new();
        for (list_idx, (face_idx, _)) in rendered.iter().enumerate() {
            let face_idx = *face_idx;
            if selected_faces.contains(&face_idx) {
                selected_face_indices.push(list_idx);
            }
            for peg_ref in &selected_peg_refs {
                if let Some(face) = self.doc.faces.get(face_idx) {
                    if self.face_references_peg(face, peg_ref) {
                        selected_face_indices.push(list_idx);
                    }
                }
            }
        }

        let mut poly_points: Vec<Vec<Point>> = Vec::new();
        let mut invalid_faces = Vec::new();
        for &(face_idx, ref render) in rendered.iter() {
            // マージ済みは穴を含む全リングを塗りに渡す（NonZero）
            for ring in &render.rings {
                if ring.len() >= 3 {
                    poly_points.push(ring.clone());
                }
            }
            if render.invalid {
                invalid_faces.push(face_idx);
            }
        }
        let screen_polys: Vec<Vec<Point>> = poly_points
            .iter()
            .map(|poly| {
                poly.iter()
                    .map(|p| {
                        let sp = self.to_screen(origin, *p);
                        Point::new(f64::from(sp.x), f64::from(sp.y))
                    })
                    .collect()
            })
            .collect();
        if let Some(mesh) = build_faces_mesh(&screen_polys, Pos2::new(0.0, 0.0), colors.face_fill) {
            painter.add(mesh);
        }

        // 引っかかり範囲は糸より先に描く。同じ実効半径上の暗いガイドが
        // 円弧だけを後から上書きし、直線より細く見えていたため。
        if self.show_circles && self.doc.grid.catch > 0.5 {
            let zoom = self.view.zoom;
            for (peg, peg_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
                if let PegRef::Custom { id } = peg_ref {
                    if self.doc.custom_pegs.iter().find(|custom| custom.id == *id).is_some_and(|custom| !custom.visible) {
                        continue;
                    }
                }
                let c = self.to_screen(origin, peg.center);
                if peg.shape == crate::model::PegShape::RoundedSquare {
                    let catch_rect = Rect::from_center_size(c, Vec2::splat(peg.radius as f32 * zoom * 2.0));
                    painter.rect_stroke(catch_rect, (peg.rounding as f32 * zoom).min(peg.radius as f32 * zoom), Stroke::new(0.6, colors.catch));
                } else {
                    painter.circle_stroke(c, peg.radius as f32 * zoom, Stroke::new(0.6, colors.catch));
                }
            }
        }

        // 確定済みの面を描画
        for (list_index, &(_, ref render)) in rendered.iter().enumerate() {
            if selected_face_indices.contains(&list_index) {
                continue;
            }
            if render.points.len() >= 2 {
                let line: Vec<Pos2> = render
                    .points
                    .iter()
                    .map(|p| self.to_screen(origin, *p))
                    .collect();
                let mut line = line;
                if line.len() > 2 && line.first().zip(line.last()).is_some_and(|(a, b)| a.distance(*b) < 0.01) {
                    line.pop();
                }
                painter.add(egui::Shape::closed_line(
                    line,
                    Stroke::new(1.0, colors.thread),
                ));
            }
        }
        for face_idx in invalid_faces {
            if let Some((_, ref render)) = rendered.iter().find(|(i, _)| *i == face_idx) {
                let mut pts = Vec::with_capacity(render.points.len() + 1);
                pts.extend(render.points.iter().map(|p| self.to_screen(origin, *p)));
                if let Some(first) = pts.first().copied() {
                    pts.push(first);
                }
                painter.add(egui::Shape::line(pts, Stroke::new(1.8, colors.error)));
            }
        }

        for (list_idx, &(_, ref render)) in rendered.iter().enumerate() {
            if render.points.len() >= 2 && selected_face_indices.contains(&list_idx) {
                let line: Vec<Pos2> = render
                    .points
                    .iter()
                    .map(|p| self.to_screen(origin, *p))
                    .collect();
                let mut line = line;
                if line.len() > 2 && line.first().zip(line.last()).is_some_and(|(a, b)| a.distance(*b) < 0.01) {
                    line.pop();
                }
                painter.add(egui::Shape::closed_line(line, Stroke::new(2.2, colors.select)));
            }
        }
        for id in self.selected_pegs() {
            if let Some(p) = self.doc.custom_pegs.iter().find(|peg| peg.id == id) {
                let c = self.to_screen(origin, p.center);
                let r = p.radius as f32 * self.view.zoom + 2.0;
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    painter.rect_stroke(Rect::from_center_size(c, Vec2::splat(r * 2.0)), (self.doc.peg_appearance.rounding as f32 * self.view.zoom).min(r), Stroke::new(2.0, colors.select));
                } else {
                    painter.circle_stroke(c, r, Stroke::new(2.0, colors.select));
                }
            }
        }

        // peg.radius は実効半径（引っかかり範囲込み）。見た目の円はそこから
        // catch を引いた各ペグ固有の半径で描く（サブ円・任意円もここで正しくなる）
        if self.show_circles {
            let zoom = self.view.zoom;
            for (peg, peg_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
                if let PegRef::Custom { id } = peg_ref {
                    if self.doc.custom_pegs.iter().find(|custom| custom.id == *id).is_some_and(|custom| !custom.visible) {
                        continue;
                    }
                }
                let c = self.to_screen(origin, peg.center);
                let r_draw = ((peg.radius - self.doc.grid.catch).max(0.0) as f32) * zoom;
                // 任意円がグリッド/サブ円に被っているときは赤で警告表示
                let overlaps = matches!(peg_ref, PegRef::Custom { .. })
                    && self.custom_peg_overlaps_grid(peg);
                let stroke_color = if overlaps { colors.close_ng } else { colors.peg_stroke };
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    let peg_rect = Rect::from_center_size(c, Vec2::splat(r_draw * 2.0));
                    let rounding = (self.doc.peg_appearance.rounding as f32 * zoom).min(r_draw);
                    painter.rect_filled(peg_rect, rounding, colors.peg_fill);
                    painter.rect_stroke(peg_rect, rounding, Stroke::new(1.2, stroke_color));
                } else {
                    painter.circle(c, r_draw, colors.peg_fill, Stroke::new(1.2, stroke_color));
                }
            }
        }

        // 開始点マーカー
        if self.tool == Tool::Draw && self.draw.is_none() {
            self.draw_start_markers(hover, &painter, origin);
        }

        if let Some(active) = &self.draw {
            let mut pts = Vec::new();
            if let Some(segs) = active_preview.as_ref() {
                let mut color = colors.thread;
                let command_penetrates = if is_command {
                    self.command_line_penetrates(active)
                } else {
                    false
                };
                if is_command && command_penetrates {
                    color = colors.cmd_green;
                }
                // 確定済み区間は実線、先端の自由区間（最後の直線）は点線で描く
                let (solid, dashed_tail) = if active_preview_is_closed {
                    (segs.as_slice(), None)
                } else {
                    match segs.split_last() {
                        Some((Seg::Line { from, to }, rest)) => (rest, Some((*from, *to))),
                        _ => (segs.as_slice(), None),
                    }
                };
                let tol = (0.35 / f64::from(self.view.zoom)).max(0.01);
                self.flatten_segments_for_appearance(solid, &mut pts, tol);
                if pts.len() >= 2 {
                    let line: Vec<Pos2> = pts.iter().map(|p| self.to_screen(origin, *p)).collect();
                    painter.add(egui::Shape::line(line, Stroke::new(2.0, color)));
                }
                if let Some((from, to)) = dashed_tail {
                    let a = self.to_screen(origin, from);
                    let b = self.to_screen(origin, to);
                    painter.add(egui::Shape::dashed_line(
                        &[a, b],
                        Stroke::new(1.6, color),
                        7.0,
                        6.0,
                    ));
                }
            }
            let anchor = self.to_screen(origin, active.start_point);
            painter.circle_filled(anchor, 3.5, colors.anchor);
            if active.th.tip.distance(active.start_point) <= self.world_unit(CLOSE_RADIUS) {
                if active_close_allowed && !is_command {
                    painter.circle_stroke(
                        anchor,
                        CLOSE_RADIUS as f32,
                        Stroke::new(1.6, colors.close_ok),
                    );
                } else if !is_command && active_has_penetration {
                    painter.circle_stroke(
                        anchor,
                        CLOSE_RADIUS as f32,
                        Stroke::new(1.6, colors.close_ng),
                    );
                }
            }
        }
        if self.tool == Tool::CustomPeg {
            if let Some(p) = hover {
                let ok = self.can_place_custom_peg(p, self.custom_radius, &[]);
                let color = if ok { colors.close_ok } else { colors.close_ng };
                let preview_center = self.to_screen(origin, p);
                let preview_radius = self.custom_radius as f32 * self.view.zoom;
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    painter.rect_stroke(Rect::from_center_size(preview_center, Vec2::splat(preview_radius * 2.0)), (self.doc.peg_appearance.rounding as f32 * self.view.zoom).min(preview_radius), Stroke::new(1.5, color));
                } else {
                    painter.circle_stroke(preview_center, preview_radius, Stroke::new(1.5, color));
                }
                painter.circle_filled(self.to_screen(origin, p), 2.0, color);
            }
        }

        let minimap_alpha = response.ctx.animate_value_with_time(
            Id::new("minimap-alpha"),
            if self.view_overlay_timer > 0.0 { 1.0 } else { 0.0 },
            1.0,
        );
        if minimap_alpha > 0.01 {
            let map_rect = Rect::from_min_size(
                Pos2::new(rect.right() - 205.0, rect.bottom() - 170.0),
                Vec2::new(190.0, 150.0),
            );

            let mut face_min = Point::new(f64::INFINITY, f64::INFINITY);
            let mut face_max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
            for &(_, ref render) in &rendered {
                if render.points.len() >= 3 {
                    for p in &render.points {
                        face_min.x = face_min.x.min(p.x);
                        face_min.y = face_min.y.min(p.y);
                        face_max.x = face_max.x.max(p.x);
                        face_max.y = face_max.y.max(p.y);
                    }
                }
            }

            let bounds = if face_min.x.is_finite() && face_min.y.is_finite() {
                let pad = self.world_unit(30.0);
                (face_min.x - pad, face_min.y - pad, face_max.x + pad, face_max.y + pad)
            } else {
                let view_min = self.to_world(origin, rect.min);
                let view_max = self.to_world(origin, rect.max);
                let center_x = (view_min.x + view_max.x) * 0.5;
                let center_y = (view_min.y + view_max.y) * 0.5;
                let span_x = (view_max.x - view_min.x).abs().max(100.0) * 1.2;
                let span_y = (view_max.y - view_min.y).abs().max(100.0) * 1.2;
                (
                    center_x - span_x * 0.5,
                    center_y - span_y * 0.5,
                    center_x + span_x * 0.5,
                    center_y + span_y * 0.5,
                )
            };

            let map_w = (bounds.2 - bounds.0).abs().max(1.0);
            let map_h = (bounds.3 - bounds.1).abs().max(1.0);
            let map_scale = ((f64::from(map_rect.width()) - 12.0) / map_w)
                .min((f64::from(map_rect.height()) - 12.0) / map_h);
            let to_map = |p: Point| -> Pos2 {
                Pos2::new(
                    ((p.x - bounds.0) * map_scale + (f64::from(map_rect.min.x) + 6.0)) as f32,
                    ((p.y - bounds.1) * map_scale + (f64::from(map_rect.min.y) + 6.0)) as f32,
                )
            };

            let mut map_polys: Vec<Vec<Point>> = Vec::new();
            for &(_, ref render) in &rendered {
                if render.points.len() >= 3 {
                    let mut pts = Vec::with_capacity(render.points.len());
                    for p in &render.points {
                        let mapped = to_map(*p);
                        pts.push(Point::new(mapped.x as f64, mapped.y as f64));
                    }
                    map_polys.push(pts);
                }
            }

            let bg_alpha = (minimap_alpha * 170.0).round() as u8;
            let stroke_alpha = (minimap_alpha * 210.0).round() as u8;
            let face_alpha = (minimap_alpha * 130.0).round() as u8;
            painter.rect_filled(
                map_rect,
                6.0,
                Color32::from_rgba_unmultiplied(colors.bg.r(), colors.bg.g(), colors.bg.b(), bg_alpha),
            );
            painter.rect_stroke(
                map_rect,
                6.0,
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(colors.peg_stroke.r(), colors.peg_stroke.g(), colors.peg_stroke.b(), stroke_alpha),
                ),
            );
            if let Some(shape) =
                build_faces_mesh(&map_polys, Pos2::new(0.0, 0.0), Color32::from_rgba_unmultiplied(160, 160, 160, face_alpha))
            {
                if let egui::Shape::Mesh(mesh) = shape {
                    painter.add(egui::Shape::mesh(mesh));
                }
            }

            let view_min = self.to_world(origin, rect.min);
            let view_max = self.to_world(origin, rect.max);
            let v0 = to_map(view_min);
            let v1 = to_map(view_max);
            painter.add(egui::Shape::line(
                vec![
                    v0,
                    Pos2::new(v1.x, v0.y),
                    v1,
                    Pos2::new(v0.x, v1.y),
                    v0,
                ],
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(180, 220, 255, (minimap_alpha * 220.0).round() as u8),
                ),
            ));
        }

        // 左下: 表示倍率バー
        egui::Area::new(egui::Id::new("zoom-bar"))
            .fixed_pos(Pos2::new(rect.left() + 8.0, rect.bottom() - 34.0))
            .show(&response.ctx, |ui| {
                egui::Frame::popup(&ui.ctx().style().clone()).show(ui, |ui| {
                    ui.spacing_mut().slider_width = 120.0;
                    ui.horizontal(|ui| {
                        let mut changed = false;
                        if ui.add(egui::Button::new("−").small()).clicked() {
                            changed |= self.set_zoom(origin, rect.center(), self.view.zoom * 0.9);
                        }
                        let mut pct = f64::from(self.view.zoom) * 100.0;
                        let resp = ui.add(
                            egui::Slider::new(&mut pct, 20.0..=400.0)
                                .suffix("%")
                                .fixed_decimals(0),
                        );
                        if resp.changed() {
                            changed |= self.set_zoom(origin, rect.center(), (pct / 100.0) as f32);
                        }
                        if ui.add(egui::Button::new("＋").small()).clicked() {
                            changed |= self.set_zoom(origin, rect.center(), self.view.zoom * 1.1);
                        }
                        if ui.add(egui::Button::new("100%").small()).clicked()
                            && (self.view.zoom != 1.0 || self.view.offset != Vec2::ZERO)
                        {
                            self.view.zoom = 1.0;
                            self.view.offset = Vec2::ZERO;
                            changed = true;
                        }
                        if changed {
                            self.view_overlay_timer = 1.0;
                        }
                    });
                });
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !ENABLE_EXPERIMENTAL_SQUARE_PEGS {
            self.doc.peg_appearance.shape = crate::model::PegShape::Circle;
        }
        if self.enforce_safe_grid_radii() {
            self.status = "円同士が重ならない安全な半径へ自動調整しました".to_owned();
        }
        let redo = ctx.input_mut(|i| i.consume_shortcut(&SC_REDO) || i.consume_shortcut(&SC_REDO2));
        let undo = ctx.input_mut(|i| i.consume_shortcut(&SC_UNDO));
        if redo {
            self.do_redo();
        } else if undo {
            if self.draw.is_some() {
                // 描画中は「直前の引っ掛かりの解除」として働く
                self.undo_last_catch();
            } else {
                self.do_undo();
            }
        }
        // テキスト編集中（名前変更など）は単キーショートカットを無効化
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::V)) {
                self.tool = Tool::Select;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::A)) {
                self.tool = Tool::DirectSelect;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::P)) {
                self.tool = Tool::Draw;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::C)) && self.tool != Tool::Draw
            {
                self.tool = Tool::CustomPeg;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::H)) {
                self.show_circles = !self.show_circles;
            }
        }
        self.apply_theme(ctx);

        egui::TopBottomPanel::top("toolbar")
            .exact_height(32.0)
            .show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(24.0)
            .show(ctx, |ui| self.status_bar(ui));
        // DockState と App の二重借用を避けるため一時的に取り出す
        let mut dock = std::mem::replace(&mut self.dock, DockState::new(vec![DockTab::Canvas]));
        let colors = self.theme_colors();
        let mut dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
        // 区切り線の既定色は黒なので、テーマの枠色に上書きしてライトモードで
        // 黒い縁が出ないようにする
        dock_style.separator.width = 1.0;
        dock_style.separator.color_idle = colors.border;
        dock_style.separator.color_hovered = colors.accent;
        dock_style.separator.color_dragged = colors.accent;
        dock_style.main_surface_border_stroke = Stroke::NONE;
        dock_style.tab_bar.bg_fill = colors.bg1;
        dock_style.tab_bar.hline_color = colors.border;
        dock_style.tab.tab_body.bg_fill = colors.panel;
        dock_style.tab.tab_body.stroke = Stroke::NONE;
        dock_style.tab.tab_body.inner_margin = egui::Margin::same(10.0);
        DockArea::new(&mut dock)
            .style(dock_style)
            // 常時表示の×ボタンは煩いので消す（パネルの開閉は上部「パネル」メニュー）
            .show_close_buttons(false)
            .show_window_close_buttons(false)
            .show(ctx, &mut AppTabViewer { app: self });
        self.dock = dock;

        // ドックの上に浮く、細くて動かせるツールパレット
        self.tool_palette(ctx);
        self.alignment_palette(ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // 起動直後やリサイズ時に地の黒が見えないよう、テーマの背景で塗る
        let c = self.theme_colors().panel;
        let n = |v: u8| v as f32 / 255.0;
        [n(c.r()), n(c.g()), n(c.b()), 1.0]
    }
}

struct AppTabViewer<'a> {
    app: &'a mut App,
}

impl TabViewer for AppTabViewer<'_> {
    type Tab = DockTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            DockTab::Canvas => "キャンバス".into(),
            DockTab::Layers => "レイヤー".into(),
            DockTab::Circles => "円".into(),
            DockTab::Properties => "プロパティ".into(),
            DockTab::Grid => "グリッド".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        self.app.render_tab_content(ui, *tab);
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        *tab != DockTab::Canvas
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        *tab != DockTab::Canvas
    }
}

/// ツールアイコンボタン（Illustrator風、描画ベース）
fn tool_icon_button(ui: &mut egui::Ui, active: bool, tool: Tool, tooltip: &str) -> egui::Response {
    let size = Vec2::new(26.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact_selectable(&resp, active);
    if active {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, ui.visuals().selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    let color = if active {
        ui.visuals().selection.stroke.color
    } else {
        visuals.fg_stroke.color
    };
    let c = rect.center();
    let p = ui.painter();
    match tool {
        Tool::Select => {
            // 塗りのカーソル矢印
            let pts = vec![
                Pos2::new(c.x - 3.5, c.y - 6.5),
                Pos2::new(c.x - 3.5, c.y + 5.0),
                Pos2::new(c.x - 0.5, c.y + 2.2),
                Pos2::new(c.x + 1.6, c.y + 6.5),
                Pos2::new(c.x + 3.2, c.y + 5.7),
                Pos2::new(c.x + 1.1, c.y + 1.4),
                Pos2::new(c.x + 4.5, c.y + 1.0),
            ];
            p.add(egui::Shape::convex_polygon(
                pts[..3].to_vec(),
                color,
                Stroke::NONE,
            ));
            p.add(egui::Shape::closed_line(pts, Stroke::new(1.3, color)));
        }
        Tool::DirectSelect => {
            // 白抜きのカーソル矢印
            let pts = vec![
                Pos2::new(c.x - 3.5, c.y - 6.5),
                Pos2::new(c.x - 3.5, c.y + 5.0),
                Pos2::new(c.x - 0.5, c.y + 2.2),
                Pos2::new(c.x + 1.6, c.y + 6.5),
                Pos2::new(c.x + 3.2, c.y + 5.7),
                Pos2::new(c.x + 1.1, c.y + 1.4),
                Pos2::new(c.x + 4.5, c.y + 1.0),
            ];
            p.add(egui::Shape::closed_line(pts, Stroke::new(1.3, color)));
        }
        Tool::Draw => {
            // 素直な鉛筆: 斜めの軸 + 先端の三角チップ
            let top = Pos2::new(c.x + 5.0, c.y - 5.5);
            let mid = Pos2::new(c.x - 2.0, c.y + 1.5);
            let tip = Pos2::new(c.x - 5.0, c.y + 5.5);
            // 軸（2本の平行線）
            let n = Vec2::new(0.9, 0.9); // 軸に直交する太さ方向
            p.line_segment([top + n, mid + n], Stroke::new(1.3, color));
            p.line_segment([top - n, mid - n], Stroke::new(1.3, color));
            p.line_segment([top + n, top - n], Stroke::new(1.3, color)); // 消しゴム側
            // 先端チップ（塗り三角）
            p.add(egui::Shape::convex_polygon(
                vec![mid + n, mid - n, tip],
                color,
                Stroke::NONE,
            ));
        }
        Tool::CustomPeg => {
            // 円 + プラス
            p.circle_stroke(Pos2::new(c.x - 1.5, c.y), 5.0, Stroke::new(1.3, color));
            let px = c.x + 5.0;
            let py = c.y - 4.5;
            p.line_segment(
                [Pos2::new(px - 2.5, py), Pos2::new(px + 2.5, py)],
                Stroke::new(1.3, color),
            );
            p.line_segment(
                [Pos2::new(px, py - 2.5), Pos2::new(px, py + 2.5)],
                Stroke::new(1.3, color),
            );
        }
    }
    resp.on_hover_text(tooltip)
}

fn align_icon_button(ui: &mut egui::Ui, axis: AlignAxis, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(27.0, 25.0), Sense::click());
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    if response.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 4.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let c = rect.center();
    let stroke = Stroke::new(1.25, color);
    let guide = Stroke::new(0.9, color.gamma_multiply(0.55));
    let horizontal = matches!(axis, AlignAxis::Left | AlignAxis::CenterX | AlignAxis::Right);
    if horizontal {
        let gx = match axis {
            AlignAxis::Left => c.x - 7.0,
            AlignAxis::Right => c.x + 7.0,
            _ => c.x,
        };
        ui.painter().line_segment([Pos2::new(gx, c.y - 9.0), Pos2::new(gx, c.y + 9.0)], guide);
        for (i, width) in [8.0, 5.0, 10.0].iter().enumerate() {
            let y = c.y - 6.0 + i as f32 * 6.0;
            let (x1, x2) = match axis {
                AlignAxis::Left => (gx, gx + *width),
                AlignAxis::Right => (gx - *width, gx),
                _ => (gx - *width * 0.5, gx + *width * 0.5),
            };
            ui.painter().line_segment([Pos2::new(x1, y), Pos2::new(x2, y)], stroke);
        }
    } else {
        let gy = match axis {
            AlignAxis::Top => c.y - 7.0,
            AlignAxis::Bottom => c.y + 7.0,
            _ => c.y,
        };
        ui.painter().line_segment([Pos2::new(c.x - 9.0, gy), Pos2::new(c.x + 9.0, gy)], guide);
        for (i, width) in [8.0, 5.0, 10.0].iter().enumerate() {
            let x = c.x - 6.0 + i as f32 * 6.0;
            let (y1, y2) = match axis {
                AlignAxis::Top => (gy, gy + *width),
                AlignAxis::Bottom => (gy - *width, gy),
                _ => (gy - *width * 0.5, gy + *width * 0.5),
            };
            ui.painter().line_segment([Pos2::new(x, y1), Pos2::new(x, y2)], stroke);
        }
    }
    response.on_hover_text(tooltip)
}

/// テーマ切替アイコン（月/太陽）
fn theme_icon_button(ui: &mut egui::Ui, dark: bool) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    let color = visuals.fg_stroke.color;
    let c = rect.center();
    let p = ui.painter();
    if dark {
        // 太陽（ライトへ切替）
        p.circle_stroke(c, 3.2, Stroke::new(1.3, color));
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            let dir = Vec2::new(a.cos(), a.sin());
            p.line_segment([c + dir * 5.0, c + dir * 7.0], Stroke::new(1.2, color));
        }
    } else {
        // 月（ダークへ切替）: 実塗りの円から、ずらした背景色の円をくり抜いて
        // 三日月にする。背景はツールバーのパネル色に合わせる
        let bg = ui.visuals().panel_fill;
        p.circle_filled(c, 6.0, color);
        p.circle_filled(Pos2::new(c.x + 3.0, c.y - 2.2), 5.4, bg);
    }
    resp.on_hover_text(if dark { "ライトモードへ" } else { "ダークモードへ" })
}

#[derive(Clone, Copy)]
enum IconKind {
    Eye,
}

/// アクティブ状態がはっきり分かるアイコントグル。ON のとき accent 背景 + 白、
/// OFF のとき淡色。テキストトグルの見づらさを避ける
fn icon_toggle_button(
    ui: &mut egui::Ui,
    on: bool,
    kind: IconKind,
    tooltip: &str,
) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if on {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, ui.visuals().selection.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, ui.style().interact(&resp).bg_fill);
    }
    let color = if on {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().widgets.inactive.fg_stroke.color.gamma_multiply(0.75)
    };
    let c = rect.center();
    match kind {
        IconKind::Eye => {
            let p = ui.painter();
            let w = 6.5;
            let h = 4.0;
            let mut top = Vec::new();
            let mut bottom = Vec::new();
            for i in 0..=12 {
                let t = i as f32 / 12.0;
                let x = c.x - w + 2.0 * w * t;
                let arc = (t * std::f32::consts::PI).sin() * h;
                top.push(Pos2::new(x, c.y - arc));
                bottom.push(Pos2::new(x, c.y + arc));
            }
            p.add(egui::Shape::line(top, Stroke::new(1.3, color)));
            p.add(egui::Shape::line(bottom, Stroke::new(1.3, color)));
            p.circle_filled(c, 1.9, color);
            if !on {
                p.line_segment(
                    [
                        Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
                        Pos2::new(rect.right() - 4.0, rect.top() + 4.0),
                    ],
                    Stroke::new(1.4, color),
                );
            }
        }
    }
    resp.on_hover_text(tooltip)
}

/// ラベル付きの数値スクラブ入力（デザインモックの scrub-input 相当）。
/// 左に小さなラベルチップ、右にドラッグで増減できる数値。順序は常に
/// [ラベル][値] で統一する。value を破壊的に更新し、変更/確定を返す
fn scrub_value(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) -> egui::Response {
    ui.horizontal(|ui| {
        let colors_text3 = ui.visuals().weak_text_color();
        // ラベルチップ
        egui::Frame::none()
            .fill(ui.visuals().faint_bg_color)
            .rounding(egui::Rounding {
                nw: 2.0,
                sw: 2.0,
                ne: 0.0,
                se: 0.0,
            })
            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(label).color(colors_text3).small());
            });
        ui.add(
            egui::DragValue::new(value)
                .speed(speed)
                .range(range)
                .fixed_decimals(1),
        )
    })
    .inner
}

/// パネル内のセクション見出し（小さめ・弱色 + 下線）
fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).strong());
    let sep = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let rect = ui.max_rect();
    let y = ui.cursor().top() + 2.0;
    ui.painter().line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, sep),
    );
    ui.add_space(6.0);
}

/// リスト行の共通フレーム。全幅・26px 高で、選択時は accent 背景、
/// ホバー時は淡い背景を敷いてから中身を左右レイアウトで並べる
fn row_frame<R>(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let full_w = ui.available_width();
    let (bg_rect, _) = ui.allocate_exact_size(Vec2::new(full_w, 26.0), Sense::hover());
    let hovered = ui.rect_contains_pointer(bg_rect);
    if selected {
        ui.painter()
            .rect_filled(bg_rect, 5.0, ui.visuals().selection.bg_fill);
    } else if hovered {
        ui.painter()
            .rect_filled(bg_rect, 5.0, ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.5));
    }
    let inner = bg_rect.shrink2(Vec2::new(6.0, 2.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    add(&mut child)
}

/// 小さな削除（×）ボタン。描画ベースでフォント非依存
fn delete_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.bg_fill);
    }
    let c = rect.center();
    let r = 4.0;
    let color = if resp.hovered() {
        Color32::from_rgb(255, 69, 58)
    } else {
        visuals.fg_stroke.color.gamma_multiply(0.7)
    };
    ui.painter().line_segment(
        [Pos2::new(c.x - r, c.y - r), Pos2::new(c.x + r, c.y + r)],
        Stroke::new(1.4, color),
    );
    ui.painter().line_segment(
        [Pos2::new(c.x - r, c.y + r), Pos2::new(c.x + r, c.y - r)],
        Stroke::new(1.4, color),
    );
    resp
}

/// 目アイコンのトグルボタン（表示/非表示）。フォント依存を避けて描画で作る
fn eye_button(ui: &mut egui::Ui, visible: bool) -> egui::Response {
    let size = 20.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    // 表示/非表示で色は変えず、目のアイコン（開き/斜線）だけで区別する
    let color = visuals.fg_stroke.color.gamma_multiply(0.85);
    let c = rect.center();
    let w = size * 0.36;
    let h = size * 0.22;
    // アーモンド形の輪郭
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = c.x - w + 2.0 * w * t;
        let arc = (t * std::f32::consts::PI).sin() * h;
        top.push(Pos2::new(x, c.y - arc));
        bottom.push(Pos2::new(x, c.y + arc));
    }
    ui.painter().add(egui::Shape::line(top, Stroke::new(1.2, color)));
    ui.painter().add(egui::Shape::line(bottom, Stroke::new(1.2, color)));
    if visible {
        ui.painter().circle_filled(c, size * 0.11, color);
    } else {
        // 斜線
        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 4.0, rect.bottom() - 4.0),
                Pos2::new(rect.right() - 4.0, rect.top() + 4.0),
            ],
            Stroke::new(1.2, color),
        );
    }
    resp.on_hover_text(if visible { "非表示にする" } else { "表示する" })
}

fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn segments_intersect(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let eps = 1e-9;
    fn cross(a: kurbo::Vec2, b: kurbo::Vec2) -> f64 {
        a.x * b.y - a.y * b.x
    }
    fn orient(a: Point, b: Point, c: Point) -> f64 {
        cross(b - a, c - a)
    }
    fn on_segment(a: Point, b: Point, c: Point, eps: f64) -> bool {
        let min_x = a.x.min(b.x) - 1e-9;
        let max_x = a.x.max(b.x) + 1e-9;
        let min_y = a.y.min(b.y) - 1e-9;
        let max_y = a.y.max(b.y) + 1e-9;
        c.x >= min_x
            && c.x <= max_x
            && c.y >= min_y
            && c.y <= max_y
            && orient(a, b, c).abs() <= eps
    }

    let o1 = orient(a1, a2, b1);
    let o2 = orient(a1, a2, b2);
    let o3 = orient(b1, b2, a1);
    let o4 = orient(b1, b2, a2);

    let s1 = o1 > eps;
    let s2 = o2 > eps;
    let s3 = o3 > eps;
    let s4 = o4 > eps;

    if s1 != s2 && s3 != s4 {
        return true;
    }
    if o1.abs() <= eps && on_segment(a1, a2, b1, eps) {
        return true;
    }
    if o2.abs() <= eps && on_segment(a1, a2, b2, eps) {
        return true;
    }
    if o3.abs() <= eps && on_segment(b1, b2, a1, eps) {
        return true;
    }
    if o4.abs() <= eps && on_segment(b1, b2, a2, eps) {
        return true;
    }
    false
}

fn build_faces_mesh(faces: &[Vec<Point>], origin: Pos2, color: Color32) -> Option<egui::Shape> {
    use lyon::math::point as lpoint;
    use lyon::path::Path;
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    };

    if faces.is_empty() {
        return None;
    }

    let mut builder = Path::builder();
    for pts in faces {
        if pts.len() < 3 {
            continue;
        }
        builder.begin(lpoint(
            (origin.x as f64 + pts[0].x) as f32,
            (origin.y as f64 + pts[0].y) as f32,
        ));
        for p in pts.iter().skip(1) {
            builder.line_to(lpoint((origin.x as f64 + p.x) as f32, (origin.y as f64 + p.y) as f32));
        }
        builder.close();
    }

    let path = builder.build();
    let mut buffers: VertexBuffers<egui::epaint::Vertex, u32> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    let options = FillOptions::default()
        .with_fill_rule(FillRule::NonZero)
        .with_tolerance(0.25);
    tess.tessellate_path(
        &path,
        &options,
        &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| egui::epaint::Vertex {
            pos: Pos2::new(v.position().x, v.position().y),
            uv: egui::epaint::WHITE_UV,
            color,
        }),
    )
    .ok()?;

    if buffers.indices.is_empty() {
        return None;
    }
    let mesh = egui::epaint::Mesh {
        indices: buffers.indices,
        vertices: buffers.vertices,
        texture_id: egui::TextureId::default(),
    };
    Some(egui::Shape::mesh(mesh))
}

/// 日本語表示用のシステムフォントを読み込んでフォールバックに追加する
fn load_first_valid_font(paths: &[&str]) -> Option<Vec<u8>> {
    for path in paths {
        let Ok(bytes) = std::fs::read(path) else { continue };
        if ab_glyph::FontArc::try_from_vec(bytes.clone()).is_ok() {
            return Some(bytes);
        }
    }
    None
}

/// UIフォント: 欧文はシステムのサンセリフ（SF/Helvetica系）を第一候補にし、
/// 日本語はヒラギノ等をフォールバックとして重ねる。egui 既定の Ubuntu
/// フォントと日本語の混植が不格好なのを避ける
fn install_cjk_fonts(ctx: &egui::Context) {
    const LATIN: &[&str] = &[
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/HelveticaNeue.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    const CJK: &[&str] = &[
        "/System/Library/Fonts/ヒラギノ角ゴシック W4.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut order: Vec<String> = Vec::new();
    // 混植でベースラインがガタつくのを防ぐため、和欧混在に強い CJK フォントを
    // 「主フォント」にする（ヒラギノ等は欧文グリフも持つので日本語・英字を
    // 同じフォントで描け、ベースラインが揃う）。欧文専用フォントは、CJK が
    // 見つからない場合のフォールバックとしてのみ後ろに置く
    if let Some(bytes) = load_first_valid_font(CJK) {
        // 主フォント。日本語フォントは egui の行ボックス内で上寄りに描かれ、
        // ボタン内でテキストが上に偏る。わずかに下げて視覚的な中央に合わせる
        let mut data = egui::FontData::from_owned(bytes);
        data.tweak = egui::FontTweak {
            scale: 1.0,
            y_offset_factor: 0.11,
            y_offset: 0.0,
            baseline_offset_factor: 0.0,
        };
        fonts.font_data.insert("cjk".to_owned(), data);
        order.push("cjk".to_owned());
    }
    if let Some(bytes) = load_first_valid_font(LATIN) {
        fonts
            .font_data
            .insert("latin".to_owned(), egui::FontData::from_owned(bytes));
        order.push("latin".to_owned());
    }
    if order.is_empty() {
        eprintln!("警告: フォントが見つかりませんでした");
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        for (i, name) in order.iter().enumerate() {
            list.insert(i, name.clone());
        }
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_direct_selected_grid_peg_disables_it_and_releases_wraps() {
        let mut app = test_app();
        let target = PegRef::Grid { row: 0, col: 1 };
        app.doc.faces.push(Face {
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::Right },
            elements: vec![Element::Wrap { peg: target, s: 1.0, turns: 0, exit_offset: 0.0, sweep_hint: 1.0 }],
            name: String::new(), visible: true, baked: None, ignored_pegs: Vec::new(),
        });
        app.set_single_selection(Some(Selection::PegRef(target)));
        app.delete_selected();
        assert!(app.doc.disabled_pegs.contains(&target));
        assert!(app.doc.faces[0].elements.is_empty());
    }

    #[test]
    fn merge_selected_unions_two_faces_into_baked_layer() {
        let mut app = test_app();
        app.canvas_size = Vec2::new(1280.0, 800.0);
        app.canvas_center = Point::new(640.0, 400.0);
        let mk = |c0: i32, c1: i32| Face {
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: c0 }, port: Port::Top },
            elements: vec![
                Element::Wrap { peg: PegRef::Grid { row: 0, col: c0 }, s: 1.0, turns: 0, exit_offset: 0.0, sweep_hint: 3.0 },
                Element::Wrap { peg: PegRef::Grid { row: 0, col: c1 }, s: 1.0, turns: 0, exit_offset: 0.0, sweep_hint: 3.0 },
            ],
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        };
        app.doc.faces.push(mk(0, 1));
        app.doc.faces.push(mk(1, 2));
        app.set_single_selection(Some(Selection::Face(0)));
        app.toggle_selection(Selection::Face(1));
        app.rebuild_pegs(app.canvas_size);
        app.merge_selected();
        assert_eq!(app.doc.faces.len(), 1);
        assert!(app.doc.faces[0].baked.is_some());
        app.doc.faces[0].baked.as_mut().unwrap().bevel = 6.0;
        let rendered = app.materialized_faces();
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].1.points.len() >= 3);
    }

    #[test]
    fn app_grid_scrub_pipeline_does_not_panic() {
        let mut app = test_app();
        app.canvas_size = Vec2::new(1280.0, 800.0);
        app.canvas_center = Point::new(640.0, 400.0);
        // レイヤーと任意円を用意
        app.doc.custom_pegs.push(CustomPeg {
            id: 0,
            center: Point::new(500.0, 300.0),
            radius: 40.0,
            visible: true,
        });
        app.doc.faces.push(Face {
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::TopRight },
            elements: vec![
                Element::Wrap { peg: PegRef::Grid { row: 0, col: 1 }, s: 1.0, turns: 0, exit_offset: 0.2, sweep_hint: 1.0 },
                Element::Wrap { peg: PegRef::Sub { row: 0, col: 0 }, s: -1.0, turns: 0, exit_offset: 0.0, sweep_hint: 2.0 },
                Element::Wrap { peg: PegRef::Custom { id: 0 }, s: 1.0, turns: 1, exit_offset: -0.1, sweep_hint: 0.1 },
            ],
            visible: true,
            name: String::new(), baked: None, ignored_pegs: Vec::new(),
        });
        for i in 0..600 {
            app.doc.grid.spacing = 40.0 + (i as f64 * 11.7) % 280.0;
            app.doc.grid.radius = (i as f64 * 5.3) % 100.0;
            app.doc.grid.catch = (i as f64 * 1.7) % 24.0;
            app.doc.grid.sub_radius = (i as f64 * 3.1) % 60.0;
            app.doc.grid.scale_anchor = if i % 2 == 0 { ScaleAnchor::TopLeft } else { ScaleAnchor::Center };
            app.view.zoom = 0.2 + ((i as f32 * 0.37) % 3.8);
            app.rebuild_pegs(app.canvas_size);
            let rendered = app.materialized_faces();
            let mut polys: Vec<Vec<Point>> = Vec::new();
            for (_, r) in &rendered {
                if r.points.len() >= 3 {
                    polys.push(r.points.clone());
                }
                let _ = app.hit_test_face(Point::new(400.0, 300.0), &rendered);
            }
            let _ = build_faces_mesh(&polys, Pos2::new(0.0, 0.0), Color32::WHITE);
        }
    }

    #[test]
    fn safe_sub_radius_prevents_main_sub_overlap() {
        let mut grid = GridSettings::default();
        grid.spacing = 200.0;
        grid.radius = 90.0;
        grid.catch = 6.0;
        let (_, sub_max) = safe_grid_radius_limits(&grid);
        let center_distance = grid.spacing / std::f64::consts::SQRT_2;

        assert!(sub_max < 40.0);
        assert!(
            grid.radius + grid.catch + sub_max + grid.catch + PEG_CLEARANCE
                <= center_distance + f64::EPSILON
        );
    }

    #[test]
    fn unsafe_loaded_grid_radii_are_clamped() {
        let mut app = test_app();
        app.doc.grid.spacing = 120.0;
        app.doc.grid.radius = 100.0;
        app.doc.grid.catch = 12.0;
        app.doc.grid.subs_enabled = true;
        app.doc.grid.sub_radius = 60.0;

        assert!(app.enforce_safe_grid_radii());
        let (main_max, sub_max) = safe_grid_radius_limits(&app.doc.grid);
        assert!(app.doc.grid.radius <= main_max);
        assert!(app.doc.grid.sub_radius <= sub_max);
        assert!(!app.enforce_safe_grid_radii());
    }

    fn test_app() -> App {
        App {
            doc: Document::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            pegs: Vec::new(),
            peg_refs: Vec::new(),
            pegs_key: (0, 0, 0, 0, 0, 0),
            canvas_size: Vec2::ZERO,
            canvas_center: Point::new(0.0, 0.0),
            view: ViewTransform {
                offset: Vec2::ZERO,
                zoom: 1.0,
            },
            custom_radius: 40.0,
            active_custom_drag: None,
            face_drag: None,
            grab: None,
            view_overlay_timer: 0.0,
            show_circles: true,
            dark_theme: true,
            drag_snapshot: None,
            grid_edit_orig: None,
            radius_edit_orig: None,
            bevel_edit_orig: None,
            draw: None,
            selection: Vec::new(),
            dock: App::default_dock_layout(),
            face_editor: None,
            face_name_snapshot: None,
            tool: Tool::Draw,
            status: String::new(),
        }
    }

    #[test]
    fn self_intersection_detector_false_case() {
        let app = test_app();
        let segs = vec![
            Seg::Line {
                from: Point::new(0.0, 0.0),
                to: Point::new(120.0, 0.0),
            },
            Seg::Line {
                from: Point::new(120.0, 0.0),
                to: Point::new(120.0, 120.0),
            },
            Seg::Line {
                from: Point::new(120.0, 120.0),
                to: Point::new(0.0, 120.0),
            },
            Seg::Line {
                from: Point::new(0.0, 120.0),
                to: Point::new(0.0, 0.0),
            },
        ];
        assert!(!app.has_penetration_on_lines(&segs));
    }

    #[test]
    fn self_intersection_detector_true_case() {
        let app = test_app();
        let segs = vec![
            Seg::Line {
                from: Point::new(0.0, 0.0),
                to: Point::new(120.0, 120.0),
            },
            Seg::Line {
                from: Point::new(120.0, 120.0),
                to: Point::new(0.0, 120.0),
            },
            Seg::Line {
                from: Point::new(0.0, 120.0),
                to: Point::new(120.0, 0.0),
            },
            Seg::Line {
                from: Point::new(120.0, 0.0),
                to: Point::new(0.0, 0.0),
            },
        ];
        assert!(app.has_penetration_on_lines(&segs));
    }

    #[test]
    fn self_intersection_detector_checks_final_closing_edge() {
        let app = test_app();
        let points = vec![
            Point::new(2.0, -1.0),
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
            Point::new(0.0, 4.0),
            Point::new(2.0, -1.0),
        ];
        assert!(app.has_self_intersections(&points));
    }

    #[test]
    fn drawing_undo_uses_temporary_guard_not_persistent_ignore() {
        let mut app = test_app();
        let anchor = Point::new(0.0, 0.0);
        app.draw = Some(ActiveDraw {
            th: Thread {
                anchor,
                wraps: vec![crate::geometry::Wrap {
                    peg: 1,
                    s: 1.0,
                    entry: 0.0,
                    exit: 1.0,
                }],
                tip: Point::new(100.0, 0.0),
            },
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::Right },
            start_point: anchor,
            elements: Vec::new(),
            armed: true,
            ignored: vec![3],
            undo_guard: None,
            suppress_cmd_ignore_until_release: false,
            cmd_was_down: false,
        });

        app.undo_last_catch();
        let active = app.draw.as_ref().unwrap();
        assert!(active.th.wraps.is_empty());
        assert_eq!(active.ignored, vec![3]);
        assert_eq!(active.undo_guard, Some(1));
        assert!(active.suppress_cmd_ignore_until_release);
    }

    #[test]
    fn drawing_undo_without_wrap_still_suppresses_command_release() {
        let mut app = test_app();
        app.draw = Some(ActiveDraw {
            th: Thread::new(Point::new(0.0, 0.0)),
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::Right },
            start_point: Point::new(0.0, 0.0),
            elements: Vec::new(),
            armed: true,
            ignored: Vec::new(),
            undo_guard: None,
            suppress_cmd_ignore_until_release: false,
            cmd_was_down: true,
        });
        app.undo_last_catch();
        assert!(app.draw.as_ref().unwrap().suppress_cmd_ignore_until_release);
    }

    #[test]
    fn cancel_custom_drag_restores_pre_drag_document() {
        let mut app = test_app();
        app.doc.custom_pegs.push(CustomPeg {
            id: 1,
            center: Point::new(10.0, 20.0),
            radius: 12.0,
            visible: true,
        });
        app.drag_snapshot = Some(app.doc.clone());
        app.begin_drag(&[1], Point::new(10.0, 20.0), false);
        app.doc.custom_pegs[0].center = Point::new(90.0, 80.0);
        assert!(app.cancel_active_manipulation());
        assert_eq!(app.doc.custom_pegs[0].center, Point::new(10.0, 20.0));
        assert!(app.active_custom_drag.is_none());
    }

    #[test]
    fn baked_face_translation_moves_every_ring_point() {
        let mut app = test_app();
        app.doc.grid.spacing = 100.0;
        app.doc.faces.push(Face {
            start: StartAnchor { peg: PegRef::Grid { row: 0, col: 0 }, port: Port::Top },
            elements: Vec::new(),
            name: String::new(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![Point::new(1.0, 2.0), Point::new(3.0, 2.0), Point::new(1.0, 4.0)]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        });
        let base = app.doc.clone();
        app.apply_face_translation(&base, &[0], 2, 1);
        let moved = &app.doc.faces[0].baked.as_ref().unwrap().rings[0];
        assert_eq!(moved[0], Point::new(101.0, 202.0));
        assert_eq!(moved[2], Point::new(101.0, 204.0));
    }
}
