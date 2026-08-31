use crate::export::{document_to_svg, document_to_svg_styled};
use crate::font_export::export_ttf;
use crate::geometry::{
    bevel_ring, capture_face_obstacles, clamp_outside_pegs, materialize_chain, materialize_face,
    materialize_face_with_obstacles, peg_signed_distance, pegs_penetrated_by_line,
    segments_penetrate_pegs, signed_area, stroke_visible_ring_sets, thread_to_wrap_elements,
    union_polygon_ring_sets, union_visible_ring_sets, update_thread_ignoring, Peg, Seg, Thread,
};
use crate::model::{
    glyph_name, resolve_peg, CustomPeg, Document, Element, EmptyLayer, Face, FontProject,
    FontRenderStyle, GlyphRecord, GridSettings, LayerFolder, PegRef, Port, ScaleAnchor,
    StartAnchor, CURRENT_DOCUMENT_VERSION, CURRENT_FONT_PROJECT_VERSION,
};
use egui::{
    Color32, FontFamily, FontId, Id, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke,
    TextStyle, Vec2,
};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};
use kurbo::Point;
use lucide_icons::Icon as LucideIcon;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const CLOSE_RADIUS: f64 = 16.0;
const START_SNAP: f64 = 12.0;
const START_PROX: f64 = 40.0;
// 角丸四角は張力経路と形状変更時の再捕捉仕様が未確定のため、製品UIでは休眠。
// 実験コードは残し、円の挙動を固めた後に別ブランチで再開できるようにする。
const ENABLE_EXPERIMENTAL_SQUARE_PEGS: bool = false;
const PEG_CLEARANCE: f64 = 1.0;
const MAX_MANUAL_TURNS: u32 = 8;
const SINGLE_CIRCLE_MIN_SWEEP: f64 = std::f64::consts::PI;

/// 閉路候補の巻き方向を、ライブ描画の符号に近い順で展開する。
/// 小規模な閉路では全組合せを調べ、三角形など複数接触の同時反転が
/// 必要なケースを一箇所反転だけで取りこぼさない。
fn winding_variants(seed: &[Element]) -> Vec<Vec<Element>> {
    let mut normalized = Vec::with_capacity(seed.len());
    for element in seed.iter().cloned() {
        let duplicate = match (normalized.last_mut(), &element) {
            (
                Some(Element::Wrap {
                    peg: previous_peg,
                    turns: previous_turns,
                    ..
                }),
                Element::Wrap { peg, turns, .. },
            ) if previous_peg == peg => {
                *previous_turns = previous_turns.saturating_add(*turns);
                true
            }
            _ => false,
        };
        if !duplicate {
            normalized.push(element);
        }
    }
    // 閉路では末尾と先頭も隣接する。同じ円なら一つの接触へ統合し、
    // 同一中心同士の「存在しない接線」を要求しないようにする。
    if normalized.len() > 1 {
        let same_cyclic_peg = matches!(
            (normalized.first(), normalized.last()),
            (
                Some(Element::Wrap { peg: first, .. }),
                Some(Element::Wrap { peg: last, .. })
            ) if first == last
        );
        if same_cyclic_peg {
            let last = normalized.pop().unwrap();
            if let (
                Some(Element::Wrap {
                    turns: first_turns, ..
                }),
                Element::Wrap {
                    turns: last_turns, ..
                },
            ) = (normalized.first_mut(), last)
            {
                *first_turns = first_turns.saturating_add(last_turns);
            }
        }
    }

    let wrap_indices: Vec<usize> = normalized
        .iter()
        .enumerate()
        .filter_map(|(index, element)| matches!(element, Element::Wrap { .. }).then_some(index))
        .collect();
    if wrap_indices.len() <= 8 {
        let mut masks: Vec<usize> = (0..(1usize << wrap_indices.len())).collect();
        masks.sort_by_key(|mask| mask.count_ones());
        return masks
            .into_iter()
            .map(|mask| {
                let mut candidate = normalized.clone();
                for (bit, index) in wrap_indices.iter().enumerate() {
                    if mask & (1 << bit) != 0 {
                        if let Element::Wrap { s, .. } = &mut candidate[*index] {
                            *s = -*s;
                        }
                    }
                }
                candidate
            })
            .collect();
    }

    let mut variants = vec![normalized.clone()];
    for index in wrap_indices {
        let mut candidate = normalized.clone();
        if let Element::Wrap { s, .. } = &mut candidate[index] {
            *s = -*s;
        }
        variants.push(candidate);
    }
    variants
}

/// ライブ描画で実際に捕捉した巻き方向から、候補が何箇所変わったか。
/// 閉じる際の形状最短化より先にこの値を比較し、凹みなどの作図意図を守る。
fn winding_change_count(preferred: &[Element], candidate: &[Element]) -> usize {
    let mut changes = preferred.len().abs_diff(candidate.len());
    for (a, b) in preferred.iter().zip(candidate) {
        changes += match (a, b) {
            (
                Element::Wrap {
                    peg: a_peg, s: a_s, ..
                },
                Element::Wrap {
                    peg: b_peg, s: b_s, ..
                },
            ) => usize::from(a_peg != b_peg || a_s.signum() != b_s.signum()),
            (Element::Pin { pos: a }, Element::Pin { pos: b }) => {
                usize::from(a.distance(*b) > 1e-9)
            }
            _ => 1,
        };
    }
    changes
}

fn closure_score_is_better(
    winding_changes: usize,
    total_arc: f64,
    best: Option<(usize, f64)>,
) -> bool {
    best.is_none_or(|(best_changes, best_arc)| {
        winding_changes < best_changes || (winding_changes == best_changes && total_arc < best_arc)
    })
}

/// 符号付き面積が相殺される自己交差閉路でも、縦横に実体があれば形として扱う。
/// 一直線を往復するだけの退化経路は、どちらかの広がりがないため除外できる。
fn path_has_two_dimensional_extent(points: &[Point]) -> bool {
    let Some(first) = points.first() else {
        return false;
    };
    let (mut min_x, mut max_x) = (first.x, first.x);
    let (mut min_y, mut max_y) = (first.y, first.y);
    for point in &points[1..] {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    max_x - min_x > 1e-6 && max_y - min_y > 1e-6
}

fn should_release_undo_guard(
    cursor: Point,
    origin: Option<Point>,
    peg: Option<&Peg>,
    move_threshold: f64,
) -> bool {
    peg.is_none_or(|peg| peg_signed_distance(peg, cursor) > 1.0)
        || origin.is_some_and(|origin| origin.distance(cursor) > move_threshold)
}

fn face_has_minimum_constraints(face: &Face) -> bool {
    if face
        .elements
        .iter()
        .all(|element| matches!(element, Element::Wrap { .. }))
    {
        let mut unique = Vec::new();
        for element in &face.elements {
            if let Element::Wrap { peg, .. } = element {
                if !unique.contains(peg) {
                    unique.push(*peg);
                }
            }
        }
        if unique.len() >= 2 {
            true
        } else if unique.len() == 1 {
            face.elements.iter().any(|element| {
                matches!(
                    element,
                    Element::Wrap { sweep_hint, .. }
                        if sweep_hint.abs() >= std::f64::consts::TAU - 1e-6
                )
            })
        } else {
            false
        }
    } else {
        // Commandで置いたPinを含む自由形状は従来どおり許可する。
        face.elements.len() >= 2
    }
}

fn intentional_single_circle_wrap(
    thread: &Thread,
    start: StartAnchor,
    peg_refs: &[PegRef],
) -> Option<Element> {
    let wrap = thread
        .wraps
        .iter()
        .filter(|wrap| peg_refs.get(wrap.peg).is_some_and(|peg| *peg == start.peg))
        .max_by(|a, b| {
            let a_sweep = ((a.exit - a.entry) * a.s).abs();
            let b_sweep = ((b.exit - b.entry) * b.s).abs();
            a_sweep
                .partial_cmp(&b_sweep)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    (((wrap.exit - wrap.entry) * wrap.s).abs() >= SINGLE_CIRCLE_MIN_SWEEP).then_some(
        Element::Wrap {
            peg: start.peg,
            s: wrap.s,
            turns: 0,
            exit_offset: 0.0,
            // 一円シェイプは操作量にかかわらず、保存時には綺麗な一周へ揃える。
            sweep_hint: std::f64::consts::TAU,
        },
    )
}

fn safe_grid_radius_limits(grid: &GridSettings) -> (f64, f64) {
    let main_max = (grid.spacing * 0.5 - grid.catch - PEG_CLEARANCE).clamp(0.0, 100.0);
    let sub_to_sub = grid.spacing * 0.5 - grid.catch - PEG_CLEARANCE;
    let main_to_sub =
        grid.spacing / std::f64::consts::SQRT_2 - grid.radius - grid.catch * 2.0 - PEG_CLEARANCE;
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
            canvas: Color32::from_rgb(13, 13, 15),
            panel: Color32::from_rgb(16, 16, 18),
            bg1: Color32::from_rgb(21, 21, 24),
            bg2: Color32::from_rgb(34, 34, 39),
            border: Color32::from_rgb(34, 34, 38),
            text1: Color32::from_rgb(232, 232, 235),
            text2: Color32::from_rgb(170, 170, 178),
            text3: Color32::from_rgb(105, 105, 114),
            accent: Color32::from_rgb(94, 106, 210),
            accent_bg: Color32::from_rgb(34, 34, 40),
            warning: Color32::from_rgb(255, 160, 10),
            error: Color32::from_rgb(255, 69, 58),
            bg: Color32::from_rgb(13, 13, 15),
            peg_stroke: Color32::from_rgb(138, 138, 148),
            peg_fill: Color32::from_rgb(23, 23, 26),
            catch: Color32::from_rgb(72, 72, 80),
            face_fill: Color32::from_rgba_unmultiplied(214, 214, 218, 24),
            thread: Color32::from_rgb(222, 222, 226),
            anchor: Color32::from_rgb(222, 222, 226),
            close_ok: Color32::from_rgb(94, 106, 210),
            close_ng: Color32::from_rgb(255, 69, 58),
            select: Color32::from_rgb(94, 106, 210),
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
            accent: Color32::from_rgb(82, 91, 194),
            accent_bg: Color32::from_rgb(234, 235, 250),
            warning: Color32::from_rgb(255, 159, 10),
            error: Color32::from_rgb(255, 59, 48),
            bg: Color32::from_rgb(239, 239, 239),
            peg_stroke: Color32::from_rgb(130, 130, 142),
            peg_fill: Color32::from_rgb(224, 224, 228),
            catch: Color32::from_rgb(190, 190, 202),
            face_fill: Color32::from_rgb(212, 212, 216),
            thread: Color32::from_rgb(22, 22, 22),
            anchor: Color32::from_rgb(22, 22, 22),
            close_ok: Color32::from_rgb(82, 91, 194),
            close_ng: Color32::from_rgb(255, 59, 48),
            select: Color32::from_rgb(82, 91, 194),
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
        visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        visuals.widgets.active.bg_fill = self.bg2;
        visuals.widgets.hovered.bg_fill = self.bg2;
        visuals.widgets.noninteractive.bg_fill = self.panel;
        visuals.widgets.inactive.fg_stroke.color = self.text2;
        visuals.widgets.active.fg_stroke.color = self.text1;
        visuals.widgets.hovered.fg_stroke.color = self.text1;
        visuals.widgets.noninteractive.fg_stroke.color = self.text2;
        visuals.selection.bg_fill = self.accent_bg;
        visuals.selection.stroke.color = self.accent;
        visuals.widgets.inactive.corner_radius = 6.into();
        visuals.widgets.active.corner_radius = 6.into();
        visuals.widgets.hovered.corner_radius = 6.into();
        visuals.widgets.noninteractive.corner_radius = 6.into();
        visuals.window_corner_radius = 8.into();
        visuals.menu_corner_radius = 8.into();
        visuals.window_fill = self.panel;
        visuals.panel_fill = self.panel;
        visuals.extreme_bg_color = self.bg1;
        visuals.faint_bg_color = self.bg2;
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = egui::epaint::Shadow {
            offset: [0, 3],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 90 } else { 36 }),
        };
        visuals.popup_shadow = egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 80 } else { 28 }),
        };
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.active.bg_stroke = Stroke::NONE;
        visuals.widgets.hovered.bg_stroke = Stroke::NONE;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        // After Effects風: トラックは細い一本線、操作点は小さな丸。
        // 値まで塗りつぶさず、位置そのものをノブで読む編集UIにする。
        visuals.slider_trailing_fill = false;
        visuals.handle_shape = egui::style::HandleShape::Circle;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (TextStyle::Body, FontId::new(11.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(10.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(11.0, FontFamily::Monospace),
            ),
        ]
        .into_iter()
        .collect();
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(8.0, 2.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.icon_width = 12.0;
        style.spacing.icon_width_inner = 6.0;
        style.spacing.icon_spacing = 4.0;
        style.spacing.slider_width = 144.0;
        style.spacing.slider_rail_height = 1.5;
        ctx.set_style(style);
    }
}

const SC_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const SC_REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const SC_REDO2: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Y);
const SC_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

#[derive(PartialEq, Clone, Copy)]
enum Tool {
    Draw,
    Select,
    DirectSelect,
    CustomPeg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum WorkspaceMode {
    Shape,
    Font,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum UiLanguage {
    #[default]
    Japanese,
    English,
}

impl UiLanguage {
    fn text<'a>(self, japanese: &'a str, english: &'a str) -> &'a str {
        match self {
            Self::Japanese => japanese,
            Self::English => english,
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct WorkspaceFile {
    version: u32,
    shape: Document,
    font: FontProject,
    mode: WorkspaceMode,
    dark_theme: bool,
    show_circles: bool,
    #[serde(default)]
    language: UiLanguage,
    #[serde(default)]
    dock: WorkspaceDockLayout,
    #[serde(default = "default_proof_text")]
    proof_text: String,
    #[serde(default = "default_proof_size")]
    proof_size: f32,
    #[serde(default)]
    proof_tracking: f32,
    #[serde(default)]
    onion_skin: bool,
    #[serde(default)]
    onion_next: bool,
    #[serde(default = "default_onion_opacity")]
    onion_opacity: f32,
    #[serde(default = "default_onion_previous_color")]
    onion_previous_color: [u8; 4],
    #[serde(default = "default_onion_next_color")]
    onion_next_color: [u8; 4],
    #[serde(default)]
    reference_font_path: Option<PathBuf>,
    #[serde(default)]
    reference_font_face_index: u32,
    #[serde(default = "default_reference_font_opacity")]
    reference_font_opacity: f32,
    #[serde(default = "default_reference_font_scale")]
    reference_font_scale: f32,
    #[serde(default)]
    reference_font_offset: [f32; 2],
}

fn default_proof_text() -> String {
    "Hamburgefontsiv 0123".to_owned()
}

const fn default_proof_size() -> f32 {
    64.0
}

const fn default_onion_opacity() -> f32 {
    0.42
}

const fn default_onion_previous_color() -> [u8; 4] {
    [116, 184, 255, 255]
}

const fn default_onion_next_color() -> [u8; 4] {
    [255, 137, 174, 255]
}

const fn default_reference_font_opacity() -> f32 {
    0.28
}

const fn default_reference_font_scale() -> f32 {
    1.0
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct FloatingDockWindow {
    tabs: Vec<DockTab>,
    position: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct WorkspaceDockLayout {
    left_fraction: f32,
    right_fraction: f32,
    center_active: Option<DockTab>,
    inspector_active: Option<DockTab>,
    floating: Vec<FloatingDockWindow>,
}

impl Default for WorkspaceDockLayout {
    fn default() -> Self {
        Self {
            // 既定ウィンドウの実表示幅では、モックの155px / 210pxに相当。
            left_fraction: 0.14,
            right_fraction: 0.19,
            center_active: Some(DockTab::Canvas),
            inspector_active: Some(DockTab::Properties),
            floating: Vec::new(),
        }
    }
}

const CURRENT_WORKSPACE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FontMetricGuide {
    Origin,
    Right,
    Ascender,
    XHeight,
    Baseline,
    Descender,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum DockTab {
    Project,
    Canvas,
    Proof,
    Glyphs,
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

/// レイヤーパネル内で移動できる項目。インデックス／IDだけを運び、
/// ドロップ確定時に最新のDocumentへ適用する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum LayerDragPayload {
    Face(usize),
    EmptyLayer(u64),
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
    /// Undoした瞬間のカーソル位置。円内に留まっていてもここから少し動けば
    /// ガードを解除し、同じ円を再び巻けるようにする。
    undo_guard_origin: Option<Point>,
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
    wrap_accents: Vec<WrapAccent>,
}

struct ProofCacheEntry {
    document: Document,
    canvas_size: Vec2,
    canvas_center: Point,
    render_style: FontRenderStyle,
    thread_width: f64,
    rings: Arc<Vec<Vec<Point>>>,
}

#[derive(Clone)]
struct ReferenceFont {
    name: String,
    data: Vec<u8>,
    face_index: u32,
    path: PathBuf,
}

#[derive(Default)]
struct ReferenceOutlineBuilder {
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
    cursor: Point,
}

impl ReferenceOutlineBuilder {
    fn finish_contour(&mut self) {
        if self.current.len() >= 2 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn sample_quad(&mut self, control: Point, end: Point) {
        let start = self.cursor;
        for step in 1..=12 {
            let t = step as f64 / 12.0;
            let mt = 1.0 - t;
            self.current.push(Point::new(
                mt * mt * start.x + 2.0 * mt * t * control.x + t * t * end.x,
                mt * mt * start.y + 2.0 * mt * t * control.y + t * t * end.y,
            ));
        }
        self.cursor = end;
    }

    fn sample_cubic(&mut self, first: Point, second: Point, end: Point) {
        let start = self.cursor;
        for step in 1..=16 {
            let t = step as f64 / 16.0;
            let mt = 1.0 - t;
            self.current.push(Point::new(
                mt.powi(3) * start.x
                    + 3.0 * mt * mt * t * first.x
                    + 3.0 * mt * t * t * second.x
                    + t.powi(3) * end.x,
                mt.powi(3) * start.y
                    + 3.0 * mt * mt * t * first.y
                    + 3.0 * mt * t * t * second.y
                    + t.powi(3) * end.y,
            ));
        }
        self.cursor = end;
    }
}

impl ttf_parser::OutlineBuilder for ReferenceOutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        self.cursor = Point::new(f64::from(x), f64::from(y));
        self.current.push(self.cursor);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.cursor = Point::new(f64::from(x), f64::from(y));
        self.current.push(self.cursor);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.sample_quad(
            Point::new(f64::from(x1), f64::from(y1)),
            Point::new(f64::from(x), f64::from(y)),
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.sample_cubic(
            Point::new(f64::from(x1), f64::from(y1)),
            Point::new(f64::from(x2), f64::from(y2)),
            Point::new(f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.finish_contour();
    }
}

#[derive(Clone, Copy)]
struct WrapAccent {
    center: Point,
    radius: f64,
    start: f64,
    direction: f64,
    turns: u32,
}

enum SavePayload {
    Shape(Document),
    Font(FontProject),
    Workspace(WorkspaceFile),
}

enum SaveEvent {
    Progress(f32),
    Finished(Result<(), String>),
}

struct SaveOperation {
    label: String,
    progress: f32,
    receiver: mpsc::Receiver<SaveEvent>,
    finished_at: Option<Instant>,
}

pub struct App {
    doc: Document,
    workspace_mode: WorkspaceMode,
    shape_document: Document,
    shape_undo: Vec<Document>,
    shape_redo: Vec<Document>,
    font_project: FontProject,
    glyph_undo: Vec<Vec<Document>>,
    glyph_redo: Vec<Vec<Document>>,
    new_glyph_text: String,
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
    language: UiLanguage,
    dock: DockState<DockTab>,
    active_center_tab: DockTab,
    pending_panel_tab: Option<DockTab>,
    drag_snapshot: Option<Document>,
    grid_edit_orig: Option<Document>,
    /// 任意円編集中の (id)
    radius_edit_orig: Option<u64>,
    bevel_edit_orig: Option<Document>,
    face_name_snapshot: Option<Document>,
    layer_item_name_snapshot: Option<Document>,
    draw: Option<ActiveDraw>,
    selection: Vec<Selection>,
    selected_empty_layers: std::collections::HashSet<u64>,
    selected_layer_folders: std::collections::HashSet<u64>,
    face_editor: Option<usize>,
    layer_item_editor: Option<u64>,
    tool: Tool,
    status: String,
    shape_save_path: Option<PathBuf>,
    font_save_path: Option<PathBuf>,
    workspace_save_path: Option<PathBuf>,
    save_operation: Option<SaveOperation>,
    /// 前のグリフのオニオンスキン。
    onion_skin: bool,
    onion_next: bool,
    onion_opacity: f32,
    onion_previous_color: Color32,
    onion_next_color: Color32,
    proof_text: String,
    proof_size: f32,
    proof_tracking: f32,
    proof_cache: HashMap<usize, ProofCacheEntry>,
    reference_font: Option<ReferenceFont>,
    reference_font_opacity: f32,
    reference_font_scale: f32,
    reference_font_offset: Vec2,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_cjk_fonts(&cc.egui_ctx);
        let font_project = FontProject::default();
        let glyph_count = font_project.glyphs.len();
        let mut app = App {
            doc: Document::default(),
            workspace_mode: WorkspaceMode::Shape,
            shape_document: Document::default(),
            shape_undo: Vec::new(),
            shape_redo: Vec::new(),
            font_project,
            glyph_undo: vec![Vec::new(); glyph_count],
            glyph_redo: vec![Vec::new(); glyph_count],
            new_glyph_text: String::new(),
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
            language: UiLanguage::Japanese,
            dock: App::default_dock_layout(),
            active_center_tab: DockTab::Canvas,
            pending_panel_tab: None,
            drag_snapshot: None,
            grid_edit_orig: None,
            radius_edit_orig: None,
            bevel_edit_orig: None,
            face_name_snapshot: None,
            layer_item_name_snapshot: None,
            draw: None,
            selection: Vec::new(),
            selected_empty_layers: std::collections::HashSet::new(),
            selected_layer_folders: std::collections::HashSet::new(),
            face_editor: None,
            layer_item_editor: None,
            tool: Tool::Select,
            status: "選択ツール・レイヤーまたは円をクリックして選択".to_owned(),
            shape_save_path: None,
            font_save_path: None,
            workspace_save_path: None,
            save_operation: None,
            onion_skin: false,
            onion_next: false,
            onion_opacity: 0.42,
            onion_previous_color: Color32::from_rgb(116, 184, 255),
            onion_next_color: Color32::from_rgb(255, 137, 174),
            proof_text: "Hamburgefontsiv 0123".to_owned(),
            proof_size: 64.0,
            proof_tracking: 0.0,
            proof_cache: HashMap::new(),
            reference_font: None,
            reference_font_opacity: 0.28,
            reference_font_scale: 1.0,
            reference_font_offset: Vec2::ZERO,
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

    fn dock_layout_with_fractions(left: f32, right: f32) -> DockState<DockTab> {
        // 試打はフォントモードでのみキャンバスの兄弟タブとして追加する。
        let mut dock = DockState::new(vec![DockTab::Canvas]);
        // 左はプロジェクト／素材、中央は制作面、右は選択内容のインスペクター。
        // 各タブは egui_dock のドラッグで従来どおり分離できる。
        let right = right.clamp(0.16, 0.36);
        let left = left.clamp(0.12, 0.34).min(0.82 - right);
        let [center, _right] = dock.main_surface_mut().split_right(
            NodeIndex::root(),
            1.0 - right,
            vec![DockTab::Properties, DockTab::Grid, DockTab::Layers],
        );
        // split_left は画面上の左側（新規ノード）の比率を受け取る。
        // 参照モックは Project 155px / Inspector 210px の固定幅に近い。
        // egui_dock は比率指定なので、既定ウィンドウで同寸法になる比率を使う。
        dock.main_surface_mut()
            .split_left(center, left / (1.0 - right), vec![DockTab::Project]);
        if let Some(location) = dock.find_tab(&DockTab::Properties) {
            dock.set_active_tab(location);
        }
        dock
    }

    fn default_dock_layout() -> DockState<DockTab> {
        let layout = WorkspaceDockLayout::default();
        Self::dock_layout_with_fractions(layout.left_fraction, layout.right_fraction)
    }

    fn activate_panel_tab(&mut self, tab: DockTab) {
        if self.dock.find_tab(&tab).is_none() {
            if let Some((surface, node, _)) = self.dock.find_tab(&DockTab::Properties) {
                self.dock[surface][node].append_tab(tab);
            } else {
                self.dock.add_window(vec![tab]);
            }
        }
        if let Some(location) = self.dock.find_tab(&tab) {
            self.dock.set_active_tab(location);
        }
    }

    fn detach_panel_tab(&mut self, tab: DockTab, content_rect: Rect) -> bool {
        let Some(location) = self.dock.find_tab(&tab) else {
            return false;
        };
        if !location.0.is_main() || matches!(tab, DockTab::Canvas | DockTab::Proof) {
            return false;
        }
        let size = Vec2::new(
            340.0_f32.min(content_rect.width() - 32.0).max(260.0),
            620.0_f32.min(content_rect.height() - 88.0).max(320.0),
        );
        let position = Pos2::new(
            (content_rect.right() - size.x - 18.0).max(content_rect.left() + 18.0),
            content_rect.top() + 54.0,
        );
        self.dock
            .detach_tab(location, Rect::from_min_size(position, size));
        true
    }

    fn restore_workspace_dock(
        layout: WorkspaceDockLayout,
        mode: WorkspaceMode,
    ) -> DockState<DockTab> {
        let mut dock =
            Self::dock_layout_with_fractions(layout.left_fraction, layout.right_fraction);
        match mode {
            WorkspaceMode::Shape => {
                for tab in [DockTab::Proof, DockTab::Glyphs] {
                    if let Some(location) = dock.find_tab(&tab) {
                        dock.remove_tab(location);
                    }
                }
            }
            WorkspaceMode::Font => {
                if dock.find_tab(&DockTab::Proof).is_none() {
                    if let Some((surface, node, _)) = dock.find_tab(&DockTab::Canvas) {
                        dock[surface][node].append_tab(DockTab::Proof);
                    }
                }
                // フォント設定は選択なし時の「プロパティ」へ文脈表示する。
                // 専用タブはパネルメニューから必要時だけ追加・分離できる。
            }
        }
        for window in layout.floating {
            let mut tabs = Vec::new();
            for tab in window.tabs {
                if matches!(tab, DockTab::Canvas | DockTab::Proof | DockTab::Project) {
                    continue;
                }
                if let Some(location) = dock.find_tab(&tab) {
                    dock.remove_tab(location);
                }
                if !tabs.contains(&tab) {
                    tabs.push(tab);
                }
            }
            if tabs.is_empty() {
                continue;
            }
            let surface = dock.add_window(tabs);
            if let Some(state) = dock.get_window_state_mut(surface) {
                let position = Pos2::new(window.position[0], window.position[1]);
                let size = Vec2::new(window.size[0].max(260.0), window.size[1].max(220.0));
                if position.x.is_finite()
                    && position.y.is_finite()
                    && size.x.is_finite()
                    && size.y.is_finite()
                {
                    state.set_position(position).set_size(size);
                }
            }
        }
        for active in [layout.center_active, layout.inspector_active]
            .into_iter()
            .flatten()
        {
            if let Some(location) = dock.find_tab(&active) {
                dock.set_active_tab(location);
            }
        }
        dock
    }

    fn workspace_dock_layout(&self) -> WorkspaceDockLayout {
        let mut layout = WorkspaceDockLayout::default();
        let leaf_for = |tab: DockTab| {
            self.dock
                .find_tab(&tab)
                .and_then(|(surface, node, _)| surface.is_main().then_some(node))
        };
        let project_node = leaf_for(DockTab::Project);
        let center_node = leaf_for(DockTab::Canvas);
        let inspector_node = [DockTab::Properties, DockTab::Grid, DockTab::Layers]
            .into_iter()
            .find_map(leaf_for);
        if let (Some(project), Some(center), Some(inspector)) =
            (project_node, center_node, inspector_node)
        {
            let project_width = self.dock.main_surface()[project]
                .rect()
                .map_or(0.0, |rect| rect.width());
            let center_width = self.dock.main_surface()[center]
                .rect()
                .map_or(0.0, |rect| rect.width());
            let inspector_width = self.dock.main_surface()[inspector]
                .rect()
                .map_or(0.0, |rect| rect.width());
            let total = project_width + center_width + inspector_width;
            if total.is_finite() && total > 100.0 {
                layout.left_fraction = project_width / total;
                layout.right_fraction = inspector_width / total;
            }
            if let egui_dock::Node::Leaf(leaf) = &self.dock.main_surface()[center] {
                layout.center_active = leaf.tabs.get(leaf.active.0).copied();
            }
            if let egui_dock::Node::Leaf(leaf) = &self.dock.main_surface()[inspector] {
                layout.inspector_active = leaf.tabs.get(leaf.active.0).copied();
            }
        }
        layout.floating = self
            .dock
            .iter_surfaces()
            .filter_map(|surface| match surface {
                egui_dock::Surface::Window(tree, state) => {
                    let rect = state.rect();
                    let tabs: Vec<_> = tree
                        .iter()
                        .flat_map(|node| node.iter_tabs().copied())
                        .collect();
                    let has_valid_rect = rect.min.x.is_finite()
                        && rect.min.y.is_finite()
                        && rect.width().is_finite()
                        && rect.height().is_finite()
                        && rect.width() >= 1.0
                        && rect.height() >= 1.0;
                    let (position, size) = if has_valid_rect {
                        ([rect.min.x, rect.min.y], [rect.width(), rect.height()])
                    } else {
                        ([80.0, 80.0], [340.0, 620.0])
                    };
                    (!tabs.is_empty()).then_some(FloatingDockWindow {
                        tabs,
                        position,
                        size,
                    })
                }
                _ => None,
            })
            .collect();
        layout
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
                active.undo_guard_origin = Some(active.th.tip);
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
            let Some(face) = self.doc.faces.get_mut(i) else {
                continue;
            };
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

    /// マージ済み輪郭は円参照を持たないため、グリッド変更時に元スナップショット
    /// から座標を再計算する。Grid(0,0) の新旧原点を使うため、Center 基準では
    /// 参照セル (3,2) がキャンバス中央に固定されたまま間隔へ追従する。
    fn remap_baked_shapes_for_grid_change(
        document: &mut Document,
        source: &Document,
        canvas_center: Point,
    ) -> bool {
        let old_spacing = source.grid.spacing;
        let new_spacing = document.grid.spacing;
        if old_spacing.abs() <= f64::EPSILON || new_spacing.abs() <= f64::EPSILON {
            return false;
        }
        let origin = |grid: &GridSettings| match grid.scale_anchor {
            ScaleAnchor::TopLeft => Point::new(110.0, 110.0),
            ScaleAnchor::Center => Point::new(
                canvas_center.x - 3.0 * grid.spacing,
                canvas_center.y - 2.0 * grid.spacing,
            ),
        };
        let old_origin = origin(&source.grid);
        let new_origin = origin(&document.grid);
        let scale = new_spacing / old_spacing;
        let mut changed = false;
        for (face, source_face) in document.faces.iter_mut().zip(&source.faces) {
            let (Some(baked), Some(source_baked)) =
                (face.baked.as_mut(), source_face.baked.as_ref())
            else {
                continue;
            };
            for (ring, source_ring) in baked.rings.iter_mut().zip(&source_baked.rings) {
                if ring.len() != source_ring.len() {
                    continue;
                }
                for (point, source_point) in ring.iter_mut().zip(source_ring) {
                    let mapped = Point::new(
                        new_origin.x + (source_point.x - old_origin.x) * scale,
                        new_origin.y + (source_point.y - old_origin.y) * scale,
                    );
                    changed |= *point != mapped;
                    *point = mapped;
                }
            }
        }
        changed
    }

    /// レイヤー行の小さな形状プレビュー
    fn face_preview_widget(&mut self, ui: &mut egui::Ui, face_idx: usize, size: f32) {
        let (resp, painter) = ui.allocate_painter(Vec2::splat(size), Sense::click());
        let rect = resp.rect;
        let Some(face) = self.doc.faces.get(face_idx) else {
            return;
        };
        let resolver = |r: &PegRef| {
            resolve_peg(
                &self.doc.grid,
                &self.doc.custom_pegs,
                self.canvas_center,
                self.doc.peg_appearance,
                r,
            )
        };
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
        self.selected_empty_layers.clear();
        self.selected_layer_folders.clear();
        if let Some(next) = next {
            self.selection.push(next);
        }
    }

    fn toggle_selection(&mut self, target: Selection) {
        self.selected_empty_layers.clear();
        self.selected_layer_folders.clear();
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
        self.selected_empty_layers.clear();
        self.selected_layer_folders.clear();
    }

    fn select_empty_layer(&mut self, id: u64, extend: bool) {
        self.selection.clear();
        self.selected_layer_folders.clear();
        if !extend {
            self.selected_empty_layers.clear();
        }
        if extend && self.selected_empty_layers.contains(&id) {
            self.selected_empty_layers.remove(&id);
        } else {
            self.selected_empty_layers.insert(id);
        }
        self.status = if self.language == UiLanguage::Japanese {
            "空レイヤーを選択しました".to_owned()
        } else {
            "Empty layer selected".to_owned()
        };
    }

    fn select_layer_folder(&mut self, id: u64, extend: bool) {
        self.selection.clear();
        self.selected_empty_layers.clear();
        if !extend {
            self.selected_layer_folders.clear();
        }
        if extend && self.selected_layer_folders.contains(&id) {
            self.selected_layer_folders.remove(&id);
        } else {
            self.selected_layer_folders.insert(id);
        }
        self.status = self
            .language
            .text("フォルダを選択しました", "Folder selected")
            .to_owned();
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
        let empty_layer_ids: Vec<u64> = self.selected_empty_layers.iter().copied().collect();
        let folder_ids: Vec<u64> = self.selected_layer_folders.iter().copied().collect();
        if selections.is_empty() && empty_layer_ids.is_empty() && folder_ids.is_empty() {
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
                    } else if !self.doc.disabled_pegs.contains(&peg_ref)
                        && !peg_refs.contains(&peg_ref)
                    {
                        peg_refs.push(peg_ref);
                    }
                }
            }
        }

        if face_indices.is_empty()
            && peg_ids.is_empty()
            && peg_refs.is_empty()
            && empty_layer_ids.is_empty()
            && folder_ids.is_empty()
        {
            if skip_count > 0 {
                self.status = "参照中の円は削除できません".to_owned();
            } else {
                self.status = "削除対象がありません".to_owned();
            }
            return;
        }

        self.push_history();
        if !folder_ids.is_empty() {
            self.doc
                .layer_folders
                .retain(|folder| !folder_ids.contains(&folder.id));
            for layer in &mut self.doc.empty_layers {
                if layer
                    .folder_id
                    .is_some_and(|folder_id| folder_ids.contains(&folder_id))
                {
                    layer.folder_id = None;
                }
            }
        }
        if !empty_layer_ids.is_empty() {
            self.doc
                .empty_layers
                .retain(|layer| !empty_layer_ids.contains(&layer.id));
        }
        face_indices.sort_unstable_by(|a, b| b.cmp(a));
        for face_idx in &face_indices {
            self.doc.remove_face_from_layer_metadata(*face_idx);
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
                face.elements.retain(
                    |element| !matches!(element, Element::Wrap { peg, .. } if peg == peg_ref),
                );
            }
        }

        if !peg_ids.is_empty() {
            self.doc.next_peg_id = self.next_peg_id();
        }
        self.reindex_faces_after_deletion(&face_indices);
        self.selected_empty_layers
            .retain(|id| !empty_layer_ids.contains(id));
        self.selected_layer_folders
            .retain(|id| !folder_ids.contains(id));
        let faces_len = self.doc.faces.len();
        self.sync_selection(|sel| match sel {
            Selection::Peg(id) => !peg_ids.contains(id),
            Selection::Face(i) => *i < faces_len,
            Selection::PegRef(peg_ref) => !peg_refs.contains(peg_ref),
        });
        self.status = if skip_count > 0 {
            "一部の項目は参照中のため削除しませんでした".to_owned()
        } else if peg_refs.is_empty() {
            "選択項目を削除しました".to_owned()
        } else {
            "選択したグリッド円を無効化し、糸を張り直しました".to_owned()
        };
        if !peg_refs.is_empty() {
            self.rebuild_pegs(self.canvas_size);
        }
    }

    fn clear_glyph(&mut self) {
        if !self.doc.faces.is_empty()
            || !self.doc.custom_pegs.is_empty()
            || !self.doc.disabled_pegs.is_empty()
            || !self.doc.layer_folders.is_empty()
            || !self.doc.empty_layers.is_empty()
            || self.draw.is_some()
        {
            self.push_history();
            self.doc.faces.clear();
            self.doc.custom_pegs.clear();
            self.doc.disabled_pegs.clear();
            self.doc.layer_folders.clear();
            self.doc.empty_layers.clear();
            self.doc.next_peg_id = 0;
            self.doc.next_layer_item_id = 1;
            self.draw = None;
            self.clear_selection();
            self.rebuild_pegs(self.canvas_size);
            self.status = match self.workspace_mode {
                WorkspaceMode::Shape => "シェイプをクリアしました",
                WorkspaceMode::Font => "グリフをクリアしました",
            }
            .to_owned();
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
                Element::Wrap {
                    peg: PegRef::Custom { id: used },
                    ..
                } if used == id => true,
                _ => false,
            })
        })
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
        // 元のグリッド円を残すと、新しい任意円と完全に重なり、衝突判定が
        // ドラッグを常に拒否する。移動対象へ置き換えた時点で元を無効化する。
        if !self.doc.disabled_pegs.contains(&source) {
            self.doc.disabled_pegs.push(source);
        }
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

    #[cfg(test)]
    fn segments_self_intersect(&self, segs: &[Seg]) -> bool {
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
        let repeated_end = points
            .first()
            .zip(points.last())
            .is_some_and(|(a, b)| a.distance(*b) < 1e-8);
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
                    if let Some(peg) =
                        resolve_peg(grid, customs, canvas_center, appearance, &grid_ref)
                    {
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
            if let Some(peg) = resolve_peg(
                grid,
                customs,
                canvas_center,
                appearance,
                &PegRef::Custom { id: custom.id },
            ) {
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
                PegRef::Grid { row, col } => {
                    (row as u64).wrapping_mul(31) ^ (col as u64).wrapping_mul(131)
                }
                PegRef::Sub { row, col } => {
                    0x51b5 ^ (row as u64).wrapping_mul(37) ^ (col as u64).wrapping_mul(137)
                }
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
        let key = Self::peg_ref_key(
            &self.doc.grid,
            size,
            &self.doc.custom_pegs,
            self.doc.peg_appearance,
            &self.doc.disabled_pegs,
        );
        if key == self.pegs_key {
            return;
        }
        let (pegs, refs) = Self::engine_pegs(
            &self.doc.grid,
            size,
            self.canvas_center,
            &self.doc.custom_pegs,
            self.doc.peg_appearance,
            &self.doc.disabled_pegs,
        );
        self.pegs = pegs;
        self.peg_refs = refs;
        self.pegs_key = key;
        if self.draw.is_some() {
            self.draw = None;
            self.status = "グリッド/空間変更のため描画中の線をキャンセルしました".to_owned();
        }
    }

    fn nearest_grid_peg_for_start(&self, pos: Point) -> Option<(Peg, PegRef)> {
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

    fn pick_start_anchor(&self, cursor: Point) -> Option<(Point, StartAnchor)> {
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

    fn draw_start_markers(&self, ui_pos: Option<Point>, painter: &egui::Painter, origin: Pos2) {
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

    fn commit_moved_peg_captures(&mut self, moving_ids: &[u64]) {
        let resolver = |peg_ref: &PegRef| {
            resolve_peg(
                &self.doc.grid,
                &self.doc.custom_pegs,
                self.canvas_center,
                self.doc.peg_appearance,
                peg_ref,
            )
        };
        let obstacles: Vec<(PegRef, Peg)> = moving_ids
            .iter()
            .filter_map(|id| {
                let peg_ref = PegRef::Custom { id: *id };
                resolver(&peg_ref).map(|peg| (peg_ref, peg))
            })
            .collect();
        if obstacles.is_empty() {
            return;
        }

        let updates: Vec<(usize, Vec<Element>)> = self
            .doc
            .faces
            .iter()
            .enumerate()
            .filter(|(_, face)| face.visible && face.baked.is_none())
            .filter(|(_, face)| {
                !moving_ids.iter().any(|id| {
                    self.face_references_peg(face, &PegRef::Custom { id: *id })
                })
            })
            .filter_map(|(index, face)| {
                let filtered: Vec<(PegRef, Peg)> = obstacles
                    .iter()
                    .copied()
                    .filter(|(peg_ref, _)| !face.ignored_pegs.contains(peg_ref))
                    .collect();
                let elements = capture_face_obstacles(face, &resolver, &filtered)?;
                let caught = moving_ids.iter().any(|id| {
                    elements.iter().any(|element| {
                        matches!(element, Element::Wrap { peg: PegRef::Custom { id: used }, .. } if used == id)
                    })
                });
                caught.then_some((index, elements))
            })
            .collect();

        for (index, elements) in updates {
            if let Some(face) = self.doc.faces.get_mut(index) {
                face.elements = elements;
            }
        }
    }

    fn materialized_faces(&self) -> Vec<(usize, FaceRender)> {
        let resolver = |r: &PegRef| {
            resolve_peg(
                &self.doc.grid,
                &self.doc.custom_pegs,
                self.canvas_center,
                self.doc.peg_appearance,
                r,
            )
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
                out.push((
                    i,
                    FaceRender {
                        points,
                        rings,
                        invalid: false,
                        wrap_accents: Vec::new(),
                    },
                ));
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
                // 糸同士の交差は意図的な造形として許可する。無効なのは
                // 円の貫通と、手動巻きの安全上限を超えた場合だけ。
                let invalid = self.face_has_overwrapped_peg(face)
                    || segments_penetrate_pegs(
                        &segments,
                        &obstacles.iter().map(|(_, peg)| *peg).collect::<Vec<_>>(),
                    );
                let mut wrap_accents = Vec::new();
                for element in &face.elements {
                    let Element::Wrap { peg, s, turns, .. } = *element else {
                        continue;
                    };
                    if turns == 0 {
                        continue;
                    }
                    let Some(resolved) = resolver(&peg) else {
                        continue;
                    };
                    if let Some((start, direction)) =
                        segments.iter().find_map(|segment| match *segment {
                            Seg::Arc {
                                center, start, end, ..
                            } if center.distance(resolved.center) < 1e-8 => {
                                Some((start, (end - start).signum()))
                            }
                            _ => None,
                        })
                    {
                        wrap_accents.push(WrapAccent {
                            center: resolved.center,
                            radius: resolved.radius,
                            start,
                            direction: if direction == 0.0 {
                                s.signum()
                            } else {
                                direction
                            },
                            turns,
                        });
                    }
                }
                out.push((
                    i,
                    FaceRender {
                        rings: vec![points.clone()],
                        points,
                        invalid,
                        wrap_accents,
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
                    if out.is_empty() {
                        out.push(from);
                    }
                    out.push(to);
                }
                Seg::Arc {
                    center,
                    radius,
                    start,
                    end,
                } => {
                    let peg = self
                        .pegs
                        .iter()
                        .find(|peg| {
                            peg.center.distance(center) < 0.01 && (peg.radius - radius).abs() < 0.01
                        })
                        .copied();
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
                        if out
                            .last()
                            .is_none_or(|last| last.distance(point) > tol * 0.25)
                        {
                            out.push(point);
                        }
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
        let Some(reference) = rendered
            .iter()
            .find(|(index, _)| *index == indices[0])
            .and_then(|(_, render)| Self::face_bounds(render))
        else {
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
                AlignAxis::Left | AlignAxis::CenterX | AlignAxis::Right => {
                    (0, (delta / spacing).round() as i32)
                }
                AlignAxis::Top | AlignAxis::CenterY | AlignAxis::Bottom => {
                    ((delta / spacing).round() as i32, 0)
                }
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
        let resolver = |r: &PegRef| {
            resolve_peg(
                &self.doc.grid,
                &self.doc.custom_pegs,
                self.canvas_center,
                self.doc.peg_appearance,
                r,
            )
        };
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
                } else if let Some(segs) =
                    materialize_face_with_obstacles(face, &resolver, &obstacles)
                {
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
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
                turns > MAX_MANUAL_TURNS
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

    fn start_background_save(&mut self, path: PathBuf, payload: SavePayload, label: &str) {
        if self.save_operation.is_some() {
            self.status = "保存処理が進行中です".to_owned();
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let label_owned = label.to_owned();
        std::thread::spawn(move || {
            let _ = sender.send(SaveEvent::Progress(0.08));
            let serialized = match payload {
                SavePayload::Shape(document) => serde_json::to_vec_pretty(&document),
                SavePayload::Font(project) => serde_json::to_vec_pretty(&project),
                SavePayload::Workspace(workspace) => serde_json::to_vec_pretty(&workspace),
            }
            .map_err(|error| error.to_string());
            let result = serialized.and_then(|json| {
                let _ = sender.send(SaveEvent::Progress(0.55));
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("json");
                let temporary = path.with_extension(format!("{extension}.saving"));
                std::fs::write(&temporary, json).map_err(|error| error.to_string())?;
                let _ = sender.send(SaveEvent::Progress(0.86));
                if let Err(error) = std::fs::rename(&temporary, &path) {
                    let _ = std::fs::remove_file(&temporary);
                    return Err(error.to_string());
                }
                Ok(())
            });
            let _ = sender.send(SaveEvent::Finished(result));
        });
        self.save_operation = Some(SaveOperation {
            label: label_owned.clone(),
            progress: 0.02,
            receiver,
            finished_at: None,
        });
        self.status = format!("{label_owned}を保存しています…");
    }

    fn poll_save_operation(&mut self, ctx: &egui::Context) {
        let mut finished_result = None;
        let mut should_clear = false;
        if let Some(operation) = &mut self.save_operation {
            loop {
                match operation.receiver.try_recv() {
                    Ok(SaveEvent::Progress(progress)) => operation.progress = progress,
                    Ok(SaveEvent::Finished(result)) => {
                        operation.progress = 1.0;
                        operation.finished_at = Some(Instant::now());
                        finished_result = Some((operation.label.clone(), result));
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if operation.finished_at.is_none() {
                            operation.finished_at = Some(Instant::now());
                            finished_result = Some((
                                operation.label.clone(),
                                Err("保存スレッドが予期せず終了しました".to_owned()),
                            ));
                        }
                        break;
                    }
                }
            }
            if let Some(finished_at) = operation.finished_at {
                should_clear = finished_at.elapsed() >= Duration::from_millis(900);
            }
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if let Some((label, result)) = finished_result {
            self.status = match result {
                Ok(()) => format!("{label}を保存しました"),
                Err(error) => format!("{label}の保存に失敗しました: {error}"),
            };
        }
        if should_clear {
            self.save_operation = None;
        }
    }

    fn save_current_workspace(&mut self) {
        match self.workspace_mode {
            WorkspaceMode::Shape => self.save_json(),
            WorkspaceMode::Font => self.save_font_project(),
        }
    }

    fn save_json(&mut self) {
        let path = self.shape_save_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("glyph.json")
                .save_file()
        });
        if let Some(path) = path {
            self.shape_save_path = Some(path.clone());
            self.start_background_save(path, SavePayload::Shape(self.doc.clone()), "シェイプ");
        }
    }

    fn sync_active_glyph(&mut self) {
        if self.workspace_mode != WorkspaceMode::Font {
            return;
        }
        if let Some(glyph) = self
            .font_project
            .glyphs
            .get_mut(self.font_project.active_glyph)
        {
            glyph.document = self.doc.clone();
        }
    }

    fn reset_transient_editor(&mut self) {
        self.draw = None;
        self.active_custom_drag = None;
        self.face_drag = None;
        self.grab = None;
        self.drag_snapshot = None;
        self.grid_edit_orig = None;
        self.radius_edit_orig = None;
        self.bevel_edit_orig = None;
        self.face_name_snapshot = None;
        self.face_editor = None;
        self.clear_selection();
        self.pegs_key = (0, 0, 0, 0, 0, 0);
        self.rebuild_pegs(self.canvas_size);
    }

    fn ensure_glyph_history_slots(&mut self) {
        let count = self.font_project.glyphs.len();
        self.glyph_undo.resize_with(count, Vec::new);
        self.glyph_redo.resize_with(count, Vec::new);
    }

    fn store_active_glyph_session(&mut self) {
        if self.workspace_mode != WorkspaceMode::Font {
            return;
        }
        self.sync_active_glyph();
        self.ensure_glyph_history_slots();
        let index = self.font_project.active_glyph;
        self.glyph_undo[index] = std::mem::take(&mut self.undo);
        self.glyph_redo[index] = std::mem::take(&mut self.redo);
    }

    fn load_active_glyph_session(&mut self) {
        self.ensure_glyph_history_slots();
        let index = self.font_project.active_glyph;
        self.doc = self.font_project.glyphs[index].document.clone();
        self.undo = std::mem::take(&mut self.glyph_undo[index]);
        self.redo = std::mem::take(&mut self.glyph_redo[index]);
        self.reset_transient_editor();
    }

    fn switch_workspace_mode(&mut self, mode: WorkspaceMode) {
        if mode == self.workspace_mode {
            return;
        }
        self.cancel_active_manipulation();
        match self.workspace_mode {
            WorkspaceMode::Shape => {
                self.shape_document = self.doc.clone();
                self.shape_undo = std::mem::take(&mut self.undo);
                self.shape_redo = std::mem::take(&mut self.redo);
            }
            WorkspaceMode::Font => self.store_active_glyph_session(),
        }

        self.workspace_mode = mode;
        self.active_center_tab = DockTab::Canvas;
        match mode {
            WorkspaceMode::Shape => {
                if let Some(location) = self.dock.find_tab(&DockTab::Proof) {
                    self.dock.remove_tab(location);
                }
                if let Some(location) = self.dock.find_tab(&DockTab::Glyphs) {
                    self.dock.remove_tab(location);
                }
                self.doc = self.shape_document.clone();
                self.undo = std::mem::take(&mut self.shape_undo);
                self.redo = std::mem::take(&mut self.shape_redo);
                self.reset_transient_editor();
                if let Some(location) = self.dock.find_tab(&DockTab::Properties) {
                    self.dock.set_active_tab(location);
                }
                self.status = "シェイプモードに切り替えました".to_owned();
            }
            WorkspaceMode::Font => {
                if self.dock.find_tab(&DockTab::Proof).is_none() {
                    if let Some((surface, node, _)) = self.dock.find_tab(&DockTab::Canvas) {
                        self.dock[surface][node].append_tab(DockTab::Proof);
                    } else {
                        self.dock.add_window(vec![DockTab::Proof]);
                    }
                }
                self.load_active_glyph_session();
                // フォント制作へ入った直後は中央Canvas、右Propertiesを前面にする。
                if let Some(location) = self.dock.find_tab(&DockTab::Canvas) {
                    self.dock.set_active_tab(location);
                }
                if let Some(location) = self.dock.find_tab(&DockTab::Properties) {
                    self.dock.set_active_tab(location);
                }
                let glyph = &self.font_project.glyphs[self.font_project.active_glyph];
                self.status = format!("フォントモード: {} ({})", glyph.character, glyph.name);
            }
        }
    }

    fn switch_glyph(&mut self, index: usize) {
        if index >= self.font_project.glyphs.len() {
            return;
        }
        if self.workspace_mode != WorkspaceMode::Font {
            self.switch_workspace_mode(WorkspaceMode::Font);
        }
        if index == self.font_project.active_glyph {
            return;
        }
        self.store_active_glyph_session();
        self.font_project.active_glyph = index;
        self.load_active_glyph_session();
        let glyph = &self.font_project.glyphs[index];
        self.status = format!("グリフ {} ({}) を編集中", glyph.character, glyph.name);
    }

    fn add_or_switch_glyph(&mut self, character: char, duplicate_current: bool) {
        if let Some(index) = self
            .font_project
            .glyphs
            .iter()
            .position(|glyph| glyph.character == character)
        {
            self.switch_glyph(index);
            self.status = format!("既存のグリフ {character} に切り替えました");
            return;
        }
        if self.workspace_mode != WorkspaceMode::Font {
            self.switch_workspace_mode(WorkspaceMode::Font);
        }
        self.store_active_glyph_session();
        let document = if duplicate_current {
            self.font_project.glyphs[self.font_project.active_glyph]
                .document
                .clone()
        } else {
            Document::default()
        };
        self.font_project.glyphs.push(GlyphRecord {
            character,
            name: glyph_name(character),
            advance_width: if character == ' ' { 300 } else { 600 },
            left_side_bearing: 0,
            document: document.clone(),
        });
        self.glyph_undo.push(Vec::new());
        self.glyph_redo.push(Vec::new());
        self.font_project.active_glyph = self.font_project.glyphs.len() - 1;
        self.load_active_glyph_session();
        self.new_glyph_text.clear();
        self.status = format!("グリフ {character} を追加しました");
    }

    fn save_font_project(&mut self) {
        self.sync_active_glyph();
        let path = self.font_save_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("ThreadType project", &["json"])
                .set_file_name("ThreadType.font.json")
                .save_file()
        });
        if let Some(path) = path {
            self.font_save_path = Some(path.clone());
            self.start_background_save(
                path,
                SavePayload::Font(self.font_project.clone()),
                "フォントプロジェクト",
            );
        }
    }

    fn workspace_snapshot(&self) -> WorkspaceFile {
        let shape = if self.workspace_mode == WorkspaceMode::Shape {
            self.doc.clone()
        } else {
            self.shape_document.clone()
        };
        let mut font = self.font_project.clone();
        if self.workspace_mode == WorkspaceMode::Font {
            if let Some(glyph) = font.glyphs.get_mut(font.active_glyph) {
                glyph.document = self.doc.clone();
            }
        }
        WorkspaceFile {
            version: CURRENT_WORKSPACE_VERSION,
            shape,
            font,
            mode: self.workspace_mode,
            dark_theme: self.dark_theme,
            show_circles: self.show_circles,
            language: self.language,
            dock: self.workspace_dock_layout(),
            proof_text: self.proof_text.clone(),
            proof_size: self.proof_size,
            proof_tracking: self.proof_tracking,
            onion_skin: self.onion_skin,
            onion_next: self.onion_next,
            onion_opacity: self.onion_opacity,
            onion_previous_color: self.onion_previous_color.to_array(),
            onion_next_color: self.onion_next_color.to_array(),
            reference_font_path: self.reference_font.as_ref().map(|font| font.path.clone()),
            reference_font_face_index: self
                .reference_font
                .as_ref()
                .map_or(0, |font| font.face_index),
            reference_font_opacity: self.reference_font_opacity,
            reference_font_scale: self.reference_font_scale,
            reference_font_offset: [self.reference_font_offset.x, self.reference_font_offset.y],
        }
    }

    fn save_workspace(&mut self) {
        let path = self.workspace_save_path.clone().or_else(|| {
            rfd::FileDialog::new()
                .add_filter("ThreadType workspace", &["json"])
                .set_file_name("ThreadType.workspace.json")
                .save_file()
        });
        if let Some(path) = path {
            self.workspace_save_path = Some(path.clone());
            self.start_background_save(
                path,
                SavePayload::Workspace(self.workspace_snapshot()),
                "ワークスペース",
            );
        }
    }

    fn load_workspace(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ThreadType workspace", &["json"])
            .pick_file()
        else {
            return;
        };
        let loaded = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|json| {
                serde_json::from_str::<WorkspaceFile>(&json).map_err(|error| error.to_string())
            });
        match loaded {
            Ok(mut workspace)
                if workspace.version <= CURRENT_WORKSPACE_VERSION
                    && workspace.shape.version <= CURRENT_DOCUMENT_VERSION
                    && !workspace.font.glyphs.is_empty()
                    && workspace.font.version <= CURRENT_FONT_PROJECT_VERSION
                    && workspace
                        .font
                        .glyphs
                        .iter()
                        .all(|glyph| glyph.document.version <= CURRENT_DOCUMENT_VERSION) =>
            {
                workspace.font.version = CURRENT_FONT_PROJECT_VERSION;
                workspace.font.ensure_default_glyphs();
                workspace.font.active_glyph = workspace
                    .font
                    .active_glyph
                    .min(workspace.font.glyphs.len() - 1);
                self.shape_document = workspace.shape;
                self.font_project = workspace.font;
                self.workspace_mode = workspace.mode;
                self.dark_theme = workspace.dark_theme;
                self.show_circles = workspace.show_circles;
                self.language = workspace.language;
                self.dock = Self::restore_workspace_dock(workspace.dock, self.workspace_mode);
                self.proof_text = workspace.proof_text;
                self.proof_size = workspace.proof_size.clamp(20.0, 500.0);
                self.proof_tracking = workspace.proof_tracking.clamp(-40.0, 120.0);
                self.onion_skin = workspace.onion_skin;
                self.onion_next = workspace.onion_next;
                self.onion_opacity = workspace.onion_opacity.clamp(0.05, 0.9);
                self.onion_previous_color = Color32::from_rgba_unmultiplied(
                    workspace.onion_previous_color[0],
                    workspace.onion_previous_color[1],
                    workspace.onion_previous_color[2],
                    workspace.onion_previous_color[3],
                );
                self.onion_next_color = Color32::from_rgba_unmultiplied(
                    workspace.onion_next_color[0],
                    workspace.onion_next_color[1],
                    workspace.onion_next_color[2],
                    workspace.onion_next_color[3],
                );
                self.reference_font = workspace.reference_font_path.and_then(|path| {
                    Self::read_reference_font(path, workspace.reference_font_face_index)
                });
                self.reference_font_opacity = workspace.reference_font_opacity.clamp(0.03, 0.8);
                self.reference_font_scale = workspace.reference_font_scale.clamp(0.25, 2.0);
                self.reference_font_offset = Vec2::new(
                    workspace.reference_font_offset[0],
                    workspace.reference_font_offset[1],
                );
                self.shape_undo.clear();
                self.shape_redo.clear();
                self.glyph_undo = vec![Vec::new(); self.font_project.glyphs.len()];
                self.glyph_redo = vec![Vec::new(); self.font_project.glyphs.len()];
                self.undo.clear();
                self.redo.clear();
                self.doc = match self.workspace_mode {
                    WorkspaceMode::Shape => self.shape_document.clone(),
                    WorkspaceMode::Font => self.font_project.glyphs[self.font_project.active_glyph]
                        .document
                        .clone(),
                };
                self.workspace_save_path = Some(path);
                self.reset_transient_editor();
                self.rebuild_pegs(self.canvas_size);
                self.proof_cache.clear();
                self.status = "ワークスペースを読み込みました".to_owned();
            }
            Ok(_) => self.status = "未対応バージョンのワークスペースです".to_owned(),
            Err(error) => self.status = format!("ワークスペースを読めません: {error}"),
        }
    }

    fn load_font_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("ThreadType project", &["json"])
            .pick_file()
        {
            let loaded = std::fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|json| {
                    serde_json::from_str::<FontProject>(&json).map_err(|error| error.to_string())
                });
            match loaded {
                Ok(mut project)
                    if project.version <= CURRENT_FONT_PROJECT_VERSION
                        && !project.glyphs.is_empty()
                        && project
                            .glyphs
                            .iter()
                            .all(|glyph| glyph.document.version <= CURRENT_DOCUMENT_VERSION) =>
                {
                    project.version = CURRENT_FONT_PROJECT_VERSION;
                    for glyph in &mut project.glyphs {
                        glyph.document.version = CURRENT_DOCUMENT_VERSION;
                    }
                    project.active_glyph = project.active_glyph.min(project.glyphs.len() - 1);
                    project.ensure_default_glyphs();
                    self.font_project = project;
                    self.glyph_undo = vec![Vec::new(); self.font_project.glyphs.len()];
                    self.glyph_redo = vec![Vec::new(); self.font_project.glyphs.len()];
                    if self.workspace_mode == WorkspaceMode::Font {
                        self.undo.clear();
                        self.redo.clear();
                        self.load_active_glyph_session();
                    }
                    self.font_save_path = Some(path.clone());
                    self.status =
                        format!("フォントプロジェクトを読み込みました: {}", path.display());
                }
                Ok(project) if project.version > CURRENT_FONT_PROJECT_VERSION => {
                    self.status = format!(
                        "このプロジェクトは新しいバージョンです（ファイル: v{} / アプリ: v{}）",
                        project.version, CURRENT_FONT_PROJECT_VERSION
                    );
                }
                Ok(project)
                    if project
                        .glyphs
                        .iter()
                        .any(|glyph| glyph.document.version > CURRENT_DOCUMENT_VERSION) =>
                {
                    self.status = "このプロジェクト内のシェイプは新しいバージョンです".to_owned();
                }
                Ok(_) => self.status = "グリフがないため読み込めません".to_owned(),
                Err(error) => self.status = format!("プロジェクト読込に失敗しました: {error}"),
            }
        }
    }

    fn export_font_ttf(&mut self) {
        self.sync_active_glyph();
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("TrueType font", &["ttf"])
            .set_file_name(format!(
                "{}.ttf",
                self.font_project.family_name.replace(' ', "-")
            ))
            .save_file()
        {
            match export_ttf(&self.font_project, self.canvas_center, &path) {
                Ok(()) => self.status = format!("TTFを書き出しました: {}", path.display()),
                Err(error) => self.status = format!("TTF書き出しに失敗しました: {error}"),
            }
        }
    }

    fn load_json(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
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
                    self.shape_save_path = Some(path.clone());
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
            let svg = match self.workspace_mode {
                WorkspaceMode::Shape => document_to_svg(&self.doc, self.canvas_center),
                WorkspaceMode::Font => document_to_svg_styled(
                    &self.doc,
                    self.canvas_center,
                    self.font_project.render_style,
                    self.font_project.thread_width,
                ),
            };
            match std::fs::write(&path, svg) {
                Ok(()) => self.status = format!("SVGを書き出しました: {}", path.display()),
                Err(e) => self.status = format!("書き出しに失敗しました: {e}"),
            }
        }
    }

    fn hit_test_face(&self, point: Point, rendered: &[(usize, FaceRender)]) -> Option<Selection> {
        if let Some(custom) = self.doc.custom_pegs.iter().rev().find(|custom| {
            custom.visible
                && resolve_peg(
                    &self.doc.grid,
                    &self.doc.custom_pegs,
                    self.canvas_center,
                    self.doc.peg_appearance,
                    &PegRef::Custom { id: custom.id },
                )
                .is_some_and(|peg| peg_signed_distance(&peg, point) <= self.doc.grid.catch)
        }) {
            return Some(Selection::Peg(custom.id));
        }
        rendered.iter().rev().find_map(|(i, render)| {
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
            DockTab::Project => self.render_project_tab(ui),
            DockTab::Canvas => self.canvas(ui),
            DockTab::Proof => self.render_proof_tab(ui),
            DockTab::Glyphs => {
                egui::ScrollArea::vertical()
                    .id_salt("font-inspector-scroll")
                    .show(ui, |ui| self.render_glyphs_tab(ui));
            }
            DockTab::Layers => self.render_layers_tab(ui),
            DockTab::Circles => self.render_circles_tab(ui),
            DockTab::Properties => {
                egui::ScrollArea::vertical()
                    .id_salt("properties-scroll")
                    .show(ui, |ui| self.render_properties_tab(ui));
            }
            DockTab::Grid => self.render_grid_tab(ui),
        }
    }

    fn render_project_tab(&mut self, ui: &mut egui::Ui) {
        let language = self.language;
        match self.workspace_mode {
            WorkspaceMode::Shape => {
                project_nav_row(
                    ui,
                    PanelGlyph::Shape,
                    language.text("シェイプ", "Shape"),
                    None,
                    true,
                );
                project_section_label(ui, language.text("ドキュメント", "DOCUMENT"));
                for (tab, icon, label, value) in [
                    (
                        DockTab::Layers,
                        PanelGlyph::Layers,
                        language.text("レイヤー", "Layers"),
                        (self.doc.faces.len() + self.doc.empty_layers.len()).to_string(),
                    ),
                    (
                        DockTab::Circles,
                        PanelGlyph::Peg,
                        language.text("任意円", "Custom pegs"),
                        self.doc.custom_pegs.len().to_string(),
                    ),
                    (
                        DockTab::Grid,
                        PanelGlyph::Grid,
                        language.text("グリッド", "Grid"),
                        format!("{:.0}", self.doc.grid.spacing),
                    ),
                ] {
                    let response = project_nav_row(ui, icon, label, Some(&value), false);
                    if response.clicked() {
                        self.pending_panel_tab = Some(tab);
                    }
                }
            }
            WorkspaceMode::Font => {
                let glyph_count = self.font_project.glyphs.len().to_string();
                if project_nav_row(
                    ui,
                    PanelGlyph::Grid,
                    language.text("グリフ", "Glyphs"),
                    Some(&glyph_count),
                    self.active_center_tab == DockTab::Canvas,
                )
                .clicked()
                {
                    self.active_center_tab = DockTab::Canvas;
                    self.pending_panel_tab = Some(DockTab::Canvas);
                }
                if project_nav_row(
                    ui,
                    PanelGlyph::Type,
                    language.text("試し組み", "Proof"),
                    None,
                    self.active_center_tab == DockTab::Proof,
                )
                .clicked()
                {
                    self.active_center_tab = DockTab::Proof;
                    self.pending_panel_tab = Some(DockTab::Proof);
                }
                project_section_label(ui, language.text("グリフ一覧", "GLYPH LIST"));
                let glyphs: Vec<(usize, char, bool)> = self
                    .font_project
                    .glyphs
                    .iter()
                    .enumerate()
                    .map(|(index, glyph)| {
                        let document = if index == self.font_project.active_glyph {
                            &self.doc
                        } else {
                            &glyph.document
                        };
                        (index, glyph.character, !document.faces.is_empty())
                    })
                    .collect();
                let mut switch_to = None;
                // モックと同じ5列を基準にし、列数を増やして情報密度だけを
                // 上げない。余白はパネル幅へ均等配分する。
                let columns = if ui.available_width() < 136.0 { 4 } else { 5 };
                let active_glyph = self.font_project.active_glyph;
                let accent = self.theme_colors().accent;
                egui::ScrollArea::vertical()
                    .id_salt("project-glyph-scroll")
                    .max_height(282.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for row in glyphs.chunks(columns) {
                            ui.columns(columns, |column_uis| {
                                for (column, &(index, character, complete)) in
                                    row.iter().enumerate()
                                {
                                    let text = if character == ' ' {
                                        "␠".to_owned()
                                    } else {
                                        character.to_string()
                                    };
                                    let response = column_uis[column]
                                        .vertical_centered(|ui| {
                                            ui.add_sized(
                                                [24.0, 24.0],
                                                egui::Button::selectable(
                                                    index == active_glyph,
                                                    text,
                                                ),
                                            )
                                        })
                                        .inner;
                                    if complete {
                                        column_uis[column].painter().circle_filled(
                                            response.rect.right_bottom() - Vec2::new(4.0, 4.0),
                                            1.5,
                                            accent,
                                        );
                                    }
                                    if response.clicked() {
                                        switch_to = Some(index);
                                    }
                                }
                            });
                            ui.add_space(2.0);
                        }
                    });
                if let Some(index) = switch_to {
                    self.switch_glyph(index);
                }

                project_section_label(ui, language.text("ドキュメント", "DOCUMENT"));
                for (tab, icon, label, value) in [
                    (
                        DockTab::Layers,
                        PanelGlyph::Layers,
                        language.text("レイヤー", "Layers"),
                        (self.doc.faces.len() + self.doc.empty_layers.len()).to_string(),
                    ),
                    (
                        DockTab::Circles,
                        PanelGlyph::Peg,
                        language.text("ペグ", "Pegs"),
                        self.doc.custom_pegs.len().to_string(),
                    ),
                ] {
                    let response = project_nav_row(ui, icon, label, Some(&value), false);
                    if response.clicked() {
                        self.pending_panel_tab = Some(tab);
                    }
                }
            }
        }
    }

    fn render_glyphs_tab(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme_colors();
        let language = self.language;
        if self.workspace_mode == WorkspaceMode::Shape {
            ui.label(
                egui::RichText::new(language.text("フォント制作", "Font workspace"))
                    .size(18.0)
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(language.text(
                "現在は単体シェイプを整形するモードです。フォントモードでは、シェイプとは独立した a–z・約物のグリフセットを編集できます。",
                "Shape mode edits a standalone shape. Font mode edits an independent glyph set including letters and punctuation.",
            ));
            ui.add_space(10.0);
            if ui
                .button(language.text("フォントモードへ", "Open font workspace"))
                .clicked()
            {
                self.switch_workspace_mode(WorkspaceMode::Font);
            }
            return;
        }
        section_header(ui, language.text("フォント", "Font"));
        inspector_text_row(
            ui,
            language.text("ファミリー", "Family"),
            &mut self.font_project.family_name,
            language.text("ファミリー名", "Family name"),
        );
        inspector_text_row(
            ui,
            language.text("スタイル", "Style"),
            &mut self.font_project.style_name,
            language.text("スタイル名", "Style name"),
        );
        section_header(ui, language.text("グリフ", "Glyph"));

        let parameter_speed = if ui.input(|input| input.modifiers.shift) {
            8.0
        } else {
            0.5
        };

        if let Some(glyph) = self
            .font_project
            .glyphs
            .get_mut(self.font_project.active_glyph)
        {
            ui.horizontal(|ui| {
                let character = if glyph.character == ' ' {
                    "␠".to_owned()
                } else {
                    glyph.character.to_string()
                };
                ui.label(egui::RichText::new(character).size(26.0).strong());
                ui.vertical(|ui| {
                    ui.label(&glyph.name);
                    let mut advance = f64::from(glyph.advance_width);
                    if scrub_value(
                        ui,
                        language.text("送り幅", "Advance"),
                        &mut advance,
                        parameter_speed,
                        100.0..=1200.0,
                    )
                    .on_hover_text(language.text(
                        "ドラッグで微調整 / Shift+ドラッグで高速",
                        "Drag to adjust · Shift-drag for larger steps",
                    ))
                    .changed()
                    {
                        glyph.advance_width = advance.round() as u16;
                    }
                    let design_width = if glyph.character == ' ' { 0.0 } else { 600.0 };
                    let mut left = f64::from(glyph.left_side_bearing);
                    let mut right = f64::from(glyph.advance_width) - left - design_width;
                    let left_changed = scrub_value(
                        ui,
                        language.text("左余白", "Left bearing"),
                        &mut left,
                        parameter_speed,
                        -300.0..=600.0,
                    )
                    .on_hover_text(language.text(
                        "グリフを右へ送る余白 / Shift+ドラッグで高速",
                        "Space before the glyph · Shift-drag for larger steps",
                    ))
                    .changed();
                    let right_changed = scrub_value(
                        ui,
                        language.text("右余白", "Right bearing"),
                        &mut right,
                        parameter_speed,
                        -300.0..=600.0,
                    )
                    .on_hover_text(language.text(
                        "次の文字までの右余白 / Shift+ドラッグで高速",
                        "Space after the glyph · Shift-drag for larger steps",
                    ))
                    .changed();
                    if left_changed || right_changed {
                        glyph.left_side_bearing = left.round() as i16;
                        glyph.advance_width =
                            (design_width + left + right).round().clamp(100.0, 1800.0) as u16;
                    }
                });
            });
        }
        section_header(ui, language.text("オニオンスキン", "Onion skin"));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.onion_skin, language.text("前", "Previous"));
            ui.color_edit_button_srgba(&mut self.onion_previous_color);
            ui.checkbox(&mut self.onion_next, language.text("次", "Next"));
            ui.color_edit_button_srgba(&mut self.onion_next_color);
        });
        mock_slider_row_f32(
            ui,
            language.text("透明度", "Opacity"),
            &mut self.onion_opacity,
            0.05..=1.0,
            2,
        );
        section_header(ui, language.text("下書きフォント", "Reference font"));
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(language.text("Macのフォントを選択…", "Choose Mac font…"))
                .clicked()
            {
                self.choose_reference_font();
            }
            if let Some(font) = &self.reference_font {
                ui.label(egui::RichText::new(&font.name).small().color(colors.text2));
                if ui.button(language.text("解除", "Remove")).clicked() {
                    self.reference_font = None;
                    self.status = "下書きフォントを解除しました".to_owned();
                }
            }
        });
        if self.reference_font.is_some() {
            mock_slider_row_f32(
                ui,
                language.text("濃度", "Opacity"),
                &mut self.reference_font_opacity,
                0.03..=0.8,
                2,
            );
            mock_percent_slider_row_f32(
                ui,
                language.text("サイズ", "Scale"),
                &mut self.reference_font_scale,
                0.25..=2.0,
            );
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.reference_font_offset.x)
                        .speed(0.5)
                        .prefix("X "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.reference_font_offset.y)
                        .speed(0.5)
                        .prefix("Y "),
                );
                if ui
                    .button(language.text("位置をリセット", "Reset position"))
                    .clicked()
                {
                    self.reference_font_offset = Vec2::ZERO;
                }
            });
        }
        section_header(ui, language.text("メトリクス箱", "Font metrics"));
        let metrics = &mut self.font_project.metrics;
        let mut metrics_changed = false;
        metrics_changed |= scrub_value(
            ui,
            language.text("左端", "Origin"),
            &mut metrics.origin_x,
            parameter_speed,
            -400.0..=1600.0,
        )
        .on_hover_text(language.text(
            "ドラッグで微調整 / Shift+ドラッグで高速",
            "Drag to adjust · Shift-drag for larger steps",
        ))
        .changed();
        metrics_changed |= scrub_value(
            ui,
            language.text("右端", "Right edge"),
            &mut metrics.right_x,
            parameter_speed,
            -400.0..=2000.0,
        )
        .on_hover_text(language.text(
            "フォント全体で共有 / ドラッグで微調整 / Shiftで高速",
            "Shared across the font · Shift-drag for larger steps",
        ))
        .changed();
        metrics_changed |= scrub_value(
            ui,
            language.text("アセンダー", "Ascender"),
            &mut metrics.ascender_y,
            parameter_speed,
            -400.0..=1600.0,
        )
        .on_hover_text(language.text(
            "ドラッグで微調整 / Shift+ドラッグで高速",
            "Drag to adjust · Shift-drag for larger steps",
        ))
        .changed();
        metrics_changed |= scrub_value(
            ui,
            "x-height",
            &mut metrics.x_height_y,
            parameter_speed,
            -400.0..=1600.0,
        )
        .on_hover_text(language.text(
            "ドラッグで微調整 / Shift+ドラッグで高速",
            "Drag to adjust · Shift-drag for larger steps",
        ))
        .changed();
        metrics_changed |= scrub_value(
            ui,
            language.text("ベースライン", "Baseline"),
            &mut metrics.baseline_y,
            parameter_speed,
            -400.0..=1600.0,
        )
        .on_hover_text(language.text(
            "ドラッグで微調整 / Shift+ドラッグで高速",
            "Drag to adjust · Shift-drag for larger steps",
        ))
        .changed();
        metrics_changed |= scrub_value(
            ui,
            language.text("ディセンダー", "Descender"),
            &mut metrics.descender_y,
            parameter_speed,
            -400.0..=1600.0,
        )
        .on_hover_text(language.text(
            "ドラッグで微調整 / Shift+ドラッグで高速",
            "Drag to adjust · Shift-drag for larger steps",
        ))
        .changed();
        if metrics_changed {
            metrics.origin_x = metrics.origin_x.min(metrics.right_x - 40.0);
            metrics.right_x = metrics.right_x.max(metrics.origin_x + 40.0);
            let mut guides = [
                metrics.ascender_y,
                metrics.x_height_y,
                metrics.baseline_y,
                metrics.descender_y,
            ];
            for index in 1..guides.len() {
                guides[index] = guides[index].max(guides[index - 1] + 20.0);
            }
            metrics.ascender_y = guides[0];
            metrics.x_height_y = guides[1];
            metrics.baseline_y = guides[2];
            metrics.descender_y = guides[3];
            self.status = "フォントメトリクスを変更しました".to_owned();
        }
        let metrics = self.font_project.metrics;
        ui.add(
            egui::Label::new(
                egui::RichText::new(if language == UiLanguage::Japanese {
                    format!(
                        "asc {} / x {} / desc {} units / 横倍率 {:.2}",
                        metrics.ascender_units(),
                        metrics.x_height_units(),
                        metrics.descender_units(),
                        metrics.horizontal_scale()
                    )
                } else {
                    format!(
                        "asc {} / x {} / desc {} units / width scale {:.2}",
                        metrics.ascender_units(),
                        metrics.x_height_units(),
                        metrics.descender_units(),
                        metrics.horizontal_scale()
                    )
                })
                .small()
                .color(colors.text2),
            )
            .wrap(),
        );
        section_header(ui, language.text("グリフを追加", "Add glyph"));
        ui.horizontal_wrapped(|ui| {
            ui.label(language.text("追加", "Character"));
            ui.add_sized(
                [42.0, 24.0],
                egui::TextEdit::singleline(&mut self.new_glyph_text).char_limit(1),
            );
        });
        let new_character = self.new_glyph_text.chars().next();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    new_character.is_some(),
                    egui::Button::new(language.text("空で追加", "Add empty")),
                )
                .clicked()
            {
                self.add_or_switch_glyph(new_character.unwrap(), false);
            }
            if ui
                .add_enabled(
                    new_character.is_some(),
                    egui::Button::new(language.text("現在を複製", "Duplicate current")),
                )
                .clicked()
            {
                self.add_or_switch_glyph(new_character.unwrap(), true);
            }
        });

        ui.label(
            egui::RichText::new(language.text(
                "グリフの選択は左のプロジェクトパネルで行います。",
                "Select glyphs in the Project panel on the left.",
            ))
            .small()
            .color(colors.text2),
        );
    }

    fn render_proof_tab(&mut self, ui: &mut egui::Ui) {
        let colors = self.theme_colors();
        let language = self.language;
        if self.workspace_mode != WorkspaceMode::Font {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(language.text(
                            "試し組みはフォントモードで使えます",
                            "Proof is available in the font workspace",
                        ))
                        .strong(),
                    );
                    if ui
                        .button(language.text("フォントモードへ", "Open font workspace"))
                        .clicked()
                    {
                        self.switch_workspace_mode(WorkspaceMode::Font);
                    }
                });
            });
            return;
        }
        let thread_on = matches!(
            self.font_project.render_style,
            FontRenderStyle::Thread | FontRenderStyle::FillAndThread
        );
        let fixed_width = 58.0 + 58.0 + 106.0 + if thread_on { 58.0 } else { 0.0 };
        let gaps = if thread_on { 32.0 } else { 24.0 };
        let text_width = (ui.available_width() - fixed_width - gaps).max(104.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(language.text("試し組みテキスト", "PROOF TEXT"))
                        .size(9.0)
                        .color(colors.text3),
                );
                ui.add_sized(
                    [text_width, 27.0],
                    egui::TextEdit::singleline(&mut self.proof_text)
                        .hint_text(language.text("文字を入力", "Type text")),
                );
            });
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(language.text("サイズ", "SIZE"))
                        .size(9.0)
                        .color(colors.text3),
                );
                ui.add_sized(
                    [58.0, 27.0],
                    egui::DragValue::new(&mut self.proof_size)
                        .range(20.0..=500.0)
                        .speed(1.0)
                        .suffix(" px"),
                );
            });
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(language.text("字間", "TRACKING"))
                        .size(9.0)
                        .color(colors.text3),
                );
                ui.add_sized(
                    [58.0, 27.0],
                    egui::DragValue::new(&mut self.proof_tracking)
                        .range(-40.0..=120.0)
                        .speed(1.0),
                );
            });
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(language.text("表示", "DISPLAY"))
                        .size(9.0)
                        .color(colors.text3),
                );
                let selected_style = match self.font_project.render_style {
                    FontRenderStyle::Fill => language.text("塗り", "Fill"),
                    FontRenderStyle::Thread => language.text("糸", "Thread"),
                    FontRenderStyle::FillAndThread => language.text("塗り＋糸", "Fill + thread"),
                };
                egui::ComboBox::from_id_salt("proof-render-style")
                    .width(106.0)
                    .selected_text(selected_style)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.font_project.render_style,
                            FontRenderStyle::Fill,
                            language.text("塗り", "Fill"),
                        );
                        ui.selectable_value(
                            &mut self.font_project.render_style,
                            FontRenderStyle::Thread,
                            language.text("線", "Thread"),
                        );
                        ui.selectable_value(
                            &mut self.font_project.render_style,
                            FontRenderStyle::FillAndThread,
                            language.text("塗り＋糸", "Fill + thread"),
                        );
                    });
            });
            if thread_on {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(language.text("線幅", "WIDTH"))
                            .size(9.0)
                            .color(colors.text3),
                    );
                    ui.add_sized(
                        [58.0, 27.0],
                        egui::DragValue::new(&mut self.font_project.thread_width)
                            .range(0.5..=24.0)
                            .speed(0.2),
                    );
                });
            }
        });
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(6.0);

        let text = self.proof_text.clone();
        let size = self.proof_size;
        let tracking = self.proof_tracking;
        let unit_scale = size / 1000.0;
        let line_height = size * 1.35;
        let line_count = text.lines().count().max(1) as f32;
        let line_widths: Vec<f32> = text
            .split('\n')
            .map(|line| {
                line.chars().fold(0.0, |sum, character| {
                    let advance = self
                        .font_project
                        .glyphs
                        .iter()
                        .find(|glyph| glyph.character == character)
                        .map_or(600.0, |glyph| f32::from(glyph.advance_width));
                    sum + advance * unit_scale + tracking
                })
            })
            .collect();
        let widest = line_widths.iter().copied().fold(0.0_f32, f32::max);
        let desired_width = widest.max(ui.available_width() - 8.0) + 32.0;
        let preview_height = ui.available_height().max(180.0);
        let desired_height = (line_count * line_height + 40.0).max(preview_height);
        egui::ScrollArea::both()
            .id_salt("font-proof-scroll")
            .auto_shrink([false, false])
            .max_height(preview_height)
            .show(ui, |ui| {
                let (rect, _) = ui
                    .allocate_exact_size(Vec2::new(desired_width, desired_height), Sense::hover());
                ui.painter().rect_filled(rect, 6.0, colors.bg);
                ui.painter().rect_stroke(
                    rect,
                    6.0,
                    Stroke::new(1.0, colors.border),
                    egui::StrokeKind::Inside,
                );
                let metrics = self.font_project.metrics;
                let line_origin = |index: usize| {
                    let width = line_widths.get(index).copied().unwrap_or_default();
                    (rect.center().x - width * 0.5).max(rect.left() + 16.0)
                };
                let mut line_index = 0_usize;
                let mut pen_x = line_origin(line_index);
                let mut baseline =
                    rect.center().y - (line_count - 1.0) * line_height * 0.5 + size * 0.4;
                for line in 0..line_count as usize {
                    let y = baseline + line as f32 * line_height;
                    ui.painter().add(egui::Shape::dashed_line(
                        &[
                            Pos2::new(rect.left() + 12.0, y),
                            Pos2::new(rect.right() - 12.0, y),
                        ],
                        Stroke::new(0.75, colors.text3.gamma_multiply(0.35)),
                        4.0,
                        5.0,
                    ));
                }
                let mut frame_rings: HashMap<usize, Arc<Vec<Vec<Point>>>> = HashMap::new();
                for character in text.chars() {
                    if character == '\n' {
                        line_index += 1;
                        pen_x = line_origin(line_index);
                        baseline += line_height;
                        continue;
                    }
                    let glyph_index = self
                        .font_project
                        .glyphs
                        .iter()
                        .position(|glyph| glyph.character == character);
                    let (advance, left_bearing, rings) = if let Some(index) = glyph_index {
                        let glyph = &self.font_project.glyphs[index];
                        let advance = f32::from(glyph.advance_width);
                        let left_bearing = f32::from(glyph.left_side_bearing);
                        let rings = if let Some(rings) = frame_rings.get(&index) {
                            Arc::clone(rings)
                        } else {
                            let rings = self
                                .proof_glyph_rings(index)
                                .map(|(_, rings)| rings)
                                .unwrap_or_default();
                            frame_rings.insert(index, Arc::clone(&rings));
                            rings
                        };
                        (advance, left_bearing, rings)
                    } else {
                        (600.0_f32, 0.0_f32, Arc::new(Vec::new()))
                    };
                    if rings.is_empty() && character != ' ' {
                        let box_width = size * 0.55;
                        let box_rect = Rect::from_min_size(
                            Pos2::new(pen_x + left_bearing * unit_scale, baseline - size * 0.8),
                            Vec2::new(box_width, size * 0.8),
                        );
                        ui.painter().rect_stroke(
                            box_rect,
                            0.0,
                            Stroke::new(1.0, colors.text2),
                            egui::StrokeKind::Inside,
                        );
                    } else if !rings.is_empty() {
                        let mapped: Vec<Vec<Point>> = rings
                            .iter()
                            .map(|ring| {
                                ring.iter()
                                    .map(|point| {
                                        // 縦横に同じ倍率を使い、作図時の比率を保つ。
                                        let x_units =
                                            ((point.x - metrics.origin_x) * metrics.scale()) as f32
                                                + left_bearing;
                                        let y_units = ((metrics.baseline_y - point.y)
                                            * metrics.scale())
                                            as f32;
                                        Point::new(
                                            f64::from(pen_x + x_units * unit_scale),
                                            f64::from(baseline - y_units * unit_scale),
                                        )
                                    })
                                    .collect()
                            })
                            .collect();
                        if let Some(mesh) = build_faces_mesh(&mapped, Pos2::ZERO, colors.thread) {
                            ui.painter().add(mesh);
                        }
                    }
                    pen_x += advance * unit_scale + tracking;
                }
            });
        ui.label(
            egui::RichText::new(language.text(
                "未作成・未登録の文字は □。左右余白と送り幅は上で調整できます。",
                "Unbuilt or unavailable glyphs appear as □. Adjust bearings and advance in the inspector.",
            ))
                .small()
                .color(colors.text2),
        );
    }

    /// フローティングのツールパレット。上端の掴みバーをドラッグで自由に移動でき、
    /// egui_dock のタブ幅制約を受けずに細いまま保てる（Illustrator のツールバー相当）
    fn tool_palette(&mut self, ctx: &egui::Context) {
        let colors = self.theme_colors();
        let language = self.language;
        let content_rect = ctx.content_rect();
        // Dockの実レイアウトからキャンバス左端を取り、パネル比率やリサイズ後も
        // モックと同じ「キャンバス左上から14px」の位置へ置く。
        let canvas_left = self
            .dock
            .find_tab(&DockTab::Canvas)
            .and_then(|(surface, node, _)| self.dock[surface][node].rect())
            .map_or(content_rect.left() + content_rect.width() * 0.14, |rect| {
                rect.left()
            });
        let default_position = Pos2::new(canvas_left + 14.0, content_rect.top() + 92.0);
        egui::Area::new(egui::Id::new("tool-palette"))
            .movable(true)
            .default_pos(default_position)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(colors.panel)
                    .stroke(Stroke::new(1.0, colors.border))
                    .corner_radius(9)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 2],
                        blur: 10,
                        spread: 0,
                        color: Color32::from_black_alpha(if self.dark_theme { 90 } else { 30 }),
                    })
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_width(30.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.vertical_centered(|ui| {
                            // モックの22×9pxグリップ: 下線 + 1列のドット。
                            let (bar, _) =
                                ui.allocate_exact_size(Vec2::new(22.0, 9.0), Sense::hover());
                            ui.painter().line_segment(
                                [bar.left_bottom(), bar.right_bottom()],
                                Stroke::new(1.0, colors.border),
                            );
                            for column in 0..4 {
                                ui.painter().circle_filled(
                                    Pos2::new(bar.center().x - 4.5 + column as f32 * 3.0, bar.top() + 3.0),
                                    0.8,
                                    colors.text3,
                                );
                            }
                            ui.add_space(2.0);
                            for (tool, tooltip) in [
                                (
                                    Tool::Select,
                                    language.text(
                                        "選択 (V)\nレイヤー・任意円をクリックで選択、ドラッグで移動",
                                        "Selection (V)\nSelect layers or pegs; drag to move",
                                    ),
                                ),
                                (
                                    Tool::DirectSelect,
                                    language.text(
                                        "ダイレクト選択 (A)\n構成する円を個別に選択・移動",
                                        "Direct selection (A)\nSelect and move construction pegs",
                                    ),
                                ),
                                (
                                    Tool::Draw,
                                    language.text(
                                        "描画 (P)\n十字点から線を引き、円に引っ掛けて始点で閉じる",
                                        "Draw (P)\nStart at a port, wrap pegs, and close at the start",
                                    ),
                                ),
                                (
                                    Tool::CustomPeg,
                                    language.text(
                                        "任意円 (C)\nクリックで円を配置（緑=配置可 / 赤=不可）",
                                        "Custom peg (C)\nClick to place; green is valid and red overlaps",
                                    ),
                                ),
                            ] {
                                if tool_icon_button(ui, self.tool == tool, tool, tooltip).clicked()
                                {
                                    self.tool = tool;
                                }
                            }
                        });
                    });
            });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let language = self.language;
        let shadcn = egui_shadcn::Theme::new(if self.dark_theme {
            egui_shadcn::ColorPalette::shadcn_dark(egui_shadcn::ShadcnBaseColor::Zinc)
        } else {
            egui_shadcn::ColorPalette::shadcn_light(egui_shadcn::ShadcnBaseColor::Zinc)
        });
        ui.horizontal_centered(|ui| {
            let (mark_rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            ui.painter()
                .rect_filled(mark_rect, 5.0, self.theme_colors().accent);
            ui.painter().text(
                mark_rect.center(),
                egui::Align2::CENTER_CENTER,
                "T",
                FontId::proportional(12.0),
                Color32::WHITE,
            );
            ui.label(egui::RichText::new("ThreadType").strong());
            ui.label(egui::RichText::new("/").weak());
            ui.label(
                egui::RichText::new(match self.workspace_mode {
                    WorkspaceMode::Shape => language.text("無題のシェイプ", "Untitled shape"),
                    WorkspaceMode::Font => &self.font_project.family_name,
                })
                .weak(),
            );
            // 狭い画面では左から自然に並べ、横幅に余裕があるときだけ
            // ワークスペース切替をウィンドウ中央へ寄せる。
            let mode_group_width = 126.0;
            let centered_gap =
                ui.max_rect().center().x - ui.cursor().left() - mode_group_width * 0.5;
            if ui.max_rect().width() >= 1280.0 && centered_gap > 12.0 {
                ui.add_space(centered_gap);
            } else {
                ui.add_space(12.0);
            }
            let shape_selected = self.workspace_mode == WorkspaceMode::Shape;
            if egui_shadcn::button(
                ui,
                &shadcn,
                language.text("シェイプ", "Shape"),
                if shape_selected {
                    egui_shadcn::ControlVariant::Secondary
                } else {
                    egui_shadcn::ControlVariant::Ghost
                },
                egui_shadcn::ControlSize::Sm,
                true,
            )
            .on_hover_text(language.text(
                "単体のパス／シェイプを制作する独立ワークスペース",
                "Workspace for a standalone path or shape",
            ))
            .clicked()
            {
                self.switch_workspace_mode(WorkspaceMode::Shape);
            }
            if egui_shadcn::button(
                ui,
                &shadcn,
                language.text("フォント", "Font"),
                if !shape_selected {
                    egui_shadcn::ControlVariant::Secondary
                } else {
                    egui_shadcn::ControlVariant::Ghost
                },
                egui_shadcn::ControlSize::Sm,
                true,
            )
            .on_hover_text(language.text(
                "A–Z・a–z・0–9・約物を制作し、フォントとして保存する",
                "Build letters, numbers, and punctuation, then export a font",
            ))
            .clicked()
            {
                self.switch_workspace_mode(WorkspaceMode::Font);
            }
            if self.tool == Tool::CustomPeg {
                ui.separator();
                ui.allocate_ui_with_layout(
                    Vec2::new(190.0, 22.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        mock_slider_row_f64(
                            ui,
                            language.text("新規円 半径", "New peg radius"),
                            &mut self.custom_radius,
                            4.0..=120.0,
                            1,
                            "",
                        )
                    },
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let save_response = ui.add_sized(
                    [
                        if language == UiLanguage::Japanese {
                            44.0
                        } else {
                            48.0
                        },
                        26.0,
                    ],
                    egui::Button::new(
                        egui::RichText::new(language.text("保存", "Save"))
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(self.theme_colors().accent)
                    .stroke(Stroke::NONE)
                    .corner_radius(6),
                );
                if save_response.clicked() {
                    match self.workspace_mode {
                        WorkspaceMode::Shape => self.save_json(),
                        WorkspaceMode::Font => self.save_font_project(),
                    }
                }
                let language_response = language_menu_button(
                    ui,
                    self.language,
                    language.text("表示言語", "Display language"),
                );
                egui::Popup::menu(&language_response).show(|ui| {
                    if ui
                        .selectable_label(self.language == UiLanguage::Japanese, "日本語")
                        .clicked()
                    {
                        self.language = UiLanguage::Japanese;
                        ui.close();
                    }
                    if ui
                        .selectable_label(self.language == UiLanguage::English, "English")
                        .clicked()
                    {
                        self.language = UiLanguage::English;
                        ui.close();
                    }
                });
                let panels_response = header_menu_icon_button(
                    ui,
                    HeaderMenuGlyph::Panels,
                    language.text("パネル", "Panels"),
                );
                egui::Popup::menu(&panels_response).show(|ui| {
                    for (tab, label) in [
                        (DockTab::Glyphs, language.text("フォント", "Font")),
                        (DockTab::Layers, language.text("レイヤー", "Layers")),
                        (DockTab::Circles, language.text("円", "Pegs")),
                        (
                            DockTab::Properties,
                            language.text("プロパティ", "Properties"),
                        ),
                        (DockTab::Grid, language.text("グリッド", "Grid")),
                    ] {
                        let location = self.dock.find_tab(&tab);
                        if location.is_none() && ui.button(label).clicked() {
                            self.dock.add_window(vec![tab]);
                            ui.close();
                        } else if location.is_some_and(|location| location.0.is_main())
                            && ui
                                .button(if language == UiLanguage::Japanese {
                                    format!("{label}を分離")
                                } else {
                                    format!("{label} — Detach")
                                })
                                .clicked()
                        {
                            let content_rect = ui.ctx().content_rect();
                            self.detach_panel_tab(tab, content_rect);
                            ui.close();
                        } else if location.is_some_and(|location| !location.0.is_main()) {
                            ui.add_enabled(
                                false,
                                egui::Button::new(if language == UiLanguage::Japanese {
                                    format!("{label}（分離中）")
                                } else {
                                    format!("{label} — Floating")
                                }),
                            );
                        }
                    }
                    ui.separator();
                    if ui
                        .button(language.text("標準レイアウトに戻す", "Reset layout"))
                        .clicked()
                    {
                        self.dock = Self::restore_workspace_dock(
                            WorkspaceDockLayout::default(),
                            self.workspace_mode,
                        );
                        ui.close();
                    }
                });
                if theme_icon_button(
                    ui,
                    self.dark_theme,
                    if self.dark_theme {
                        language.text("ライトモードへ", "Switch to light mode")
                    } else {
                        language.text("ダークモードへ", "Switch to dark mode")
                    },
                )
                .clicked()
                {
                    self.dark_theme = !self.dark_theme;
                }
                if icon_toggle_button(
                    ui,
                    self.show_circles,
                    IconKind::Eye,
                    language.text("円を表示 (H)", "Show pegs (H)"),
                )
                .clicked()
                {
                    self.show_circles = !self.show_circles;
                }
                let file_response = header_menu_icon_button(
                    ui,
                    HeaderMenuGlyph::File,
                    language.text("ファイル", "File"),
                );
                egui::Popup::menu(&file_response).show(|ui| {
                    if ui
                        .add_enabled(
                            !self.undo.is_empty() || self.draw.is_some(),
                            egui::Button::new(language.text("元に戻す  ⌘Z", "Undo  ⌘Z")),
                        )
                        .clicked()
                    {
                        if self.draw.is_some() {
                            self.undo_last_catch();
                        } else {
                            self.do_undo();
                        }
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.redo.is_empty(),
                            egui::Button::new(language.text("やり直す  ⇧⌘Z", "Redo  ⇧⌘Z")),
                        )
                        .clicked()
                    {
                        self.do_redo();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(language.text("開く…", "Open…")).clicked() {
                        match self.workspace_mode {
                            WorkspaceMode::Shape => self.load_json(),
                            WorkspaceMode::Font => self.load_font_project(),
                        }
                        ui.close();
                    }
                    if ui
                        .button(language.text("SVGを書き出す…", "Export SVG…"))
                        .clicked()
                    {
                        self.export_svg();
                        ui.close();
                    }
                    if self.workspace_mode == WorkspaceMode::Font
                        && ui
                            .button(language.text("TTFを書き出す…", "Export TTF…"))
                            .clicked()
                    {
                        self.export_font_ttf();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(language.text("ワークスペースを保存…", "Save workspace…"))
                        .clicked()
                    {
                        self.save_workspace();
                        ui.close();
                    }
                    if ui
                        .button(language.text("ワークスペースを開く…", "Open workspace…"))
                        .clicked()
                    {
                        self.load_workspace();
                        ui.close();
                    }
                });
                if let Some(operation) = &self.save_operation {
                    ui.add_sized(
                        [150.0, 18.0],
                        egui::ProgressBar::new(operation.progress).text(format!(
                            "{} {:>3}%",
                            operation.label,
                            (operation.progress * 100.0).round() as u32
                        )),
                    );
                }
            });
        });
    }

    fn displayed_status(&self) -> String {
        if self.language == UiLanguage::Japanese {
            return self.status.clone();
        }
        if let Some(rest) = self.status.strip_prefix("フォントモード:") {
            return format!("Font mode:{rest}");
        }
        if let Some(rest) = self.status.strip_prefix("グリフ ") {
            if let Some(glyph) = rest.strip_suffix(" を編集中") {
                return format!("Editing glyph {glyph}");
            }
            if let Some(glyph) = rest.strip_suffix(" を追加しました") {
                return format!("Added glyph {glyph}");
            }
        }
        match self.status.as_str() {
            "クリックで線を引き始め、始点に戻ってクリックで面を閉じます" => {
                "Click to start a thread; return to the start to close the face"
            }
            "選択ツール・レイヤーまたは円をクリックして選択" => {
                "Selection tool · Click a layer or peg to select"
            }
            "直前の引っ掛かりを解除しました" => "Removed the last wrap",
            "解除できる引っ掛かりがありません" => "No wrap to remove",
            "元に戻しました" => "Undone",
            "やり直しました" => "Redone",
            "描画中は削除できません" => "Finish drawing before deleting",
            "削除対象がありません" => "Nothing to delete",
            "参照中の円は削除できません" => "A peg used by a shape cannot be deleted",
            "選択項目を削除しました" => "Selected items deleted",
            "空レイヤーを選択しました" => "Empty layer selected",
            "フォルダを選択しました" => "Folder selected",
            "シェイプをクリアしました" => "Shape cleared",
            "グリフをクリアしました" => "Glyph cleared",
            "整列するレイヤーを2つ以上選んでください" => {
                "Select at least two layers to align"
            }
            "レイヤーを整列しました（グリッド単位）" => {
                "Layers aligned to the grid"
            }
            "整列対象はすでに同じ位置です" => {
                "The selected layers are already aligned"
            }
            "マージするレイヤーを2つ以上選んでください" => {
                "Select at least two layers to merge"
            }
            "マージできませんでした" => "The selected layers could not be merged",
            "レイヤーをマージしました" => "Layers merged",
            "保存処理が進行中です" => "A save is already in progress",
            "シェイプモードに切り替えました" => "Shape mode",
            "ワークスペースを読み込みました" => "Workspace loaded",
            "未対応バージョンのワークスペースです" => {
                "Unsupported workspace version"
            }
            "下書きフォントを解除しました" => "Reference font removed",
            "フォントメトリクスを変更しました" => "Font metrics updated",
            "空レイヤーを追加しました" => "Empty layer added",
            "フォルダを追加しました" => "Folder added",
            "選択中のレイヤーを表示" => "Selected layers shown",
            "選択中のレイヤーを非表示" => "Selected layers hidden",
            "任意円を変更しました" => "Custom peg updated",
            "無効化したグリッド円を戻しました" => "Disabled grid pegs restored",
            "グリッド変更のため描画中の線をキャンセルしました" => {
                "Drawing cancelled after a grid change"
            }
            "円同士が重ならない安全な半径へ自動調整しました" => {
                "Radius clamped to prevent overlapping pegs"
            }
            "メトリクスガイドを移動しました" => "Metric guide moved",
            "移動を取り消しました" => "Move cancelled",
            "任意円を移動しました" => "Custom peg moved",
            "描画をキャンセルしました" => "Drawing cancelled",
            "選択していません" => "Nothing selected",
            "任意円を配置しました" => "Custom peg added",
            "この位置には配置できません（重なり）" => {
                "A peg cannot be placed here because it overlaps"
            }
            "外周線を閉じてレイヤーを確定しました" => {
                "Closed the outline and created a layer"
            }
            "Pin を追加しました" => "Pin added",
            // Status produced while already in English is kept verbatim. Unknown
            // legacy Japanese messages are also kept instead of lying with “Ready”.
            other => other,
        }
        .to_owned()
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let status = self.displayed_status();
        ui.horizontal_centered(|ui| {
            ui.label(egui::RichText::new(status).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(if self.language == UiLanguage::Japanese {
                        format!(
                            "{:.0}% ・ グリッド {:.0}",
                            f64::from(self.view.zoom) * 100.0,
                            self.doc.grid.spacing
                        )
                    } else {
                        format!(
                            "{:.0}% · Grid {:.0}",
                            f64::from(self.view.zoom) * 100.0,
                            self.doc.grid.spacing
                        )
                    })
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

    fn begin_layer_item_edit(&mut self, id: u64) {
        self.layer_item_name_snapshot = Some(self.doc.clone());
        self.layer_item_editor = Some(id);
    }

    fn commit_layer_item_edit(&mut self) {
        if let Some(snapshot) = self.layer_item_name_snapshot.take() {
            self.push_history_snapshot(snapshot);
        }
        self.layer_item_editor = None;
    }

    fn add_empty_layer(&mut self) -> u64 {
        let snapshot = self.doc.clone();
        let id = self.doc.allocate_layer_item_id();
        let number = self.doc.empty_layers.len() + self.doc.faces.len() + 1;
        let folder_id = (self.selected_layer_folders.len() == 1)
            .then(|| self.selected_layer_folders.iter().copied().next())
            .flatten();
        self.doc.empty_layers.push(EmptyLayer {
            id,
            name: format!("{} {number}", self.language.text("レイヤー", "Layer")),
            visible: true,
            folder_id,
        });
        self.push_history_snapshot(snapshot);
        self.selection.clear();
        self.selected_empty_layers.clear();
        self.selected_layer_folders.clear();
        self.selected_empty_layers.insert(id);
        self.status = self
            .language
            .text("空レイヤーを追加しました", "Empty layer added")
            .to_owned();
        id
    }

    fn add_layer_folder(&mut self) -> u64 {
        let snapshot = self.doc.clone();
        let id = self.doc.allocate_layer_item_id();
        let mut face_indices: Vec<usize> = self
            .selection
            .iter()
            .filter_map(|selection| match selection {
                Selection::Face(index) => Some(*index),
                _ => None,
            })
            .collect();
        face_indices.sort_unstable();
        face_indices.dedup();
        for folder in &mut self.doc.layer_folders {
            folder
                .face_indices
                .retain(|index| !face_indices.contains(index));
        }
        for layer in &mut self.doc.empty_layers {
            if self.selected_empty_layers.contains(&layer.id) {
                layer.folder_id = Some(id);
            }
        }
        let number = self.doc.layer_folders.len() + 1;
        self.doc.layer_folders.push(LayerFolder {
            id,
            name: format!("{} {number}", self.language.text("フォルダ", "Folder")),
            visible: true,
            expanded: true,
            face_indices,
        });
        self.push_history_snapshot(snapshot);
        self.clear_selection();
        self.selected_layer_folders.insert(id);
        self.status = self
            .language
            .text("フォルダを追加しました", "Folder added")
            .to_owned();
        id
    }

    /// レイヤー項目を既存フォルダへ移す。所属は排他的で、同じシェイプが
    /// 複数フォルダへ残らない。D&D 1回をUndo 1回として記録する。
    fn move_layer_item_to_folder(
        &mut self,
        payload: LayerDragPayload,
        target_folder_id: u64,
    ) -> bool {
        let Some(target_index) = self
            .doc
            .layer_folders
            .iter()
            .position(|folder| folder.id == target_folder_id)
        else {
            return false;
        };
        let snapshot = self.doc.clone();
        let changed = match payload {
            LayerDragPayload::Face(face_index) if face_index < self.doc.faces.len() => {
                let already_there = self.doc.layer_folders[target_index]
                    .face_indices
                    .contains(&face_index);
                let membership_count = self
                    .doc
                    .layer_folders
                    .iter()
                    .filter(|folder| folder.face_indices.contains(&face_index))
                    .count();
                for folder in &mut self.doc.layer_folders {
                    folder.face_indices.retain(|index| *index != face_index);
                }
                self.doc.layer_folders[target_index]
                    .face_indices
                    .push(face_index);
                !already_there || membership_count != 1
            }
            LayerDragPayload::EmptyLayer(layer_id) => {
                let Some(layer) = self
                    .doc
                    .empty_layers
                    .iter_mut()
                    .find(|layer| layer.id == layer_id)
                else {
                    return false;
                };
                let changed = layer.folder_id != Some(target_folder_id);
                layer.folder_id = Some(target_folder_id);
                changed
            }
            LayerDragPayload::Face(_) => return false,
        };
        self.doc.layer_folders[target_index].expanded = true;
        if changed {
            self.push_history_snapshot(snapshot);
            self.status = self
                .language
                .text(
                    "レイヤーをフォルダへ移動しました",
                    "Layer moved into folder",
                )
                .to_owned();
        }
        changed
    }

    /// Remove an item from its folder. Faces are also inserted at the requested
    /// root-layer gap, with every folder reference and face selection remapped.
    fn move_layer_item_to_root(
        &mut self,
        payload: LayerDragPayload,
        before_face: Option<usize>,
    ) -> bool {
        let snapshot = self.doc.clone();
        let changed = match payload {
            LayerDragPayload::EmptyLayer(layer_id) => {
                let Some(layer) = self
                    .doc
                    .empty_layers
                    .iter_mut()
                    .find(|layer| layer.id == layer_id)
                else {
                    return false;
                };
                let changed = layer.folder_id.is_some();
                layer.folder_id = None;
                changed
            }
            LayerDragPayload::Face(source) if source < self.doc.faces.len() => {
                let was_grouped = self
                    .doc
                    .layer_folders
                    .iter()
                    .any(|folder| folder.face_indices.contains(&source));
                let len = self.doc.faces.len();
                let target = before_face
                    .map(|before| if source < before { before - 1 } else { before })
                    .unwrap_or(len - 1)
                    .min(len - 1);

                let mut old_order: Vec<usize> = (0..len).collect();
                let moved_old = old_order.remove(source);
                old_order.insert(target, moved_old);
                let mut old_to_new = vec![0usize; len];
                for (new_index, old_index) in old_order.iter().copied().enumerate() {
                    old_to_new[old_index] = new_index;
                }

                if target != source {
                    let old_faces = self.doc.faces.clone();
                    self.doc.faces = old_order
                        .iter()
                        .map(|old_index| old_faces[*old_index].clone())
                        .collect();
                }
                for folder in &mut self.doc.layer_folders {
                    folder.face_indices.retain(|index| *index != source);
                    for index in &mut folder.face_indices {
                        *index = old_to_new[*index];
                    }
                }
                for selection in &mut self.selection {
                    if let Selection::Face(index) = selection {
                        *index = old_to_new[*index];
                    }
                }
                if let Some(index) = self.face_editor.as_mut() {
                    *index = old_to_new[*index];
                }
                was_grouped || target != source
            }
            LayerDragPayload::Face(_) => return false,
        };
        if changed {
            self.push_history_snapshot(snapshot);
            self.status = self
                .language
                .text(
                    "レイヤーをフォルダから取り出しました",
                    "Layer moved out of folder",
                )
                .to_owned();
        }
        changed
    }

    fn render_layers_tab(&mut self, ui: &mut egui::Ui) {
        let language = self.language;
        let shift = ui.input(|i| i.modifiers.shift);
        let mut toggle_folder: Option<u64> = None;
        let mut toggle_folder_visibility: Option<u64> = None;
        let mut toggle_empty_visibility: Option<u64> = None;
        let mut select_empty_layer: Option<u64> = None;
        let mut select_layer_folder: Option<u64> = None;
        let mut begin_layer_item_edit: Option<u64> = None;
        let mut commit_layer_item_edit = false;
        let mut move_to_folder: Option<(LayerDragPayload, u64)> = None;
        let mut move_to_root: Option<(LayerDragPayload, Option<usize>)> = None;
        let grouped_faces: std::collections::HashSet<usize> = self
            .doc
            .layer_folders
            .iter()
            .flat_map(|folder| folder.face_indices.iter().copied())
            .collect();

        egui::ScrollArea::vertical()
            .id_salt("layer-list")
            .show(ui, |ui| {
                for folder in self.doc.layer_folders.clone().into_iter().rev() {
                    let folder_selected = self.selected_layer_folders.contains(&folder.id);
                    row_frame(ui, folder_selected, |ui| {
                        if disclosure_button(ui, folder.expanded).clicked() {
                            toggle_folder = Some(folder.id);
                        }
                        panel_glyph(ui, PanelGlyph::Folder);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if eye_button(ui, folder.visible).clicked() {
                                toggle_folder_visibility = Some(folder.id);
                            }
                            let width = ui.available_width();
                            if self.layer_item_editor == Some(folder.id) {
                                if let Some(target) = self
                                    .doc
                                    .layer_folders
                                    .iter_mut()
                                    .find(|target| target.id == folder.id)
                                {
                                    let response = ui.add_sized(
                                        [width, 22.0],
                                        egui::TextEdit::singleline(&mut target.name),
                                    );
                                    response.request_focus();
                                    if response.lost_focus()
                                        || ui.input(|input| input.key_pressed(Key::Enter))
                                    {
                                        commit_layer_item_edit = true;
                                    }
                                }
                            } else {
                                let item_count = folder.face_indices.len()
                                    + self
                                        .doc
                                        .empty_layers
                                        .iter()
                                        .filter(|layer| layer.folder_id == Some(folder.id))
                                        .count();
                                let kind = if language == UiLanguage::Japanese {
                                    format!("フォルダ・{item_count}項目")
                                } else if item_count == 1 {
                                    "Folder · 1 item".to_owned()
                                } else {
                                    format!("Folder · {item_count} items")
                                };
                                let response = layer_item_label(
                                    ui,
                                    width,
                                    &folder.name,
                                    &kind,
                                    folder_selected,
                                );
                                if response.dnd_hover_payload::<LayerDragPayload>().is_some() {
                                    let drop_rect = ui.max_rect().shrink2(Vec2::new(0.0, 1.0));
                                    ui.painter().rect_stroke(
                                        drop_rect,
                                        5.0,
                                        Stroke::new(1.0, self.theme_colors().accent),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if let Some(payload) =
                                    response.dnd_release_payload::<LayerDragPayload>()
                                {
                                    move_to_folder = Some((*payload, folder.id));
                                }
                                if response.clicked() {
                                    select_layer_folder = Some(folder.id);
                                }
                                if response.double_clicked() {
                                    begin_layer_item_edit = Some(folder.id);
                                }
                            }
                        });
                    });
                    if folder.expanded {
                        for face_idx in folder.face_indices.iter().rev().copied() {
                            if face_idx >= self.doc.faces.len() {
                                continue;
                            }
                            let selected = self.is_selected(Selection::Face(face_idx));
                            let editing = self.face_editor == Some(face_idx);
                            let name = if self.doc.faces[face_idx].name.is_empty() {
                                format!("{} {}", language.text("レイヤー", "Layer"), face_idx + 1)
                            } else {
                                self.doc.faces[face_idx].name.clone()
                            };
                            let kind = if self.doc.faces[face_idx].baked.is_some() {
                                language.text("結合シェイプ", "Merged shape")
                            } else {
                                language.text("シェイプ", "Shape")
                            };
                            row_frame(ui, selected, |ui| {
                                ui.add_space(17.0);
                                self.face_preview_widget(ui, face_idx, 20.0);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let visible = self.doc.faces[face_idx].visible;
                                        if eye_button(ui, visible).clicked() {
                                            let snapshot = self.doc.clone();
                                            self.doc.faces[face_idx].visible = !visible;
                                            self.push_history_snapshot(snapshot);
                                        }
                                        let width = ui.available_width();
                                        if editing {
                                            let response = ui.add_sized(
                                                [width, 22.0],
                                                egui::TextEdit::singleline(
                                                    &mut self.doc.faces[face_idx].name,
                                                ),
                                            );
                                            if response.lost_focus() {
                                                self.commit_face_name_edit();
                                            }
                                            response.request_focus();
                                        } else {
                                            let response =
                                                layer_item_label(ui, width, &name, kind, selected);
                                            layer_drag_source(
                                                ui,
                                                &response,
                                                LayerDragPayload::Face(face_idx),
                                                &name,
                                            );
                                            if response.clicked() {
                                                if shift {
                                                    self.toggle_selection(Selection::Face(
                                                        face_idx,
                                                    ));
                                                } else {
                                                    self.set_single_selection(Some(
                                                        Selection::Face(face_idx),
                                                    ));
                                                }
                                            }
                                            if response.double_clicked() {
                                                self.begin_face_edit(face_idx);
                                            }
                                        }
                                    },
                                );
                            });
                        }
                        let folder_empty_layers: Vec<_> = self
                            .doc
                            .empty_layers
                            .iter()
                            .filter(|layer| layer.folder_id == Some(folder.id))
                            .cloned()
                            .collect();
                        for empty in folder_empty_layers {
                            let selected = self.selected_empty_layers.contains(&empty.id);
                            row_frame(ui, selected, |ui| {
                                ui.add_space(17.0);
                                panel_glyph(ui, PanelGlyph::EmptyLayer);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if eye_button(ui, empty.visible).clicked() {
                                            toggle_empty_visibility = Some(empty.id);
                                        }
                                        let width = ui.available_width();
                                        if self.layer_item_editor == Some(empty.id) {
                                            if let Some(target) = self
                                                .doc
                                                .empty_layers
                                                .iter_mut()
                                                .find(|target| target.id == empty.id)
                                            {
                                                let response = ui.add_sized(
                                                    [width, 22.0],
                                                    egui::TextEdit::singleline(&mut target.name),
                                                );
                                                response.request_focus();
                                                if response.lost_focus()
                                                    || ui.input(|input| {
                                                        input.key_pressed(Key::Enter)
                                                    })
                                                {
                                                    commit_layer_item_edit = true;
                                                }
                                            }
                                        } else {
                                            let response = layer_item_label(
                                                ui,
                                                width,
                                                &empty.name,
                                                language.text("空レイヤー", "Empty layer"),
                                                selected,
                                            );
                                            layer_drag_source(
                                                ui,
                                                &response,
                                                LayerDragPayload::EmptyLayer(empty.id),
                                                &empty.name,
                                            );
                                            if response.clicked() {
                                                select_empty_layer = Some(empty.id);
                                            }
                                            if response.double_clicked() {
                                                begin_layer_item_edit = Some(empty.id);
                                            }
                                        }
                                    },
                                );
                            });
                        }
                    }
                }
                for i in (0..self.doc.faces.len()).rev() {
                    if grouped_faces.contains(&i) {
                        continue;
                    }
                    if let Some(payload) = layer_root_drop_zone(ui) {
                        move_to_root = Some((payload, Some(i + 1)));
                    }
                    let name = if self.doc.faces[i].name.is_empty() {
                        format!("{} {}", language.text("レイヤー", "Layer"), i + 1)
                    } else {
                        self.doc.faces[i].name.clone()
                    };
                    let selected = self.is_selected(Selection::Face(i));
                    let editing = self.face_editor == Some(i);
                    let kind = if self.doc.faces[i].baked.is_some() {
                        language.text("結合シェイプ", "Merged shape")
                    } else {
                        language.text("シェイプ", "Shape")
                    };
                    row_frame(ui, selected, |ui| {
                        self.face_preview_widget(ui, i, 20.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let visible = self.doc.faces[i].visible;
                            if eye_button(ui, visible).clicked() {
                                let snapshot = self.doc.clone();
                                self.doc.faces[i].visible = !visible;
                                self.push_history_snapshot(snapshot);
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
                                let resp = layer_item_label(ui, w, &name, kind, selected);
                                layer_drag_source(ui, &resp, LayerDragPayload::Face(i), &name);
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
                if let Some(payload) = layer_root_drop_zone(ui) {
                    move_to_root = Some((payload, None));
                }
                let root_empty_layers: Vec<_> = self
                    .doc
                    .empty_layers
                    .iter()
                    .filter(|layer| layer.folder_id.is_none())
                    .cloned()
                    .collect();
                for empty in root_empty_layers {
                    if let Some(payload) = layer_root_drop_zone(ui) {
                        move_to_root = Some((payload, None));
                    }
                    let selected = self.selected_empty_layers.contains(&empty.id);
                    row_frame(ui, selected, |ui| {
                        panel_glyph(ui, PanelGlyph::EmptyLayer);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if eye_button(ui, empty.visible).clicked() {
                                toggle_empty_visibility = Some(empty.id);
                            }
                            let width = ui.available_width();
                            if self.layer_item_editor == Some(empty.id) {
                                if let Some(target) = self
                                    .doc
                                    .empty_layers
                                    .iter_mut()
                                    .find(|target| target.id == empty.id)
                                {
                                    let response = ui.add_sized(
                                        [width, 22.0],
                                        egui::TextEdit::singleline(&mut target.name),
                                    );
                                    response.request_focus();
                                    if response.lost_focus()
                                        || ui.input(|input| input.key_pressed(Key::Enter))
                                    {
                                        commit_layer_item_edit = true;
                                    }
                                }
                            } else {
                                let response = layer_item_label(
                                    ui,
                                    width,
                                    &empty.name,
                                    language.text("空レイヤー", "Empty layer"),
                                    selected,
                                );
                                layer_drag_source(
                                    ui,
                                    &response,
                                    LayerDragPayload::EmptyLayer(empty.id),
                                    &empty.name,
                                );
                                if response.clicked() {
                                    select_empty_layer = Some(empty.id);
                                }
                                if response.double_clicked() {
                                    begin_layer_item_edit = Some(empty.id);
                                }
                            }
                        });
                    });
                }
            });
        if let Some((payload, folder_id)) = move_to_folder {
            self.move_layer_item_to_folder(payload, folder_id);
        }
        if let Some((payload, before_face)) = move_to_root {
            self.move_layer_item_to_root(payload, before_face);
        }
        if let Some(id) = select_empty_layer {
            self.select_empty_layer(id, shift);
        }
        if let Some(id) = select_layer_folder {
            self.select_layer_folder(id, shift);
        }
        if let Some(id) = begin_layer_item_edit {
            self.begin_layer_item_edit(id);
        }
        if commit_layer_item_edit {
            self.commit_layer_item_edit();
        }
        if let Some(id) = toggle_folder {
            if let Some(folder) = self
                .doc
                .layer_folders
                .iter_mut()
                .find(|folder| folder.id == id)
            {
                folder.expanded = !folder.expanded;
            }
        }
        if let Some(id) = toggle_folder_visibility {
            let snapshot = self.doc.clone();
            if let Some(folder) = self
                .doc
                .layer_folders
                .iter_mut()
                .find(|folder| folder.id == id)
            {
                folder.visible = !folder.visible;
                for face_idx in &folder.face_indices {
                    if let Some(face) = self.doc.faces.get_mut(*face_idx) {
                        face.visible = folder.visible;
                    }
                }
                for layer in &mut self.doc.empty_layers {
                    if layer.folder_id == Some(folder.id) {
                        layer.visible = folder.visible;
                    }
                }
                self.push_history_snapshot(snapshot);
            }
        }
        if let Some(id) = toggle_empty_visibility {
            let snapshot = self.doc.clone();
            if let Some(layer) = self
                .doc
                .empty_layers
                .iter_mut()
                .find(|layer| layer.id == id)
            {
                layer.visible = !layer.visible;
                self.push_history_snapshot(snapshot);
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            if layer_action_button(
                ui,
                LayerActionGlyph::Plus,
                language.text("レイヤー", "Layer"),
            )
            .on_hover_text(language.text("空レイヤーを追加", "Add an empty layer"))
            .clicked()
            {
                self.add_empty_layer();
            }
            if layer_action_button(
                ui,
                LayerActionGlyph::Folder,
                language.text("フォルダ", "Folder"),
            )
            .on_hover_text(language.text(
                "選択したレイヤーをフォルダへまとめる",
                "Group selected layers in a folder",
            ))
            .clicked()
            {
                self.add_layer_folder();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_layer_selection = self
                    .selection
                    .iter()
                    .any(|selection| matches!(selection, Selection::Face(_)))
                    || !self.selected_empty_layers.is_empty()
                    || !self.selected_layer_folders.is_empty();
                let response = ui
                    .add_enabled_ui(has_layer_selection, |ui| {
                        layer_action_button(
                            ui,
                            LayerActionGlyph::Trash,
                            language.text("削除", "Delete"),
                        )
                    })
                    .inner
                    .on_hover_text(language.text("選択項目を削除", "Delete selected items"));
                if response.clicked() {
                    self.delete_selected();
                }
            });
        });
    }

    fn render_circles_tab(&mut self, ui: &mut egui::Ui) {
        let language = self.language;
        let shift = ui.input(|i| i.modifiers.shift);
        ui.add_space(2.0);
        let mut remove_custom: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("custom-list")
            .show(ui, |ui| {
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
                                delete_button(ui).on_hover_text(language.text(
                                    "この円は面に使われています",
                                    "This peg is used by a shape",
                                ));
                            } else if delete_button(ui).clicked() {
                                remove_custom = Some(i);
                            }
                            let w = ui.available_width();
                            let txt = if selected {
                                egui::RichText::new(format!("{} #{id}", language.text("円", "Peg")))
                                    .color(ui.visuals().strong_text_color())
                            } else {
                                egui::RichText::new(format!("{} #{id}", language.text("円", "Peg")))
                            };
                            let resp = ui.add_sized(
                                [w, 22.0],
                                egui::Label::new(txt)
                                    .sense(Sense::click())
                                    .selectable(false),
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
        let language = self.language;
        let folder_ids: Vec<u64> = self.selected_layer_folders.iter().copied().collect();
        let empty_layer_ids: Vec<u64> = self.selected_empty_layers.iter().copied().collect();
        let face_indices: Vec<_> = self
            .selection
            .iter()
            .filter_map(|sel| {
                if let Selection::Face(i) = sel {
                    Some(*i)
                } else {
                    None
                }
            })
            .collect();
        let peg_indices: Vec<_> = self
            .selection
            .iter()
            .filter_map(|sel| {
                if let Selection::Peg(id) = sel {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        ui.add_space(2.0);
        if !folder_ids.is_empty() {
            if folder_ids.len() > 1 {
                section_header(
                    ui,
                    &format!(
                        "{} {}",
                        language.text("フォルダ", "Folders"),
                        folder_ids.len()
                    ),
                );
                ui.horizontal(|ui| {
                    for (visible, label) in [
                        (true, language.text("すべて表示", "Show all")),
                        (false, language.text("すべて非表示", "Hide all")),
                    ] {
                        if ui.button(label).clicked() {
                            let snapshot = self.doc.clone();
                            let mut changed = false;
                            for folder in &mut self.doc.layer_folders {
                                if folder_ids.contains(&folder.id) {
                                    changed |= folder.visible != visible;
                                    folder.visible = visible;
                                    for face_idx in &folder.face_indices {
                                        if let Some(face) = self.doc.faces.get_mut(*face_idx) {
                                            face.visible = visible;
                                        }
                                    }
                                    for layer in &mut self.doc.empty_layers {
                                        if layer.folder_id == Some(folder.id) {
                                            layer.visible = visible;
                                        }
                                    }
                                }
                            }
                            if changed {
                                self.push_history_snapshot(snapshot);
                            }
                        }
                    }
                });
            } else if let Some(folder_idx) = self
                .doc
                .layer_folders
                .iter()
                .position(|folder| folder.id == folder_ids[0])
            {
                let item_count = self.doc.layer_folders[folder_idx].face_indices.len()
                    + self
                        .doc
                        .empty_layers
                        .iter()
                        .filter(|layer| layer.folder_id == Some(folder_ids[0]))
                        .count();
                let title = self.doc.layer_folders[folder_idx].name.clone();
                section_header(ui, &title);
                ui.label(
                    egui::RichText::new(if language == UiLanguage::Japanese {
                        format!("フォルダ・{item_count}項目")
                    } else if item_count == 1 {
                        "Folder · 1 item".to_owned()
                    } else {
                        format!("Folder · {item_count} items")
                    })
                    .small()
                    .color(self.theme_colors().text2),
                );
                let before_name = self.doc.clone();
                let name_response = ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(language.text("名前", "Name"))
                            .weak()
                            .small(),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.doc.layer_folders[folder_idx].name)
                            .desired_width(ui.available_width()),
                    )
                });
                if name_response.inner.changed() && self.layer_item_name_snapshot.is_none() {
                    self.layer_item_name_snapshot = Some(before_name);
                }
                if name_response.inner.lost_focus() {
                    self.commit_layer_item_edit();
                }
                let visibility_snapshot = self.doc.clone();
                let mut visible = self.doc.layer_folders[folder_idx].visible;
                if ui
                    .checkbox(&mut visible, language.text("表示", "Visible"))
                    .changed()
                {
                    self.doc.layer_folders[folder_idx].visible = visible;
                    let face_indices = self.doc.layer_folders[folder_idx].face_indices.clone();
                    for face_idx in face_indices {
                        if let Some(face) = self.doc.faces.get_mut(face_idx) {
                            face.visible = visible;
                        }
                    }
                    for layer in &mut self.doc.empty_layers {
                        if layer.folder_id == Some(folder_ids[0]) {
                            layer.visible = visible;
                        }
                    }
                    self.push_history_snapshot(visibility_snapshot);
                }
            }
            return;
        }

        if !empty_layer_ids.is_empty() {
            if empty_layer_ids.len() > 1 {
                section_header(
                    ui,
                    &format!(
                        "{} {}",
                        language.text("空レイヤー", "Empty layers"),
                        empty_layer_ids.len()
                    ),
                );
                ui.horizontal(|ui| {
                    for (visible, label) in [
                        (true, language.text("すべて表示", "Show all")),
                        (false, language.text("すべて非表示", "Hide all")),
                    ] {
                        if ui.button(label).clicked() {
                            let snapshot = self.doc.clone();
                            let mut changed = false;
                            for layer in &mut self.doc.empty_layers {
                                if empty_layer_ids.contains(&layer.id) {
                                    changed |= layer.visible != visible;
                                    layer.visible = visible;
                                }
                            }
                            if changed {
                                self.push_history_snapshot(snapshot);
                            }
                        }
                    }
                });
            } else if let Some(layer_idx) = self
                .doc
                .empty_layers
                .iter()
                .position(|layer| layer.id == empty_layer_ids[0])
            {
                let title = self.doc.empty_layers[layer_idx].name.clone();
                section_header(ui, &title);
                ui.label(
                    egui::RichText::new(language.text("空レイヤー", "Empty layer"))
                        .small()
                        .color(self.theme_colors().text2),
                );
                let before_name = self.doc.clone();
                let name_response = ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(language.text("名前", "Name"))
                            .weak()
                            .small(),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.doc.empty_layers[layer_idx].name)
                            .desired_width(ui.available_width()),
                    )
                });
                if name_response.inner.changed() && self.layer_item_name_snapshot.is_none() {
                    self.layer_item_name_snapshot = Some(before_name);
                }
                if name_response.inner.lost_focus() {
                    self.commit_layer_item_edit();
                }
                let visibility_snapshot = self.doc.clone();
                if ui
                    .checkbox(
                        &mut self.doc.empty_layers[layer_idx].visible,
                        language.text("表示", "Visible"),
                    )
                    .changed()
                {
                    self.push_history_snapshot(visibility_snapshot);
                }
                let location = self.doc.empty_layers[layer_idx]
                    .folder_id
                    .and_then(|id| {
                        self.doc
                            .layer_folders
                            .iter()
                            .find(|folder| folder.id == id)
                            .map(|folder| folder.name.as_str())
                    })
                    .unwrap_or_else(|| language.text("最上位", "Root"));
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {location}",
                        language.text("場所", "Location")
                    ))
                    .small()
                    .color(self.theme_colors().text2),
                );
            }
            return;
        }

        if let Some(peg_id) = peg_indices.first().copied() {
            let Some(peg_idx) = self.doc.custom_pegs.iter().position(|peg| peg.id == peg_id) else {
                return;
            };
            section_header(
                ui,
                &format!("{} #{peg_id}", language.text("任意円", "Custom peg")),
            );
            let mut x = self.doc.custom_pegs[peg_idx].center.x;
            let mut y = self.doc.custom_pegs[peg_idx].center.y;
            let mut r = self.doc.custom_pegs[peg_idx].radius;
            let x_resp = scrub_value(ui, "X", &mut x, 0.5, -5000.0..=5000.0);
            let y_resp = scrub_value(ui, "Y", &mut y, 0.5, -5000.0..=5000.0);
            let r_resp = scrub_value(
                ui,
                language.text("半径", "Radius"),
                &mut r,
                0.5,
                0.0..=120.0,
            );

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
            if self.workspace_mode == WorkspaceMode::Font {
                self.render_glyphs_tab(ui);
                return;
            }
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(self.language.text("選択なし", "No selection")).weak(),
                );
            });
            return;
        }
        if face_indices.len() > 1 {
            section_header(
                ui,
                &format!(
                    "{} {}",
                    language.text("レイヤー", "Layers"),
                    face_indices.len()
                ),
            );
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    egui::Button::new(
                        language.text("選択したレイヤーをマージ", "Merge selected layers"),
                    ),
                )
                .on_hover_text(language.text(
                    "選択したレイヤーをパス結合し、丸ベベル可能な1枚のシェイプにします",
                    "Union the selection into one shape that supports round bevel",
                ))
                .clicked()
            {
                self.merge_selected();
                return;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(language.text("すべて表示", "Show all")).clicked() {
                    let snapshot = self.doc.clone();
                    let mut changed = false;
                    for &face_idx in &face_indices {
                        if let Some(face) = self.doc.faces.get_mut(face_idx) {
                            changed |= !face.visible;
                            face.visible = true;
                        }
                    }
                    if changed {
                        self.push_history_snapshot(snapshot);
                    }
                    self.status = "選択中のレイヤーを表示".to_owned();
                }
                if ui
                    .button(language.text("すべて非表示", "Hide all"))
                    .clicked()
                {
                    let snapshot = self.doc.clone();
                    let mut changed = false;
                    for &face_idx in &face_indices {
                        if let Some(face) = self.doc.faces.get_mut(face_idx) {
                            changed |= face.visible;
                            face.visible = false;
                        }
                    }
                    if changed {
                        self.push_history_snapshot(snapshot);
                    }
                    self.status = "選択中のレイヤーを非表示".to_owned();
                }
            });
            ui.add_space(8.0);
            section_header(ui, language.text("整列", "Align"));
            let mut align = None;
            ui.horizontal(|ui| {
                for (axis, tooltip) in [
                    (AlignAxis::Left, language.text("左揃え", "Align left")),
                    (
                        AlignAxis::CenterX,
                        language.text("左右中央揃え", "Align horizontal centers"),
                    ),
                    (AlignAxis::Right, language.text("右揃え", "Align right")),
                    (AlignAxis::Top, language.text("上揃え", "Align top")),
                    (
                        AlignAxis::CenterY,
                        language.text("上下中央揃え", "Align vertical centers"),
                    ),
                    (AlignAxis::Bottom, language.text("下揃え", "Align bottom")),
                ] {
                    if align_icon_button(ui, axis, tooltip).clicked() {
                        align = Some(axis);
                    }
                }
            });
            if let Some(axis) = align {
                self.align_selected_faces(axis);
            }
            return;
        }

        if let Some(&face_idx) = face_indices.first() {
            if face_idx >= self.doc.faces.len() {
                return;
            }
            let title = match self.doc.faces.get(face_idx) {
                Some(f) if !f.name.is_empty() => f.name.clone(),
                _ => format!("{} {}", language.text("レイヤー", "Layer"), face_idx + 1),
            };
            section_header(ui, &title);
            // 名前（インライン編集）
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(language.text("名前", "Name"))
                        .weak()
                        .small(),
                );
                if let Some(face) = self.doc.faces.get_mut(face_idx) {
                    ui.add(
                        egui::TextEdit::singleline(&mut face.name)
                            .hint_text(language.text("（無題）", "Untitled"))
                            .desired_width(ui.available_width()),
                    );
                }
            });
            let visibility_snapshot = self.doc.clone();
            if let Some(face) = self.doc.faces.get_mut(face_idx) {
                if ui
                    .checkbox(&mut face.visible, language.text("表示", "Visible"))
                    .changed()
                {
                    self.push_history_snapshot(visibility_snapshot);
                }
            }
            // マージ済みレイヤーは丸ベベル量を非破壊で調整できる
            let is_baked = self
                .doc
                .faces
                .get(face_idx)
                .map(|f| f.baked.is_some())
                .unwrap_or(false);
            if is_baked {
                ui.add_space(6.0);
                let mut bevel = self.doc.faces[face_idx].baked.as_ref().unwrap().bevel;
                let resp = mock_slider_row_f64(
                    ui,
                    language.text("丸ベベル", "Round bevel"),
                    &mut bevel,
                    0.0..=1000.0,
                    1,
                    "",
                )
                .on_hover_text(language.text(
                    "最大1000。実際の丸みは各辺長の49%まで安全に制限されます",
                    "Maximum 1000; each corner is limited to 49% of its adjacent edges",
                ));
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
        let language = self.language;
        ui.vertical(|ui| {
            let before_doc = self.doc.clone();
            let before_grid = self.doc.grid;
            section_header(ui, language.text("基本グリッド", "Grid geometry"));
            mock_slider_row_f64(
                ui,
                language.text("間隔", "Spacing"),
                &mut self.doc.grid.spacing,
                40.0..=320.0,
                1,
                "",
            );
            mock_slider_row_f64(
                ui,
                language.text("引っかかり範囲", "Snap range"),
                &mut self.doc.grid.catch,
                0.0..=24.0,
                1,
                "",
            );
            let (main_max, _) = safe_grid_radius_limits(&self.doc.grid);
            mock_slider_row_f64(
                ui,
                language.text("半径", "Radius"),
                &mut self.doc.grid.radius,
                0.0..=main_max,
                1,
                "",
            );
            if ENABLE_EXPERIMENTAL_SQUARE_PEGS {
                ui.horizontal(|ui| {
                    ui.label(language.text("ピン形状", "Peg shape"));
                    ui.radio_value(
                        &mut self.doc.peg_appearance.shape,
                        crate::model::PegShape::Circle,
                        "円",
                    );
                    ui.radio_value(
                        &mut self.doc.peg_appearance.shape,
                        crate::model::PegShape::RoundedSquare,
                        "四角",
                    );
                });
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    mock_slider_row_f64(
                        ui,
                        language.text("角のラウンド", "Corner roundness"),
                        &mut self.doc.peg_appearance.rounding,
                        0.0..=100.0,
                        1,
                        "",
                    );
                }
            }
            ui.add_space(6.0);
            section_header(ui, language.text("サブ円", "Sub pegs"));
            ui.checkbox(
                &mut self.doc.grid.subs_enabled,
                language.text("サブ円を使う", "Enable sub pegs"),
            );
            if !self.doc.disabled_pegs.is_empty() {
                let count = self.doc.disabled_pegs.len();
                if ui
                    .button(format!(
                        "{} ({count})",
                        language.text("無効化した円をすべて戻す", "Restore disabled pegs")
                    ))
                    .clicked()
                {
                    self.push_history();
                    self.doc.disabled_pegs.clear();
                    self.rebuild_pegs(self.canvas_size);
                    self.status = "無効化したグリッド円を戻しました".to_owned();
                }
            }
            let (_, sub_max) = safe_grid_radius_limits(&self.doc.grid);
            ui.add_enabled_ui(self.doc.grid.subs_enabled, |ui| {
                mock_slider_row_f64(
                    ui,
                    language.text("半径", "Radius"),
                    &mut self.doc.grid.sub_radius,
                    0.0..=sub_max,
                    1,
                    "",
                );
            });
            if self.doc.grid.subs_enabled {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {sub_max:.1}",
                        language.text("重ならない上限", "Non-overlap limit")
                    ))
                    .small()
                    .color(colors.text2),
                );
            }
            ui.add_space(6.0);
            section_header(ui, language.text("拡縮", "Scaling"));
            ui.checkbox(
                &mut self.doc.grid.scale_link,
                language.text(
                    "間隔に合わせて円の大きさも変える",
                    "Scale peg size with spacing",
                ),
            );
            ui.add_enabled_ui(self.doc.grid.scale_link, |ui| {
                ui.label(language.text("拡縮の基準", "Scale anchor"));
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut self.doc.grid.scale_anchor,
                        ScaleAnchor::TopLeft,
                        language.text("左上", "Top left"),
                    );
                    ui.radio_value(
                        &mut self.doc.grid.scale_anchor,
                        ScaleAnchor::Center,
                        language.text("中央", "Center"),
                    );
                });
            });
            if self.doc.grid != before_grid && self.draw.is_some() {
                self.draw = None;
                self.status = "グリッド変更のため描画中の線をキャンセルしました".to_owned();
            }
            if self.doc.grid.scale_link
                && before_grid.spacing != 0.0
                && self.doc.grid.spacing != before_grid.spacing
            {
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
            if let Some(source) = self.grid_edit_orig.clone() {
                if Self::remap_baked_shapes_for_grid_change(
                    &mut self.doc,
                    &source,
                    self.canvas_center,
                ) {
                    self.status = language
                        .text(
                            "結合シェイプもグリッド間隔に追従しました",
                            "Merged shapes followed the grid spacing",
                        )
                        .to_owned();
                }
            }
            if self.grid_edit_orig.is_some() && !ui.input(|input| input.pointer.primary_down()) {
                if let Some(snapshot) = self.grid_edit_orig.take() {
                    self.push_history_snapshot(snapshot);
                }
            }
            ui.add_space(8.0);
            ui.separator();
            let clear_label = match self.workspace_mode {
                WorkspaceMode::Shape => language.text("シェイプをクリア", "Clear shape"),
                WorkspaceMode::Font => language.text("グリフをクリア", "Clear glyph"),
            };
            if ui.button(clear_label).clicked() {
                self.clear_glyph();
            }
        });
    }

    fn draw_font_metric_guides(&self, painter: &egui::Painter, origin: Pos2) {
        if self.workspace_mode != WorkspaceMode::Font {
            return;
        }
        let metrics = self.font_project.metrics;
        let left = metrics.origin_x;
        let right = metrics.right_x;
        let colors = self.theme_colors();
        let box_color = colors.accent.gamma_multiply(0.42);
        let secondary = colors.text2.gamma_multiply(0.48);
        let baseline_color = Color32::from_rgb(235, 116, 116).gamma_multiply(0.72);
        let x1 = self.to_screen(origin, Point::new(left, 0.0)).x;
        let x2 = self.to_screen(origin, Point::new(right, 0.0)).x;
        let y_asc = self
            .to_screen(origin, Point::new(0.0, metrics.ascender_y))
            .y;
        let y_x = self
            .to_screen(origin, Point::new(0.0, metrics.x_height_y))
            .y;
        let y_base = self
            .to_screen(origin, Point::new(0.0, metrics.baseline_y))
            .y;
        let y_desc = self
            .to_screen(origin, Point::new(0.0, metrics.descender_y))
            .y;

        painter.rect_stroke(
            Rect::from_min_max(Pos2::new(x1, y_asc), Pos2::new(x2, y_desc)),
            0.0,
            Stroke::new(1.0, box_color),
            egui::StrokeKind::Inside,
        );
        painter.add(egui::Shape::dashed_line(
            &[Pos2::new(x1, y_x), Pos2::new(x2, y_x)],
            Stroke::new(0.9, secondary),
            5.0,
            4.0,
        ));
        painter.add(egui::Shape::dashed_line(
            &[Pos2::new(x1, y_base), Pos2::new(x2, y_base)],
            Stroke::new(1.15, baseline_color),
            7.0,
            4.0,
        ));
        for (label, y, color) in [
            ("ascender", y_asc, box_color),
            ("x-height", y_x, secondary),
            ("baseline", y_base, baseline_color),
            ("descender", y_desc, box_color),
        ] {
            let label_rect = Rect::from_min_size(
                Pos2::new(x1 + 5.0, y - 8.0),
                Vec2::new(label.len() as f32 * 5.7 + 10.0, 16.0),
            );
            painter.rect_filled(label_rect, 3.0, colors.panel);
            painter.rect_stroke(
                label_rect,
                3.0,
                Stroke::new(0.7, color.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
            painter.text(
                label_rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                color,
            );
        }
    }

    fn read_reference_font(path: PathBuf, preferred_face_index: u32) -> Option<ReferenceFont> {
        let data = std::fs::read(&path).ok()?;
        let face_count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        let face_index = if preferred_face_index < face_count
            && ttf_parser::Face::parse(&data, preferred_face_index).is_ok()
        {
            preferred_face_index
        } else {
            (0..face_count).find(|index| ttf_parser::Face::parse(&data, *index).is_ok())?
        };
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Font")
            .to_owned();
        Some(ReferenceFont {
            name,
            data,
            face_index,
            path,
        })
    }

    fn choose_reference_font(&mut self) {
        let mut dialog =
            rfd::FileDialog::new().add_filter("OpenType font", &["ttf", "otf", "ttc", "otc"]);
        dialog = dialog.set_directory("/System/Library/Fonts");
        let Some(path) = dialog.pick_file() else {
            return;
        };
        if let Some(font) = Self::read_reference_font(path.clone(), 0) {
            let name = font.name.clone();
            self.reference_font = Some(font);
            self.reference_font_offset = Vec2::ZERO;
            self.status = format!("{name} を下書きに設定しました");
        } else if path.exists() {
            self.status = "このフォントの輪郭を読み取れませんでした".to_owned();
        } else {
            self.status = "フォントを開けません".to_owned();
        }
    }

    fn draw_reference_font(&self, painter: &egui::Painter, origin: Pos2) {
        if self.workspace_mode != WorkspaceMode::Font {
            return;
        }
        let Some(reference) = &self.reference_font else {
            return;
        };
        let Some(glyph) = self.font_project.glyphs.get(self.font_project.active_glyph) else {
            return;
        };
        let Ok(face) = ttf_parser::Face::parse(&reference.data, reference.face_index) else {
            return;
        };
        let Some(glyph_id) = face.glyph_index(glyph.character) else {
            return;
        };
        let mut builder = ReferenceOutlineBuilder::default();
        if face.outline_glyph(glyph_id, &mut builder).is_none() {
            return;
        }
        builder.finish_contour();
        let metrics = self.font_project.metrics;
        let em_world = (metrics.descender_y - metrics.ascender_y).max(40.0);
        let scale =
            em_world / f64::from(face.units_per_em()) * f64::from(self.reference_font_scale);
        let color = self
            .theme_colors()
            .accent
            .gamma_multiply(self.reference_font_opacity);
        for contour in builder.contours {
            if contour.len() < 2 {
                continue;
            }
            let mut points: Vec<Pos2> = contour
                .into_iter()
                .map(|point| {
                    self.to_screen(
                        origin,
                        Point::new(
                            metrics.origin_x
                                + point.x * scale
                                + f64::from(self.reference_font_offset.x),
                            metrics.baseline_y - point.y * scale
                                + f64::from(self.reference_font_offset.y),
                        ),
                    )
                })
                .collect();
            if points.len() > 2 {
                points.push(points[0]);
            }
            painter.add(egui::Shape::line(points, Stroke::new(1.1, color)));
        }
    }

    fn glyph_ring_sets(&self, glyph_index: usize) -> Option<(char, Vec<Vec<Vec<Point>>>)> {
        if self.workspace_mode != WorkspaceMode::Font {
            return None;
        }
        let glyph = self.font_project.glyphs.get(glyph_index)?;
        // 編集中グリフは保存スナップショットではなく現在のキャンバスを使い、
        // 試打とオニオンへ操作結果を即時反映する。
        let document = if glyph_index == self.font_project.active_glyph {
            &self.doc
        } else {
            &glyph.document
        };
        if document.faces.is_empty() {
            return Some((glyph.character, Vec::new()));
        }
        let (pegs, peg_refs) = Self::engine_pegs(
            &document.grid,
            self.canvas_size,
            self.canvas_center,
            &document.custom_pegs,
            document.peg_appearance,
            &document.disabled_pegs,
        );
        let resolver = |reference: &PegRef| {
            resolve_peg(
                &document.grid,
                &document.custom_pegs,
                self.canvas_center,
                document.peg_appearance,
                reference,
            )
        };
        let mut ring_sets = Vec::new();
        for face in document.faces.iter().filter(|face| face.visible) {
            if let Some(baked) = &face.baked {
                let rings: Vec<Vec<Point>> = baked
                    .rings
                    .iter()
                    .map(|ring| bevel_ring(ring, baked.bevel))
                    .filter(|ring| ring.len() >= 3)
                    .collect();
                if !rings.is_empty() {
                    ring_sets.push(rings);
                }
                continue;
            }
            let obstacles: Vec<(PegRef, Peg)> = peg_refs
                .iter()
                .copied()
                .zip(pegs.iter().copied())
                .filter(|(peg_ref, _)| !face.ignored_pegs.contains(peg_ref))
                .collect();
            let Some(segments) = materialize_face_with_obstacles(face, &resolver, &obstacles)
            else {
                continue;
            };
            let mut points = Vec::new();
            // 試打キャッシュがキャンバスのズームに依存しない固定精度。
            let tolerance: f64 = 0.2;
            for segment in segments {
                match segment {
                    Seg::Line { from, to } => {
                        if points.is_empty() {
                            points.push(from);
                        }
                        points.push(to);
                    }
                    Seg::Arc {
                        center,
                        radius,
                        start,
                        end,
                    } => {
                        let peg = pegs.iter().find(|peg| {
                            peg.center.distance(center) < 0.01 && (peg.radius - radius).abs() < 0.01
                        });
                        let safe_tolerance = tolerance.max(0.01).min(radius.max(0.01));
                        let step_angle = (2.0 * (1.0 - safe_tolerance / radius.max(0.01)).acos())
                            .clamp(0.01, std::f64::consts::PI / 2.0);
                        let steps =
                            (((end - start).abs() / step_angle).ceil() as usize).clamp(2, 160);
                        for index in 0..=steps {
                            let angle = start + (end - start) * (index as f64 / steps as f64);
                            let point = peg.map_or_else(
                                || crate::geometry::on_circle(center, radius, angle),
                                |peg| crate::geometry::on_peg(peg, angle),
                            );
                            if points
                                .last()
                                .is_none_or(|last: &Point| last.distance(point) > tolerance * 0.25)
                            {
                                points.push(point);
                            }
                        }
                    }
                }
            }
            if points.len() >= 3 {
                ring_sets.push(vec![points]);
            }
        }
        Some((glyph.character, ring_sets))
    }

    fn glyph_rings(&self, glyph_index: usize) -> Option<(char, Vec<Vec<Point>>)> {
        let (character, ring_sets) = self.glyph_ring_sets(glyph_index)?;
        Some((character, ring_sets.into_iter().flatten().collect()))
    }

    fn proof_glyph_rings(&mut self, glyph_index: usize) -> Option<(char, Arc<Vec<Vec<Point>>>)> {
        let glyph = self.font_project.glyphs.get(glyph_index)?;
        let character = glyph.character;
        let document = if glyph_index == self.font_project.active_glyph {
            self.doc.clone()
        } else {
            glyph.document.clone()
        };
        let style = self.font_project.render_style;
        let width = self.font_project.thread_width;
        if let Some(cached) = self.proof_cache.get(&glyph_index) {
            if cached.document == document
                && cached.canvas_size == self.canvas_size
                && cached.canvas_center == self.canvas_center
                && cached.render_style == style
                && (cached.thread_width - width).abs() < f64::EPSILON
            {
                return Some((character, Arc::clone(&cached.rings)));
            }
        }

        let (_, source_ring_sets) = self.glyph_ring_sets(glyph_index)?;
        let rings = match style {
            FontRenderStyle::Thread => stroke_visible_ring_sets(&source_ring_sets, width),
            FontRenderStyle::Fill => union_visible_ring_sets(&source_ring_sets),
            FontRenderStyle::FillAndThread => {
                let fill = union_visible_ring_sets(&source_ring_sets);
                let thread = stroke_visible_ring_sets(&source_ring_sets, width);
                union_polygon_ring_sets(&[fill, thread])
            }
        };
        let rings = Arc::new(rings);
        self.proof_cache.insert(
            glyph_index,
            ProofCacheEntry {
                document,
                canvas_size: self.canvas_size,
                canvas_center: self.canvas_center,
                render_style: style,
                thread_width: width,
                rings: Arc::clone(&rings),
            },
        );
        Some((character, rings))
    }

    fn draw_onion_skin(&self, painter: &egui::Painter, origin: Pos2) {
        if self.workspace_mode != WorkspaceMode::Font {
            return;
        }
        let active = self.font_project.active_glyph;
        let mut layers = Vec::new();
        if self.onion_skin && active > 0 {
            layers.push((active - 1, self.onion_previous_color, "前"));
        }
        if self.onion_next && active + 1 < self.font_project.glyphs.len() {
            layers.push((active + 1, self.onion_next_color, "次"));
        }
        let alpha = (self.onion_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        for (layer_number, (glyph_index, base_color, label)) in layers.into_iter().enumerate() {
            let Some((character, rings)) = self.glyph_rings(glyph_index) else {
                continue;
            };
            let color = Color32::from_rgba_unmultiplied(
                base_color.r(),
                base_color.g(),
                base_color.b(),
                alpha,
            );
            for ring in rings {
                let mut line: Vec<Pos2> = ring
                    .iter()
                    .map(|point| self.to_screen(origin, *point))
                    .collect();
                if line.len() >= 3 {
                    if line
                        .first()
                        .zip(line.last())
                        .is_some_and(|(a, b)| a.distance(*b) > 0.01)
                    {
                        line.push(line[0]);
                    }
                    painter.add(egui::Shape::line(line, Stroke::new(1.25, color)));
                }
            }
            let metrics = self.font_project.metrics;
            let label_position = self
                .to_screen(origin, Point::new(metrics.right_x, metrics.ascender_y))
                + Vec2::new(-6.0, 8.0 + layer_number as f32 * 14.0);
            painter.text(
                label_position,
                egui::Align2::RIGHT_TOP,
                format!("{label}: {character}"),
                FontId::proportional(10.0),
                color,
            );
        }
    }

    fn interact_font_metric_guides(&mut self, ui: &mut egui::Ui, origin: Pos2) -> bool {
        if self.workspace_mode != WorkspaceMode::Font {
            return false;
        }
        let metrics = self.font_project.metrics;
        let x1 = self.to_screen(origin, Point::new(metrics.origin_x, 0.0)).x;
        let x2 = self.to_screen(origin, Point::new(metrics.right_x, 0.0)).x;
        let y_asc = self
            .to_screen(origin, Point::new(0.0, metrics.ascender_y))
            .y;
        let y_x = self
            .to_screen(origin, Point::new(0.0, metrics.x_height_y))
            .y;
        let y_base = self
            .to_screen(origin, Point::new(0.0, metrics.baseline_y))
            .y;
        let y_desc = self
            .to_screen(origin, Point::new(0.0, metrics.descender_y))
            .y;
        let horizontal_min = x1.min(x2) - 6.0;
        let horizontal_max = x1.max(x2) + 6.0;
        let vertical_min = y_asc.min(y_desc) - 6.0;
        let vertical_max = y_asc.max(y_desc) + 6.0;
        let guides = [
            (
                FontMetricGuide::Origin,
                Rect::from_min_max(
                    Pos2::new(x1 - 6.0, vertical_min),
                    Pos2::new(x1 + 6.0, vertical_max),
                ),
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                FontMetricGuide::Right,
                Rect::from_min_max(
                    Pos2::new(x2 - 6.0, vertical_min),
                    Pos2::new(x2 + 6.0, vertical_max),
                ),
                egui::CursorIcon::ResizeHorizontal,
            ),
            (
                FontMetricGuide::Ascender,
                Rect::from_min_max(
                    Pos2::new(horizontal_min, y_asc - 6.0),
                    Pos2::new(horizontal_max, y_asc + 6.0),
                ),
                egui::CursorIcon::ResizeVertical,
            ),
            (
                FontMetricGuide::XHeight,
                Rect::from_min_max(
                    Pos2::new(horizontal_min, y_x - 6.0),
                    Pos2::new(horizontal_max, y_x + 6.0),
                ),
                egui::CursorIcon::ResizeVertical,
            ),
            (
                FontMetricGuide::Baseline,
                Rect::from_min_max(
                    Pos2::new(horizontal_min, y_base - 6.0),
                    Pos2::new(horizontal_max, y_base + 6.0),
                ),
                egui::CursorIcon::ResizeVertical,
            ),
            (
                FontMetricGuide::Descender,
                Rect::from_min_max(
                    Pos2::new(horizontal_min, y_desc - 6.0),
                    Pos2::new(horizontal_max, y_desc + 6.0),
                ),
                egui::CursorIcon::ResizeVertical,
            ),
        ];

        let mut interacting = false;
        for (guide, hit_rect, cursor) in guides {
            let response = ui
                .interact(
                    hit_rect,
                    ui.make_persistent_id(("font-metric-guide", guide)),
                    Sense::drag(),
                )
                .on_hover_cursor(cursor);
            interacting |= response.hovered() || response.dragged();
            let Some(pointer) = response
                .interact_pointer_pos()
                .filter(|_| response.dragged())
            else {
                continue;
            };
            let world = self.to_world(origin, pointer);
            match guide {
                FontMetricGuide::Origin => {
                    self.font_project.metrics.origin_x =
                        world.x.min(self.font_project.metrics.right_x - 40.0);
                }
                FontMetricGuide::Right => {
                    self.font_project.metrics.right_x =
                        world.x.max(self.font_project.metrics.origin_x + 40.0);
                }
                FontMetricGuide::Ascender => {
                    self.font_project.metrics.ascender_y =
                        world.y.min(self.font_project.metrics.x_height_y - 20.0);
                }
                FontMetricGuide::XHeight => {
                    self.font_project.metrics.x_height_y = world.y.clamp(
                        self.font_project.metrics.ascender_y + 20.0,
                        self.font_project.metrics.baseline_y - 20.0,
                    );
                }
                FontMetricGuide::Baseline => {
                    self.font_project.metrics.baseline_y = world.y.clamp(
                        self.font_project.metrics.x_height_y + 20.0,
                        self.font_project.metrics.descender_y - 20.0,
                    );
                }
                FontMetricGuide::Descender => {
                    self.font_project.metrics.descender_y =
                        world.y.max(self.font_project.metrics.baseline_y + 20.0);
                }
            }
            self.status = "メトリクスガイドを移動しました".to_owned();
        }
        interacting
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
        self.canvas_center = Point::new(
            self.canvas_size.x as f64 / 2.0,
            self.canvas_size.y as f64 / 2.0,
        );

        self.rebuild_pegs(rect.size());
        let mut rendered = self.materialized_faces();
        let hover = response.hover_pos().map(|p| self.to_world(origin, p));
        let guide_interaction = self.interact_font_metric_guides(ui, origin);

        if !guide_interaction && is_space_drag && response.dragged() {
            did_view_change = self.pan_view(response.drag_delta()) || did_view_change;
        }
        let can_zoom = is_command || (zoom_delta - 1.0).abs() > f32::EPSILON;
        if can_zoom {
            if let Some(cursor) = response.hover_pos() {
                did_view_change =
                    self.set_zoom(origin, cursor, self.view.zoom * zoom_delta) || did_view_change;
            }
        }
        // egui のスクロール値はコンテキスト全体から届くため、ポインタが
        // グリフ等のパネル上にある時はキャンバスへ伝播させない。
        if response.hovered() && !can_zoom && smooth_scroll != Vec2::ZERO {
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
                    self.grab = Some(GrabState {
                        base: self.doc.clone(),
                        origins,
                        anchor,
                    });
                    self.status = "移動中… クリックで確定 / Escで取消".to_owned();
                } else {
                    self.status = "移動する任意円を選んでください（G）".to_owned();
                }
            }
        }
        let grabbing = self.grab.is_some();
        if grabbing {
            let cancel = ui.input(|i| i.key_pressed(Key::Escape)) || response.secondary_clicked();
            let confirm = response.clicked() || ui.input(|i| i.key_pressed(Key::Enter));
            if let (Some(grab), Some(pos)) = (self.grab.as_ref(), hover) {
                let delta = pos - grab.anchor;
                let moves: Vec<(u64, Point, f64)> = grab
                    .origins
                    .iter()
                    .filter_map(|(id, origin)| {
                        self.doc
                            .custom_pegs
                            .iter()
                            .find(|peg| peg.id == *id)
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
        let mut request_cancel_draw = false;
        let mut request_finish_face: Option<(Face, Vec<Seg>)> = None;
        let mut active_status: Option<String> = None;
        let mut rebuild_pegs = false;

        let peg_refs = self.peg_refs.clone();
        let resolved_pegs = self.pegs.clone();
        let grid = self.doc.grid;
        let customs = self.doc.custom_pegs.clone();
        let canvas_center = self.canvas_center;
        let appearance = self.doc.peg_appearance;
        let mut live_snapshot: Option<(
            StartAnchor,
            Point,
            bool,
            Thread,
            Vec<Element>,
            Vec<usize>,
        )> = None;
        let mut pin_request = false;

        let resolver = move |peg_ref: &PegRef| -> Option<Peg> {
            resolve_peg(&grid, &customs, canvas_center, appearance, peg_ref)
        };

        if !grabbing && response.drag_started() {
            if let Some(pos) = hover {
                if self.tool == Tool::Select && self.peg_at_cursor(pos).is_none() && !is_space_drag
                {
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
                    let radius =
                        self.peg_refs
                            .iter()
                            .zip(self.pegs.iter())
                            .find_map(|(pr, peg)| {
                                (peg_ref == *pr)
                                    .then_some((peg.radius - self.doc.grid.catch).max(0.0))
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
                                // 使用中／未使用にかかわらず、グリッド円をその場で
                                // 任意円へ置き換えてドラッグ可能にする。使用中なら
                                // すべての面参照も同じ任意円へ付け替わる。
                                if self.drag_snapshot.is_none() {
                                    self.drag_snapshot = Some(self.doc.clone());
                                }
                                self.set_single_selection(Some(Selection::PegRef(source)));
                                let id = self.direct_select_drag_target(source, pos_ref, radius);
                                self.set_single_selection(Some(Selection::Peg(id)));
                                self.begin_drag(&[id], pos, true);
                                self.rebuild_pegs(self.canvas_size);
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
                        if let Some(peg) = self.doc.custom_pegs.iter_mut().find(|peg| peg.id == *id)
                        {
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
                        self.commit_moved_peg_captures(&moving_ids);
                        rebuild_pegs = true;
                    }
                }
            }
        }

        if response.drag_stopped() {
            if let Some(drag) = self.active_custom_drag.take() {
                let changed = match drag {
                    ActiveDrag::Custom {
                        start, converted, ..
                    } => {
                        converted
                            || start.iter().any(|(id, start_center)| {
                                self.doc
                                    .custom_pegs
                                    .iter()
                                    .any(|peg| peg.id == *id && peg.center != *start_center)
                            })
                    }
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
        let undo_guard_release_distance = self.world_unit(6.0);
        if let Some(active) = self.draw.as_mut() {
            if let (Some(cursor), Some(guard)) = (hover, active.undo_guard) {
                if should_release_undo_guard(
                    cursor,
                    active.undo_guard_origin,
                    resolved_pegs.get(guard),
                    undo_guard_release_distance,
                ) {
                    active.undo_guard = None;
                    active.undo_guard_origin = None;
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
                    update_thread_ignoring(
                        &mut active.th,
                        cursor,
                        &resolved_pegs,
                        &capture_ignored,
                    );
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
                                undo_guard_origin: None,
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
            let live_preview = thread_to_wrap_elements(&th, &resolved_pegs, &peg_refs, th.tip);
            let live_closed = thread_to_wrap_elements(&th, &resolved_pegs, &peg_refs, start_point);
            if let (Some(lp), Some(lc)) = (live_preview, live_closed) {
                let mut preview_elements = base_elements.clone();
                preview_elements.extend(lp);
                // プレビューは開いた経路（始点→…→先端）。閉じ線は描画しない
                active_preview =
                    materialize_chain(start_point, &preview_elements, &resolver, th.tip);

                // 巻き方向の全候補探索は閉じる地点の近くでだけ必要。
                // 再訪・多重巻き中の通常ドラッグではライブ経路だけを更新し、
                // 毎フレームの指数的な再計算を避ける。
                let in_close_radius = th.tip.distance(start_point) <= self.world_unit(CLOSE_RADIUS);
                if in_close_radius {
                    let mut elements = base_elements.clone();
                    elements.extend(lc);
                    let preferred_start_s = if matches!(elements.last(), Some(Element::Wrap { peg, .. }) if *peg == start.peg)
                    {
                        None
                    } else {
                        let head = th.head(&resolved_pegs);
                        resolver(&start.peg).map(|start_peg| {
                            let d = start_point - head;
                            let to_center = start_peg.center - head;
                            if d.cross(to_center) >= 0.0 {
                                1.0
                            } else {
                                -1.0
                            }
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
                    // ライブ描画時の局所的な巻き方向が、閉路全体では成立しない
                    // 三角形などがある。Wrapが8個以下なら全符号組合せを調べ、
                    // 複数箇所の反転が必要な閉路も候補に含める。
                    if let Some(seed) = variants.first() {
                        variants = winding_variants(seed);
                    }
                    let distinct_wraps = variants.first().map_or(0, |candidate| {
                        let mut refs = Vec::new();
                        for element in candidate {
                            if let Element::Wrap { peg, .. } = element {
                                if !refs.contains(peg) {
                                    refs.push(*peg);
                                }
                            }
                        }
                        refs.len()
                    });
                    if distinct_wraps == 1 {
                        if let Some(single_circle) =
                            intentional_single_circle_wrap(&th, start, &peg_refs)
                        {
                            variants = vec![vec![single_circle]];
                        }
                    }

                    let preferred_elements = variants.first().cloned().unwrap_or_default();
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
                    let mut chosen: Option<(Face, Vec<Seg>, usize, f64)> = None;
                    for candidate_elements in variants {
                        let winding_changes =
                            winding_change_count(&preferred_elements, &candidate_elements);
                        // 候補は反転数の少ない順。すでに有効解が見つかっていて、
                        // それより多くの巻き方向変更が必要なら意図忠実度で必ず負ける。
                        // 同じ変更数だけは円弧長を比較してから打ち切る。
                        if chosen
                            .as_ref()
                            .is_some_and(|(_, _, best_changes, _)| winding_changes > *best_changes)
                        {
                            break;
                        }
                        let face = Face {
                            start,
                            elements: candidate_elements,
                            visible: true,
                            name: String::new(),
                            baked: None,
                            ignored_pegs: ignored_refs.clone(),
                        };
                        let Some(segs) =
                            materialize_face_with_obstacles(&face, &resolver, &obstacles)
                        else {
                            continue;
                        };
                        if first_candidate.is_none() {
                            first_candidate = Some((face.clone(), segs.clone()));
                        }
                        if !segments_penetrate_pegs(&segs, &obstacle_pegs) {
                            let total_arc = segs
                                .iter()
                                .map(|segment| match *segment {
                                    Seg::Arc { start, end, .. } => (end - start).abs(),
                                    Seg::Line { .. } => 0.0,
                                })
                                .sum::<f64>();
                            if closure_score_is_better(
                                winding_changes,
                                total_arc,
                                chosen.as_ref().map(|(_, _, best_changes, best_arc)| {
                                    (*best_changes, *best_arc)
                                }),
                            ) {
                                chosen = Some((face, segs, winding_changes, total_arc));
                            }
                        }
                    }
                    let chosen = chosen.map(|(face, segs, _, _)| (face, segs));
                    if let Some((face, segs)) = chosen.or(first_candidate) {
                        let over_wrap = th.wraps.iter().any(|wrap| {
                            ((wrap.exit - wrap.entry) * wrap.s).abs()
                                > std::f64::consts::TAU * MAX_MANUAL_TURNS as f64
                        });
                        active_has_penetration =
                            over_wrap || segments_penetrate_pegs(&segs, &obstacle_pegs);
                        active_close_allowed = armed
                            && !segs.is_empty()
                            && face_has_minimum_constraints(&face)
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
            }

            if response.clicked() {
                if is_command {
                    pin_request = true;
                } else if active_close_allowed {
                    request_finish = true;
                } else if request_finish_face
                    .as_ref()
                    .is_some_and(|(face, _)| !face_has_minimum_constraints(face))
                {
                    request_cancel_draw = true;
                } else if th.wraps.is_empty() && base_is_empty {
                    active_status = Some("ペグに引っかけてから始点に戻ると閉じられます".to_owned());
                } else if active_has_penetration {
                    active_status =
                        Some("糸が円を貫通しているか、巻き数が上限を超えています".to_owned());
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

        if request_cancel_draw {
            self.draw = None;
            self.status = "描画をキャンセルしました".to_owned();
        }

        if pin_request {
            request_pin = hover.map(|pos| clamp_outside_pegs(pos, &resolved_pegs));
        }
        if request_finish {
            if let Some(active) = self.draw.take() {
                if let Some((face, segs)) = request_finish_face {
                    if !face_has_minimum_constraints(&face) {
                        self.status = "異なる円を2個以上経由してから閉じてください".to_owned();
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
                            let self_intersects = self.has_self_intersections(&pts);
                            if area.abs() < 1e-9
                                && !(self_intersects && path_has_two_dimensional_extent(&pts))
                            {
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
                for idx in
                    pegs_penetrated_by_line(head, pos, &resolved_pegs, exclude, &active.ignored)
                {
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

        // 制作面はパネルや入力欄とは別の専用フラット色を使う。
        // グラデーションは入れず、モック同様に中央だけ一段深く見せる。
        painter.rect_filled(rect, 0.0, colors.canvas);

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
        // 参照フォントは作図面より下へ敷き、現在の輪郭との差を見ながらなぞれるようにする。
        self.draw_reference_font(&painter, origin);
        if let Some(mesh) = build_faces_mesh(&screen_polys, Pos2::new(0.0, 0.0), colors.face_fill) {
            painter.add(mesh);
        }
        self.draw_onion_skin(&painter, origin);

        // 引っかかり範囲は糸より先に描く。同じ実効半径上の暗いガイドが
        // 円弧だけを後から上書きし、直線より細く見えていたため。
        if self.show_circles && self.doc.grid.catch > 0.5 {
            let zoom = self.view.zoom;
            for (peg, peg_ref) in self.pegs.iter().zip(self.peg_refs.iter()) {
                if let PegRef::Custom { id } = peg_ref {
                    if self
                        .doc
                        .custom_pegs
                        .iter()
                        .find(|custom| custom.id == *id)
                        .is_some_and(|custom| !custom.visible)
                    {
                        continue;
                    }
                }
                let c = self.to_screen(origin, peg.center);
                if peg.shape == crate::model::PegShape::RoundedSquare {
                    let catch_rect =
                        Rect::from_center_size(c, Vec2::splat(peg.radius as f32 * zoom * 2.0));
                    painter.rect_stroke(
                        catch_rect,
                        (peg.rounding as f32 * zoom).min(peg.radius as f32 * zoom),
                        Stroke::new(0.6, colors.catch),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_stroke(
                        c,
                        peg.radius as f32 * zoom,
                        Stroke::new(0.6, colors.catch),
                    );
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
                if line.len() > 2
                    && line
                        .first()
                        .zip(line.last())
                        .is_some_and(|(a, b)| a.distance(*b) < 0.01)
                {
                    line.pop();
                }
                painter.add(egui::Shape::closed_line(
                    line,
                    Stroke::new(0.78, colors.thread),
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
                if line.len() > 2
                    && line
                        .first()
                        .zip(line.last())
                        .is_some_and(|(a, b)| a.distance(*b) < 0.01)
                {
                    line.pop();
                }
                painter.add(egui::Shape::closed_line(
                    line,
                    Stroke::new(1.8, colors.select),
                ));
            }
        }
        // 多重巻きは輪郭全体を太くせず、巻いた円弧の外側だけへ
        // わずかな隙間を空けた同心線を追加して見せる。
        let accent_gap_world = 2.8 / f64::from(self.view.zoom.max(0.01));
        for (list_idx, &(_, ref render)) in rendered.iter().enumerate() {
            let selected = selected_face_indices.contains(&list_idx);
            let color = if selected {
                colors.select
            } else {
                colors.thread
            };
            let width = if selected { 1.35 } else { 0.78 };
            for accent in &render.wrap_accents {
                for layer in 1..=accent.turns.min(3) {
                    let radius = accent.radius + accent_gap_world * f64::from(layer);
                    let steps = 96usize;
                    let points: Vec<Pos2> = (0..=steps)
                        .map(|index| {
                            let angle = accent.start
                                + accent.direction * std::f64::consts::TAU * index as f64
                                    / steps as f64;
                            self.to_screen(
                                origin,
                                crate::geometry::on_circle(accent.center, radius, angle),
                            )
                        })
                        .collect();
                    painter.add(egui::Shape::line(points, Stroke::new(width, color)));
                }
            }
        }
        for id in self.selected_pegs() {
            if let Some(p) = self.doc.custom_pegs.iter().find(|peg| peg.id == id) {
                let c = self.to_screen(origin, p.center);
                let r = p.radius as f32 * self.view.zoom + 2.0;
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    painter.rect_stroke(
                        Rect::from_center_size(c, Vec2::splat(r * 2.0)),
                        (self.doc.peg_appearance.rounding as f32 * self.view.zoom).min(r),
                        Stroke::new(2.0, colors.select),
                        egui::StrokeKind::Inside,
                    );
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
                    if self
                        .doc
                        .custom_pegs
                        .iter()
                        .find(|custom| custom.id == *id)
                        .is_some_and(|custom| !custom.visible)
                    {
                        continue;
                    }
                }
                let c = self.to_screen(origin, peg.center);
                let r_draw = ((peg.radius - self.doc.grid.catch).max(0.0) as f32) * zoom;
                // 任意円がグリッド/サブ円に被っているときは赤で警告表示
                let overlaps =
                    matches!(peg_ref, PegRef::Custom { .. }) && self.custom_peg_overlaps_grid(peg);
                let stroke_color = if overlaps {
                    colors.close_ng
                } else {
                    colors.peg_stroke
                };
                if self.doc.peg_appearance.shape == crate::model::PegShape::RoundedSquare {
                    let peg_rect = Rect::from_center_size(c, Vec2::splat(r_draw * 2.0));
                    let rounding = (self.doc.peg_appearance.rounding as f32 * zoom).min(r_draw);
                    painter.rect_filled(peg_rect, rounding, colors.peg_fill);
                    painter.rect_stroke(
                        peg_rect,
                        rounding,
                        Stroke::new(1.2, stroke_color),
                        egui::StrokeKind::Inside,
                    );
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
                    painter.add(egui::Shape::line(line, Stroke::new(1.55, color)));
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
                for wrap in &active.th.wraps {
                    let turns = ((((wrap.exit - wrap.entry) * wrap.s).max(0.0)
                        / std::f64::consts::TAU)
                        .floor() as u32)
                        .min(3);
                    let Some(peg) = resolved_pegs.get(wrap.peg) else {
                        continue;
                    };
                    let center = self.to_screen(origin, peg.center);
                    for layer in 1..=turns {
                        painter.circle_stroke(
                            center,
                            peg.radius as f32 * self.view.zoom + layer as f32 * 2.8,
                            Stroke::new(0.78, color),
                        );
                    }
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
                    painter.rect_stroke(
                        Rect::from_center_size(preview_center, Vec2::splat(preview_radius * 2.0)),
                        (self.doc.peg_appearance.rounding as f32 * self.view.zoom)
                            .min(preview_radius),
                        Stroke::new(1.5, color),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_stroke(preview_center, preview_radius, Stroke::new(1.5, color));
                }
                painter.circle_filled(self.to_screen(origin, p), 2.0, color);
            }
        }

        // ガイドとラベルはシェイプや円より前面に描き、文字が縁の下へ
        // 隠れないようにする。
        self.draw_font_metric_guides(&painter, origin);

        let minimap_alpha = response.ctx.animate_value_with_time(
            Id::new("minimap-alpha"),
            if self.view_overlay_timer > 0.0 {
                1.0
            } else {
                0.0
            },
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
                (
                    face_min.x - pad,
                    face_min.y - pad,
                    face_max.x + pad,
                    face_max.y + pad,
                )
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
                Color32::from_rgba_unmultiplied(
                    colors.bg.r(),
                    colors.bg.g(),
                    colors.bg.b(),
                    bg_alpha,
                ),
            );
            painter.rect_stroke(
                map_rect,
                6.0,
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(
                        colors.peg_stroke.r(),
                        colors.peg_stroke.g(),
                        colors.peg_stroke.b(),
                        stroke_alpha,
                    ),
                ),
                egui::StrokeKind::Inside,
            );
            if let Some(shape) = build_faces_mesh(
                &map_polys,
                Pos2::new(0.0, 0.0),
                Color32::from_rgba_unmultiplied(160, 160, 160, face_alpha),
            ) {
                if let egui::Shape::Mesh(mesh) = shape {
                    painter.add(egui::Shape::mesh(mesh));
                }
            }

            let view_min = self.to_world(origin, rect.min);
            let view_max = self.to_world(origin, rect.max);
            let v0 = to_map(view_min);
            let v1 = to_map(view_max);
            painter.add(egui::Shape::line(
                vec![v0, Pos2::new(v1.x, v0.y), v1, Pos2::new(v0.x, v1.y), v0],
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(
                        180,
                        220,
                        255,
                        (minimap_alpha * 220.0).round() as u8,
                    ),
                ),
            ));
        }

        // 通常時はモックどおり下端を空ける。ズーム操作直後、または左下の
        // ホットゾーンへポインタを置いたときだけ詳細バーを出す。
        let zoom_hot_zone = response.hover_pos().is_some_and(|pointer| {
            pointer.x <= rect.left() + 270.0 && pointer.y >= rect.bottom() - 52.0
        });
        if self.view_overlay_timer > 0.0 || zoom_hot_zone {
            egui::Area::new(egui::Id::new("zoom-bar"))
                .fixed_pos(Pos2::new(rect.left() + 8.0, rect.bottom() - 34.0))
                .show(&response.ctx, |ui| {
                    egui::Frame::popup(&ui.ctx().style().clone()).show(ui, |ui| {
                        ui.spacing_mut().slider_width = 120.0;
                        ui.horizontal(|ui| {
                            let mut changed = false;
                            if ui.add(egui::Button::new("−").small()).clicked() {
                                changed |=
                                    self.set_zoom(origin, rect.center(), self.view.zoom * 0.9);
                            }
                            let mut pct = f64::from(self.view.zoom) * 100.0;
                            let resp = ui
                                .allocate_ui_with_layout(
                                    Vec2::new(92.0, 22.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| mock_slider_f64(ui, &mut pct, 20.0..=400.0, 0, "%"),
                                )
                                .inner;
                            if resp.changed() {
                                changed |=
                                    self.set_zoom(origin, rect.center(), (pct / 100.0) as f32);
                            }
                            if ui.add(egui::Button::new("＋").small()).clicked() {
                                changed |=
                                    self.set_zoom(origin, rect.center(), self.view.zoom * 1.1);
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_save_operation(ctx);
        if !ENABLE_EXPERIMENTAL_SQUARE_PEGS {
            self.doc.peg_appearance.shape = crate::model::PegShape::Circle;
        }
        if self.enforce_safe_grid_radii() {
            self.status = "円同士が重ならない安全な半径へ自動調整しました".to_owned();
        }
        let redo = ctx.input_mut(|i| i.consume_shortcut(&SC_REDO) || i.consume_shortcut(&SC_REDO2));
        let undo = ctx.input_mut(|i| i.consume_shortcut(&SC_UNDO));
        let save = ctx.input_mut(|i| i.consume_shortcut(&SC_SAVE));
        if save {
            self.save_current_workspace();
        } else if redo {
            self.do_redo();
        } else if undo {
            if self.draw.is_some() {
                // 描画中は「直前の引っ掛かりの解除」として働く
                self.undo_last_catch();
            } else {
                self.do_undo();
            }
        }
        if self.workspace_mode == WorkspaceMode::Font
            && self.draw.is_none()
            && !ctx.wants_keyboard_input()
        {
            let previous =
                ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowLeft));
            let next = ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowRight));
            if previous && self.font_project.active_glyph > 0 {
                self.switch_glyph(self.font_project.active_glyph - 1);
            } else if next && self.font_project.active_glyph + 1 < self.font_project.glyphs.len() {
                self.switch_glyph(self.font_project.active_glyph + 1);
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
            .exact_height(40.0)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .stroke(Stroke::new(
                        1.0,
                        ctx.style().visuals.widgets.inactive.bg_stroke.color,
                    ))
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(24.0)
            .show(ctx, |ui| self.status_bar(ui));
        // 編集ツールは中央リーフでキャンバスが前面のときだけ出す。
        // 試し組みの入力や表示へ被せない。
        let canvas_is_active = self
            .dock
            .find_tab(&DockTab::Canvas)
            .and_then(|(surface, node, _)| match &self.dock[surface][node] {
                egui_dock::Node::Leaf(leaf) => leaf.tabs.get(leaf.active.0),
                _ => None,
            })
            .is_some_and(|tab| *tab == DockTab::Canvas);
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
        dock_style.tab_bar.height = 42.0;
        dock_style.tab_bar.inner_margin = egui::Margin::symmetric(6, 8);
        dock_style.tab_bar.corner_radius = egui::CornerRadius::same(0);
        dock_style.tab_bar.show_scroll_bar_on_overflow = true;
        dock_style.tab.spacing = 2.0;
        dock_style.tab.active.bg_fill = colors.bg2;
        dock_style.tab.active.text_color = colors.text1;
        dock_style.tab.active.corner_radius = egui::CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        };
        dock_style.tab.active.outline_color = Color32::TRANSPARENT;
        dock_style.tab.focused = dock_style.tab.active.clone();
        dock_style.tab.inactive.bg_fill = Color32::TRANSPARENT;
        dock_style.tab.inactive.text_color = colors.text2;
        dock_style.tab.inactive.corner_radius = egui::CornerRadius::ZERO;
        dock_style.tab.inactive.outline_color = Color32::TRANSPARENT;
        dock_style.tab.hovered.bg_fill = colors.bg2;
        dock_style.tab.hovered.text_color = colors.text1;
        dock_style.tab.hovered.corner_radius = egui::CornerRadius {
            nw: 5,
            ne: 5,
            sw: 0,
            se: 0,
        };
        dock_style.tab.hovered.outline_color = Color32::TRANSPARENT;
        dock_style.tab.tab_body.bg_fill = colors.panel;
        dock_style.tab.tab_body.stroke = Stroke::NONE;
        dock_style.tab.tab_body.inner_margin = egui::Margin::same(6);
        DockArea::new(&mut dock)
            .style(dock_style)
            // 常時表示の×ボタンは煩いので消す（パネルの開閉は上部「パネル」メニュー）
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show_leaf_collapse_buttons(false)
            .show(ctx, &mut AppTabViewer { app: self });
        self.dock = dock;
        if let Some(tab) = self.pending_panel_tab.take() {
            self.activate_panel_tab(tab);
        }
        // Illustrator と同様、ツールはドックを圧迫せずキャンバス上で移動できる。
        // Area は最後に描画して、ドック内容より常に手前へ出す。
        if canvas_is_active {
            self.tool_palette(ctx);
        }
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
        let language = self.app.language;
        match tab {
            DockTab::Project => match self.app.workspace_mode {
                WorkspaceMode::Shape => language
                    .text("シェイプドキュメント", "Shape document")
                    .into(),
                WorkspaceMode::Font => language.text("フォントプロジェクト", "Font project").into(),
            },
            DockTab::Canvas => language.text("キャンバス", "Canvas").into(),
            DockTab::Proof => language.text("試し組み", "Proof").into(),
            DockTab::Glyphs => language.text("フォント", "Font").into(),
            DockTab::Layers => language.text("レイヤー", "Layers").into(),
            DockTab::Circles => language.text("円", "Pegs").into(),
            DockTab::Properties => language.text("プロパティ", "Properties").into(),
            DockTab::Grid => language.text("グリッド", "Grid").into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        if matches!(*tab, DockTab::Canvas | DockTab::Proof) {
            self.app.active_center_tab = *tab;
        }
        self.app.render_tab_content(ui, *tab);
    }

    fn is_closeable(&self, tab: &Self::Tab) -> bool {
        !matches!(*tab, DockTab::Project | DockTab::Canvas | DockTab::Proof)
    }

    fn allowed_in_windows(&self, tab: &mut Self::Tab) -> bool {
        !matches!(*tab, DockTab::Project | DockTab::Canvas | DockTab::Proof)
    }
}

/// ツールアイコンボタン（Illustrator風、描画ベース）
fn tool_icon_button(ui: &mut egui::Ui, active: bool, tool: Tool, tooltip: &str) -> egui::Response {
    let size = Vec2::splat(30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled(),
            active,
            tooltip.lines().next().unwrap_or(tooltip),
        )
    });
    let visuals = ui.style().interact_selectable(&resp, active);
    if active {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 6.0, ui.visuals().widgets.active.bg_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 6.0, visuals.bg_fill);
    }
    let color = if active {
        ui.visuals().widgets.active.fg_stroke.color
    } else {
        visuals.fg_stroke.color
    };
    // モックの data-lucide と同じ配布フォントを使う。手描きの近似ではなく、
    // 元モックと同じ4種類のベクターグリフを描画する。
    let icon = match tool {
        Tool::Select => LucideIcon::MousePointer2,
        Tool::DirectSelect => LucideIcon::MousePointer,
        Tool::Draw => LucideIcon::PenTool,
        Tool::CustomPeg => LucideIcon::CircleDotDashed,
    };
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        icon.unicode(),
        lucide_font_id(16.0),
        color,
    );
    resp.on_hover_text(tooltip)
}

#[derive(Clone, Copy)]
enum PanelGlyph {
    Shape,
    Type,
    Layers,
    Peg,
    Grid,
    Folder,
    EmptyLayer,
}

/// パネル用の16pxベクターアイコン。文字記号に依存せず、すべて同じ
/// 24pxグリッド・1.35pxストロークで描く。
fn panel_glyph(ui: &mut egui::Ui, glyph: PanelGlyph) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
    let color = ui
        .visuals()
        .widgets
        .inactive
        .fg_stroke
        .color
        .gamma_multiply(0.82);
    let icon = match glyph {
        PanelGlyph::Shape => LucideIcon::Shapes,
        PanelGlyph::Type => LucideIcon::Type,
        PanelGlyph::Layers => LucideIcon::Layers2,
        PanelGlyph::Peg => LucideIcon::CircleDot,
        PanelGlyph::Grid => LucideIcon::Grid3x3,
        PanelGlyph::Folder => LucideIcon::Folder,
        PanelGlyph::EmptyLayer => LucideIcon::Square,
    };
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        icon.unicode(),
        lucide_font_id(14.0),
        color,
    );
    response
}

/// 左プロジェクト領域のLinear風ナビ行。行全体をクリック対象にし、
/// アイコン・ラベル・件数の軸をすべて27px行へ揃える。
fn project_nav_row(
    ui: &mut egui::Ui,
    glyph: PanelGlyph,
    label: &str,
    count: Option<&str>,
    selected: bool,
) -> egui::Response {
    let fill = if selected {
        ui.visuals().selection.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    let inner = egui::Frame::new()
        .fill(fill)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.set_height(25.0);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                panel_glyph(ui, glyph);
                ui.label(egui::RichText::new(label).color(if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().text_color()
                }));
                if let Some(count) = count {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(count)
                                .size(10.0)
                                .color(ui.visuals().weak_text_color()),
                        );
                    });
                }
            });
        });
    let response = inner.response.interact(Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    response
}

fn project_section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [ui.available_width(), 24.0],
        egui::Label::new(
            egui::RichText::new(text)
                .size(10.0)
                .color(ui.visuals().weak_text_color()),
        ),
    );
}

#[derive(Clone, Copy)]
enum LayerActionGlyph {
    Plus,
    Folder,
    Trash,
}

/// レイヤーパネルの追加操作。モック同様、24pxのアイコンボタンに揃え、
/// 説明はツールチップとアクセシビリティ名へ持たせる。
fn layer_action_button(ui: &mut egui::Ui, glyph: LayerActionGlyph, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(26.0, 24.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, visuals.weak_bg_fill);
    }

    let color = visuals.fg_stroke.color;
    let icon = match glyph {
        LayerActionGlyph::Plus => LucideIcon::Plus,
        LayerActionGlyph::Folder => LucideIcon::FolderPlus,
        LayerActionGlyph::Trash => LucideIcon::Trash2,
    };
    ui.painter().text(
        rect.center() + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        icon.unicode(),
        lucide_font_id(14.0),
        color,
    );
    response
}

/// レイヤー名と種類を二段で描く。狭いパネルでも種類が消えず、
/// フォルダ／結合済み／通常／空レイヤーの違いを走査できる。
fn layer_item_label(
    ui: &mut egui::Ui,
    width: f32,
    name: &str,
    kind: &str,
    selected: bool,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width.max(8.0), 32.0), Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(rect);
    let name_color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    painter.text(
        Pos2::new(rect.left(), rect.top() + 3.0),
        egui::Align2::LEFT_TOP,
        name,
        FontId::proportional(11.0),
        name_color,
    );
    painter.text(
        Pos2::new(rect.left(), rect.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        kind,
        FontId::proportional(9.0),
        ui.visuals().weak_text_color(),
    );
    response
}

/// 行ラベルをD&Dソースにし、ドラッグ中は移動対象をカーソル脇に表示する。
fn layer_drag_source(
    ui: &mut egui::Ui,
    response: &egui::Response,
    payload: LayerDragPayload,
    label: &str,
) {
    response.dnd_set_drag_payload(payload);
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui.painter().rect_stroke(
            response.rect,
            5.0,
            Stroke::new(1.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Inside,
        );
        if let Some(pointer) = ui.ctx().pointer_interact_pos() {
            egui::Area::new(Id::new(("layer-drag-preview", payload)))
                .order(egui::Order::Tooltip)
                .fixed_pos(pointer + Vec2::new(12.0, 10.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().panel_fill)
                        .stroke(Stroke::new(1.0, ui.visuals().selection.stroke.color))
                        .corner_radius(5)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.label(label);
                        });
                });
        }
    }
}

/// Narrow root-level insertion target between layer rows. It stays visually
/// quiet until a layer is dragged over it, then becomes the familiar insertion
/// line used by desktop layer panels.
fn layer_root_drop_zone(ui: &mut egui::Ui) -> Option<LayerDragPayload> {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let active = response.dnd_hover_payload::<LayerDragPayload>().is_some();
    let stroke = if active {
        Stroke::new(2.0, ui.visuals().selection.stroke.color)
    } else {
        Stroke::new(0.65, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    let inset = if active { 5.0 } else { 9.0 };
    ui.painter().line_segment(
        [
            Pos2::new(rect.left() + inset, rect.center().y),
            Pos2::new(rect.right() - inset, rect.center().y),
        ],
        stroke,
    );
    response
        .dnd_release_payload::<LayerDragPayload>()
        .map(|payload| *payload)
}

fn align_icon_button(ui: &mut egui::Ui, axis: AlignAxis, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(27.0, 25.0), Sense::click());
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let c = rect.center();
    let stroke = Stroke::new(1.25, color);
    let guide = Stroke::new(0.9, color.gamma_multiply(0.55));
    let horizontal = matches!(
        axis,
        AlignAxis::Left | AlignAxis::CenterX | AlignAxis::Right
    );
    if horizontal {
        let gx = match axis {
            AlignAxis::Left => c.x - 7.0,
            AlignAxis::Right => c.x + 7.0,
            _ => c.x,
        };
        ui.painter()
            .line_segment([Pos2::new(gx, c.y - 9.0), Pos2::new(gx, c.y + 9.0)], guide);
        for (i, width) in [8.0, 5.0, 10.0].iter().enumerate() {
            let y = c.y - 6.0 + i as f32 * 6.0;
            let (x1, x2) = match axis {
                AlignAxis::Left => (gx, gx + *width),
                AlignAxis::Right => (gx - *width, gx),
                _ => (gx - *width * 0.5, gx + *width * 0.5),
            };
            ui.painter()
                .line_segment([Pos2::new(x1, y), Pos2::new(x2, y)], stroke);
        }
    } else {
        let gy = match axis {
            AlignAxis::Top => c.y - 7.0,
            AlignAxis::Bottom => c.y + 7.0,
            _ => c.y,
        };
        ui.painter()
            .line_segment([Pos2::new(c.x - 9.0, gy), Pos2::new(c.x + 9.0, gy)], guide);
        for (i, width) in [8.0, 5.0, 10.0].iter().enumerate() {
            let x = c.x - 6.0 + i as f32 * 6.0;
            let (y1, y2) = match axis {
                AlignAxis::Top => (gy, gy + *width),
                AlignAxis::Bottom => (gy - *width, gy),
                _ => (gy - *width * 0.5, gy + *width * 0.5),
            };
            ui.painter()
                .line_segment([Pos2::new(x, y1), Pos2::new(x, y2)], stroke);
        }
    }
    response.on_hover_text(tooltip)
}

/// モックのselectと同じ26px高の言語メニュー。選択言語によって外形が変わらない。
fn language_menu_button(ui: &mut egui::Ui, language: UiLanguage, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(82.0, 26.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let colors = if ui.visuals().dark_mode {
        (Color32::from_rgb(21, 21, 24), Color32::from_rgb(43, 43, 48))
    } else {
        (
            Color32::from_rgb(247, 247, 247),
            Color32::from_rgb(0, 0, 0).gamma_multiply(0.12),
        )
    };
    let fill = if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        colors.0
    };
    ui.painter().rect(
        rect,
        6.0,
        fill,
        Stroke::new(1.0, colors.1),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        match language {
            UiLanguage::Japanese => "日本語",
            UiLanguage::English => "English",
        },
        FontId::proportional(11.0),
        ui.visuals().text_color(),
    );
    let c = Pos2::new(rect.right() - 10.0, rect.center().y + 0.5);
    ui.painter().add(egui::Shape::line(
        vec![
            Pos2::new(c.x - 3.2, c.y - 1.7),
            Pos2::new(c.x, c.y + 1.7),
            Pos2::new(c.x + 3.2, c.y - 1.7),
        ],
        Stroke::new(1.2, ui.visuals().weak_text_color()),
    ));
    response.on_hover_text(tooltip)
}

#[derive(Clone, Copy)]
enum HeaderMenuGlyph {
    File,
    Panels,
}

/// 上部バーの補助メニュー。文字ラベルを常設せず、意味の異なるベクターアイコンを
/// 一つずつ描くことで、保存と言語切替を主操作として残す。
fn header_menu_icon_button(
    ui: &mut egui::Ui,
    glyph: HeaderMenuGlyph,
    tooltip: &str,
) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    let color = visuals.fg_stroke.color.gamma_multiply(0.82);
    let stroke = Stroke::new(1.25, color);
    let center = rect.center();
    let painter = ui.painter();
    match glyph {
        HeaderMenuGlyph::File => {
            let page = Rect::from_center_size(center, Vec2::new(10.0, 13.0));
            painter.rect_stroke(page, 1.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(page.left() + 2.5, page.top() + 4.5),
                    Pos2::new(page.right() - 2.5, page.top() + 4.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(page.left() + 2.5, page.top() + 7.5),
                    Pos2::new(page.right() - 2.5, page.top() + 7.5),
                ],
                stroke,
            );
        }
        HeaderMenuGlyph::Panels => {
            let shell = Rect::from_center_size(center, Vec2::new(13.0, 11.0));
            painter.rect_stroke(shell, 1.5, stroke, egui::StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(shell.left() + 4.5, shell.top()),
                    Pos2::new(shell.left() + 4.5, shell.bottom()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(shell.right() - 3.8, shell.top()),
                    Pos2::new(shell.right() - 3.8, shell.bottom()),
                ],
                stroke,
            );
        }
    }
    response.on_hover_text(tooltip)
}

/// テーマ切替アイコン（月/太陽）
fn theme_icon_button(ui: &mut egui::Ui, dark: bool, tooltip: &str) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
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
    resp.on_hover_text(tooltip)
}

#[derive(Clone, Copy)]
enum IconKind {
    Eye,
}

/// Lucide eyeに近い14pxの細線アイコン。表示時も背景箱を付けない。
fn paint_eye_icon(painter: &egui::Painter, center: Pos2, color: Color32, visible: bool) {
    let icon = if visible {
        LucideIcon::Eye
    } else {
        LucideIcon::EyeOff
    };
    painter.text(
        center + Vec2::new(0.0, 0.5),
        egui::Align2::CENTER_CENTER,
        icon.unicode(),
        lucide_font_id(14.0),
        color,
    );
}

fn lucide_font_id(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("lucide".into()))
}

/// 表示切替はモックどおり背景なしの目だけで示す。
fn icon_toggle_button(
    ui: &mut egui::Ui,
    on: bool,
    kind: IconKind,
    tooltip: &str,
) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, tooltip)
    });
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, ui.style().interact(&resp).bg_fill);
    }
    let color = ui
        .visuals()
        .weak_text_color()
        .gamma_multiply(if on { 0.95 } else { 0.72 });
    match kind {
        IconKind::Eye => paint_eye_icon(ui.painter(), rect.center(), color, on),
    }
    resp.on_hover_text(tooltip)
}

/// インスペクタの文字列行。ラベル列を固定し、入力欄は残り幅を使う。
fn inspector_text_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    hint: &str,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [72.0, 24.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            ),
        );
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(value).hint_text(hint),
        )
    })
    .inner
}

/// ラベル付きの数値スクラブ入力（デザインモックの scrub-input 相当）。
/// 左に弱色のラベル、右端に同幅の数値欄を置き、縦に並べたときの軸を揃える。
/// value を破壊的に更新し、変更/確定を返す。
fn scrub_value(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_sized(
                [76.0, 24.0],
                egui::DragValue::new(value)
                    .speed(speed)
                    .range(range)
                    .fixed_decimals(1),
            )
        })
        .inner
    })
    .inner
}

/// モックの1pxレール＋9pxノブをそのまま描く数値スライダー。
/// 右の数値欄は24px高・5px角丸に固定し、ドラッグと直接入力の両方を維持する。
fn mock_slider_f64(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    decimals: usize,
    suffix: &str,
) -> egui::Response {
    let old_value = *value;
    let min = *range.start();
    let max = *range.end();
    let mut response = ui
        .horizontal(|ui| {
            let field_width = 50.0;
            let rail_width =
                (ui.available_width() - field_width - ui.spacing().item_spacing.x).max(28.0);
            let (rect, rail_response) =
                ui.allocate_exact_size(Vec2::new(rail_width, 22.0), Sense::click_and_drag());
            if ui.is_enabled() {
                if let Some(pointer) = rail_response.interact_pointer_pos() {
                    let usable = (rect.width() - 9.0).max(1.0);
                    let t = ((pointer.x - rect.left() - 4.5) / usable).clamp(0.0, 1.0);
                    *value = min + (max - min) * f64::from(t);
                }
            }
            rail_response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), *value, ""));
            let t = if max > min {
                ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            let center = Pos2::new(
                egui::lerp((rect.left() + 4.5)..=(rect.right() - 4.5), t),
                rect.center().y,
            );
            let rail_color = if ui.visuals().dark_mode {
                Color32::from_rgb(58, 58, 65)
            } else {
                Color32::from_rgb(190, 190, 196)
            };
            let knob_color = if ui.is_enabled() {
                if ui.visuals().dark_mode {
                    Color32::from_rgb(215, 215, 220)
                } else {
                    Color32::from_rgb(76, 76, 82)
                }
            } else {
                ui.visuals().weak_text_color()
            };
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left(), rect.center().y),
                    Pos2::new(rect.right(), rect.center().y),
                ],
                Stroke::new(1.0, rail_color),
            );
            ui.painter()
                .circle_filled(center, 6.5, ui.visuals().extreme_bg_color);
            ui.painter().circle_filled(center, 4.5, knob_color);

            let field_response = ui
                .scope(|ui| {
                    let dark = ui.visuals().dark_mode;
                    let field_fill = if dark {
                        Color32::from_rgb(21, 21, 24)
                    } else {
                        Color32::from_rgb(247, 247, 247)
                    };
                    let field_border = if dark {
                        Color32::from_rgb(43, 43, 48)
                    } else {
                        Color32::from_rgb(0, 0, 0).gamma_multiply(0.12)
                    };
                    let visuals = ui.visuals_mut();
                    for state in [
                        &mut visuals.widgets.inactive,
                        &mut visuals.widgets.hovered,
                        &mut visuals.widgets.active,
                    ] {
                        state.bg_fill = field_fill;
                        state.bg_stroke = Stroke::new(1.0, field_border);
                        state.corner_radius = 5.into();
                    }
                    ui.spacing_mut().interact_size.y = 24.0;
                    ui.add_sized(
                        [field_width, 24.0],
                        egui::DragValue::new(value)
                            .range(range)
                            .speed(((max - min).abs() / 240.0).max(0.01))
                            .fixed_decimals(decimals)
                            .suffix(suffix),
                    )
                })
                .inner;
            rail_response.union(field_response)
        })
        .inner;
    if *value != old_value {
        response.mark_changed();
    }
    response
}

fn mock_slider_row_f64(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    decimals: usize,
    suffix: &str,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [58.0, 24.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(ui.visuals().weak_text_color()),
            ),
        );
        mock_slider_f64(ui, value, range, decimals, suffix)
    })
    .inner
}

fn mock_slider_row_f32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f64>,
    decimals: usize,
) -> egui::Response {
    let mut converted = f64::from(*value);
    let response = mock_slider_row_f64(ui, label, &mut converted, range, decimals, "");
    if response.changed() {
        *value = converted as f32;
    }
    response
}

fn mock_percent_slider_row_f32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f64>,
) -> egui::Response {
    let mut percent = f64::from(*value) * 100.0;
    let percent_range = (*range.start() * 100.0)..=(*range.end() * 100.0);
    let response = mock_slider_row_f64(ui, label, &mut percent, percent_range, 0, "%");
    if response.changed() {
        *value = (percent / 100.0) as f32;
    }
    response
}

/// パネル内のセクション見出し（小さめ・弱色 + 下線）
fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(ui.visuals().text_color().gamma_multiply(0.68)),
    );
    let sep = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let rect = ui.max_rect();
    let y = ui.cursor().top() + 2.0;
    ui.painter().line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, sep),
    );
    ui.add_space(6.0);
}

/// リスト行の共通フレーム。全幅・36px 高で、選択時は accent 背景、
/// ホバー時は淡い背景を敷いてから中身を左右レイアウトで並べる
fn row_frame<R>(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let full_w = ui.available_width();
    let (bg_rect, _) = ui.allocate_exact_size(Vec2::new(full_w, 36.0), Sense::hover());
    let hovered = ui.rect_contains_pointer(bg_rect);
    if selected {
        let selected_fill = if ui.visuals().dark_mode {
            Color32::from_rgb(34, 34, 40)
        } else {
            Color32::from_rgb(234, 235, 250)
        };
        let accent = if ui.visuals().dark_mode {
            Color32::from_rgb(94, 106, 210)
        } else {
            Color32::from_rgb(82, 91, 194)
        };
        ui.painter().rect_filled(bg_rect, 5.0, selected_fill);
        ui.painter().rect_filled(
            Rect::from_min_max(
                bg_rect.left_top(),
                Pos2::new(bg_rect.left() + 2.0, bg_rect.bottom()),
            ),
            1.0,
            accent,
        );
    } else if hovered {
        ui.painter().rect_filled(
            bg_rect,
            5.0,
            ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.5),
        );
    }
    let inner = bg_rect.shrink2(Vec2::new(7.0, 1.0));
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
    let size = 18.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let visuals = ui.style().interact(&resp);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    let color = visuals.fg_stroke.color.gamma_multiply(0.78);
    paint_eye_icon(ui.painter(), rect.center(), color, visible);
    resp.on_hover_text(if visible {
        "非表示にする"
    } else {
        "表示する"
    })
}

/// フォルダ開閉用の背景なしシェブロン。既定の文字ボタンによる
/// サイズ差を避け、他の16pxアイコンと光学サイズを揃える。
fn disclosure_button(ui: &mut egui::Ui, expanded: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    let visuals = ui.style().interact(&response);
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4.0, visuals.bg_fill);
    }
    let center = rect.center();
    let color = visuals.fg_stroke.color.gamma_multiply(0.85);
    let stroke = Stroke::new(1.45, color);
    let points = if expanded {
        [
            Pos2::new(center.x - 4.0, center.y - 2.0),
            Pos2::new(center.x, center.y + 2.5),
            Pos2::new(center.x + 4.0, center.y - 2.0),
        ]
    } else {
        [
            Pos2::new(center.x - 2.0, center.y - 4.0),
            Pos2::new(center.x + 2.5, center.y),
            Pos2::new(center.x - 2.0, center.y + 4.0),
        ]
    };
    ui.painter().add(egui::Shape::line(points.to_vec(), stroke));
    response
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
        c.x >= min_x && c.x <= max_x && c.y >= min_y && c.y <= max_y && orient(a, b, c).abs() <= eps
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
            builder.line_to(lpoint(
                (origin.x as f64 + p.x) as f32,
                (origin.y as f64 + p.y) as f32,
            ));
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
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
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
    fonts.font_data.insert(
        "lucide".to_owned(),
        egui::FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name("lucide".into()),
        vec!["lucide".to_owned()],
    );
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
        };
        fonts.font_data.insert("cjk".to_owned(), data.into());
        order.push("cjk".to_owned());
    }
    if let Some(bytes) = load_first_valid_font(LATIN) {
        fonts
            .font_data
            .insert("latin".to_owned(), egui::FontData::from_owned(bytes).into());
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            elements: vec![Element::Wrap {
                peg: target,
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: 1.0,
            }],
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        });
        app.set_single_selection(Some(Selection::PegRef(target)));
        app.delete_selected();
        assert!(app.doc.disabled_pegs.contains(&target));
        assert!(app.doc.faces[0].elements.is_empty());
    }

    #[test]
    fn direct_select_replaces_the_grid_peg_before_dragging_it() {
        let mut app = test_app();
        app.canvas_size = Vec2::new(1280.0, 800.0);
        app.canvas_center = Point::new(640.0, 400.0);
        let source = PegRef::Grid { row: 0, col: 0 };
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: source,
                port: Port::Right,
            },
            elements: Vec::new(),
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        });
        let source_peg = resolve_peg(
            &app.doc.grid,
            &app.doc.custom_pegs,
            app.canvas_center,
            app.doc.peg_appearance,
            &source,
        )
        .unwrap();

        let id = app.direct_select_drag_target(
            source,
            source_peg.center,
            source_peg.radius - app.doc.grid.catch,
        );
        app.rebuild_pegs(app.canvas_size);

        assert!(app.doc.disabled_pegs.contains(&source));
        assert_eq!(app.doc.faces[0].start.peg, PegRef::Custom { id });
        assert!(app.doc.custom_pegs.iter().any(|peg| peg.id == id));
        assert!(app.can_place_custom_pegs(
            &[(
                id,
                Point::new(source_peg.center.x + 20.0, source_peg.center.y),
                source_peg.radius - app.doc.grid.catch,
            )],
            &[id],
        ));
    }

    #[test]
    fn merge_selected_unions_two_faces_into_baked_layer() {
        let mut app = test_app();
        app.canvas_size = Vec2::new(1280.0, 800.0);
        app.canvas_center = Point::new(640.0, 400.0);
        let mk = |c0: i32, c1: i32| Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: c0 },
                port: Port::Top,
            },
            elements: vec![
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: c0 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: 3.0,
                },
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: c1 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: 3.0,
                },
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
    fn align_selected_faces_moves_baked_layers_on_the_grid() {
        let mut app = test_app();
        app.doc.grid.spacing = 200.0;
        for offset in [0.0, 200.0] {
            app.doc.faces.push(Face {
                start: StartAnchor {
                    peg: PegRef::Grid { row: 0, col: 0 },
                    port: Port::Top,
                },
                elements: Vec::new(),
                name: String::new(),
                visible: true,
                baked: Some(crate::model::BakedShape {
                    rings: vec![vec![
                        Point::new(offset, 0.0),
                        Point::new(offset + 100.0, 0.0),
                        Point::new(offset + 100.0, 100.0),
                        Point::new(offset, 100.0),
                    ]],
                    bevel: 0.0,
                }),
                ignored_pegs: Vec::new(),
            });
        }
        app.set_single_selection(Some(Selection::Face(0)));
        app.toggle_selection(Selection::Face(1));

        app.align_selected_faces(AlignAxis::Left);

        let second = &app.doc.faces[1].baked.as_ref().unwrap().rings[0];
        let left = second
            .iter()
            .map(|point| point.x)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(left, 0.0);
        assert_eq!(app.undo.len(), 1);
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::TopRight,
            },
            elements: vec![
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: 1 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.2,
                    sweep_hint: 1.0,
                },
                Element::Wrap {
                    peg: PegRef::Sub { row: 0, col: 0 },
                    s: -1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: 2.0,
                },
                Element::Wrap {
                    peg: PegRef::Custom { id: 0 },
                    s: 1.0,
                    turns: 1,
                    exit_offset: -0.1,
                    sweep_hint: 0.1,
                },
            ],
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        });
        for i in 0..600 {
            app.doc.grid.spacing = 40.0 + (i as f64 * 11.7) % 280.0;
            app.doc.grid.radius = (i as f64 * 5.3) % 100.0;
            app.doc.grid.catch = (i as f64 * 1.7) % 24.0;
            app.doc.grid.sub_radius = (i as f64 * 3.1) % 60.0;
            app.doc.grid.scale_anchor = if i % 2 == 0 {
                ScaleAnchor::TopLeft
            } else {
                ScaleAnchor::Center
            };
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

    #[test]
    fn workspace_modes_preserve_shape_document_independently() {
        let mut app = test_app();
        app.doc.custom_pegs.push(CustomPeg {
            id: 7,
            center: Point::new(20.0, 30.0),
            radius: 12.0,
            visible: true,
        });
        let original_radius = app.doc.grid.radius;
        app.push_history();
        app.doc.grid.radius = 42.0;

        app.switch_workspace_mode(WorkspaceMode::Font);
        assert!(app.doc.custom_pegs.is_empty());

        app.switch_workspace_mode(WorkspaceMode::Shape);
        assert_eq!(app.doc.custom_pegs[0].id, 7);
        assert_eq!(app.doc.grid.radius, 42.0);
        app.do_undo();
        assert_eq!(app.doc.grid.radius, original_radius);
        assert_eq!(app.doc.custom_pegs[0].id, 7);
    }

    #[test]
    fn project_document_navigation_opens_and_activates_missing_panel() {
        let mut app = test_app();
        assert!(app.dock.find_tab(&DockTab::Circles).is_none());

        app.activate_panel_tab(DockTab::Circles);

        let (surface, node, _) = app.dock.find_tab(&DockTab::Circles).unwrap();
        let egui_dock::Node::Leaf(leaf) = &app.dock[surface][node] else {
            panic!("circle panel must open in a leaf");
        };
        assert_eq!(leaf.tabs.get(leaf.active.0), Some(&DockTab::Circles));
    }

    #[test]
    fn panel_command_detaches_layers_into_a_window() {
        let mut app = test_app();
        assert!(app.detach_panel_tab(
            DockTab::Layers,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0)),
        ));
        let (surface, _, _) = app.dock.find_tab(&DockTab::Layers).unwrap();
        assert!(!surface.is_main());
        assert!(!app.detach_panel_tab(
            DockTab::Layers,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0)),
        ));
    }

    #[test]
    fn workspace_round_trip_restores_detached_panel_layout() {
        let mut app = test_app();
        app.detach_panel_tab(
            DockTab::Layers,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0)),
        );

        let json = serde_json::to_string(&app.workspace_snapshot()).unwrap();
        let workspace: WorkspaceFile = serde_json::from_str(&json).unwrap();
        let dock = App::restore_workspace_dock(workspace.dock, workspace.mode);
        let (surface, _, _) = dock.find_tab(&DockTab::Layers).unwrap();
        assert!(!surface.is_main());
    }

    #[test]
    fn restored_font_layout_keeps_proof_and_uses_contextual_properties() {
        let dock = App::restore_workspace_dock(WorkspaceDockLayout::default(), WorkspaceMode::Font);
        assert!(dock.find_tab(&DockTab::Proof).is_some());
        assert!(dock.find_tab(&DockTab::Properties).is_some());
        assert!(dock.find_tab(&DockTab::Glyphs).is_none());
    }

    #[test]
    fn english_status_describes_actions_instead_of_falling_back_to_ready() {
        let mut app = test_app();
        app.language = UiLanguage::English;
        app.status = "レイヤーをマージしました".to_owned();
        assert_eq!(app.displayed_status(), "Layers merged");
        app.status = "グリフ A を編集中".to_owned();
        assert_eq!(app.displayed_status(), "Editing glyph A");
    }

    #[test]
    fn empty_layers_can_be_selected_and_grouped_into_a_folder() {
        let mut app = test_app();
        let first = app.add_empty_layer();
        let second = app.add_empty_layer();

        app.select_empty_layer(first, false);
        app.select_empty_layer(second, true);
        let folder_id = app.add_layer_folder();

        assert_eq!(app.doc.layer_folders.len(), 1);
        assert!(app.selected_empty_layers.is_empty());
        assert!(app.selected_layer_folders.contains(&folder_id));
        assert!(app
            .doc
            .empty_layers
            .iter()
            .all(|layer| layer.folder_id == Some(folder_id)));
    }

    #[test]
    fn keyboard_delete_handles_folders_and_empty_layers_with_undo() {
        let mut app = test_app();
        let layer_id = app.add_empty_layer();
        let folder_id = app.add_layer_folder();

        app.select_layer_folder(folder_id, false);
        app.delete_selected();
        assert!(app.doc.layer_folders.is_empty());
        assert_eq!(app.doc.empty_layers[0].folder_id, None);

        app.do_undo();
        assert_eq!(app.doc.layer_folders.len(), 1);
        assert_eq!(app.doc.empty_layers[0].folder_id, Some(folder_id));

        app.select_empty_layer(layer_id, false);
        app.delete_selected();
        assert!(app.doc.empty_layers.is_empty());
    }

    #[test]
    fn adding_an_empty_layer_to_a_selected_folder_places_it_inside() {
        let mut app = test_app();
        let folder_id = app.add_layer_folder();

        let layer_id = app.add_empty_layer();

        let layer = app
            .doc
            .empty_layers
            .iter()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert_eq!(layer.folder_id, Some(folder_id));
    }

    #[test]
    fn dragging_a_shape_between_folders_is_exclusive_and_undoable() {
        let mut app = test_app();
        let first_folder = app.add_layer_folder();
        app.clear_selection();
        let second_folder = app.add_layer_folder();
        app.clear_selection();
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: "Shape".to_owned(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        });

        assert!(app.move_layer_item_to_folder(LayerDragPayload::Face(0), first_folder));
        assert_eq!(app.doc.layer_folders[0].face_indices, vec![0]);
        assert!(app.move_layer_item_to_folder(LayerDragPayload::Face(0), second_folder));
        assert!(app.doc.layer_folders[0].face_indices.is_empty());
        assert_eq!(app.doc.layer_folders[1].face_indices, vec![0]);

        app.do_undo();
        assert_eq!(app.doc.layer_folders[0].face_indices, vec![0]);
        assert!(app.doc.layer_folders[1].face_indices.is_empty());
    }

    #[test]
    fn dropping_a_foldered_shape_into_a_root_gap_ungroups_and_reorders_it() {
        let mut app = test_app();
        let make_face = |name: &str| Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: name.to_owned(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        };
        app.doc.faces = vec![make_face("A"), make_face("B"), make_face("C")];
        let source_folder = app.add_layer_folder();
        let other_folder = app.add_layer_folder();
        app.doc.layer_folders[0].face_indices = vec![0];
        app.doc.layer_folders[1].face_indices = vec![1];
        app.set_single_selection(Some(Selection::Face(0)));

        assert!(app.move_layer_item_to_root(LayerDragPayload::Face(0), Some(2)));
        assert_eq!(
            app.doc
                .faces
                .iter()
                .map(|face| face.name.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "A", "C"]
        );
        assert!(app.doc.layer_folders[0].face_indices.is_empty());
        assert_eq!(app.doc.layer_folders[1].face_indices, vec![0]);
        assert!(app.is_selected(Selection::Face(1)));

        app.do_undo();
        assert_eq!(app.doc.layer_folders[0].id, source_folder);
        assert_eq!(app.doc.layer_folders[1].id, other_folder);
        assert_eq!(app.doc.layer_folders[0].face_indices, vec![0]);
    }

    #[test]
    fn clear_glyph_removes_all_document_content_but_keeps_grid_settings() {
        let mut app = test_app();
        app.doc.grid.spacing = 240.0;
        app.add_empty_layer();
        app.add_layer_folder();
        app.doc.custom_pegs.push(CustomPeg {
            id: 7,
            center: Point::new(80.0, 80.0),
            radius: 18.0,
            visible: true,
        });
        app.doc.disabled_pegs.push(PegRef::Grid { row: 2, col: 3 });

        app.clear_glyph();

        assert!(app.doc.faces.is_empty());
        assert!(app.doc.custom_pegs.is_empty());
        assert!(app.doc.disabled_pegs.is_empty());
        assert!(app.doc.layer_folders.is_empty());
        assert!(app.doc.empty_layers.is_empty());
        assert_eq!(app.doc.grid.spacing, 240.0);
    }

    #[test]
    fn workspace_snapshot_contains_shape_and_live_active_glyph() {
        let mut app = test_app();
        app.doc.grid.radius = 41.0;
        app.switch_workspace_mode(WorkspaceMode::Font);
        app.doc.grid.radius = 27.0;
        app.proof_text = "AV test".to_owned();
        app.proof_size = 132.0;
        app.proof_tracking = 14.0;
        app.onion_skin = true;
        app.onion_opacity = 0.31;

        let snapshot = app.workspace_snapshot();
        assert_eq!(snapshot.shape.grid.radius, 41.0);
        assert_eq!(
            snapshot.font.glyphs[snapshot.font.active_glyph]
                .document
                .grid
                .radius,
            27.0
        );
        let encoded = serde_json::to_string(&snapshot).unwrap();
        let decoded: WorkspaceFile = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.mode, WorkspaceMode::Font);
        assert_eq!(decoded.shape.grid.radius, 41.0);
        assert_eq!(decoded.proof_text, "AV test");
        assert_eq!(decoded.proof_size, 132.0);
        assert_eq!(decoded.proof_tracking, 14.0);
        assert!(decoded.onion_skin);
        assert_eq!(decoded.onion_opacity, 0.31);
    }

    #[test]
    fn legacy_workspace_defaults_new_editor_settings() {
        let app = test_app();
        let mut value = serde_json::to_value(app.workspace_snapshot()).unwrap();
        let object = value.as_object_mut().unwrap();
        for key in [
            "dock",
            "proof_text",
            "proof_size",
            "proof_tracking",
            "onion_skin",
            "onion_next",
            "onion_opacity",
            "onion_previous_color",
            "onion_next_color",
            "reference_font_path",
            "reference_font_face_index",
            "reference_font_opacity",
            "reference_font_scale",
            "reference_font_offset",
        ] {
            object.remove(key);
        }
        let restored: WorkspaceFile = serde_json::from_value(value).unwrap();
        assert_eq!(restored.proof_text, default_proof_text());
        assert_eq!(restored.proof_size, default_proof_size());
        assert_eq!(
            restored.dock.left_fraction,
            WorkspaceDockLayout::default().left_fraction
        );
        assert_eq!(restored.reference_font_scale, 1.0);
    }

    #[test]
    fn switching_glyphs_restores_document_and_undo_history() {
        let mut app = test_app();
        app.switch_workspace_mode(WorkspaceMode::Font);

        let original_radius = app.doc.grid.radius;
        app.push_history();
        app.doc.grid.radius = 42.0;

        app.switch_glyph(1);
        app.push_history();
        app.doc.grid.radius = 36.0;

        app.switch_glyph(0);
        assert_eq!(app.doc.grid.radius, 42.0);
        app.do_undo();
        assert_eq!(app.doc.grid.radius, original_radius);
    }

    #[test]
    fn background_save_uses_snapshot_while_editing_continues() {
        let mut app = test_app();
        app.doc.grid.radius = 42.0;
        let path = std::env::temp_dir().join(format!(
            "threadtype-save-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        app.start_background_save(path.clone(), SavePayload::Shape(app.doc.clone()), "test");
        app.doc.grid.radius = 36.0;

        let operation = app.save_operation.as_mut().unwrap();
        let result = loop {
            match operation
                .receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
            {
                SaveEvent::Progress(_) => {}
                SaveEvent::Finished(result) => break result,
            }
        };
        result.unwrap();
        let saved: Document = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.grid.radius, 42.0);
        assert_eq!(app.doc.grid.radius, 36.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn onion_skin_uses_previous_glyph_document() {
        let mut app = test_app();
        app.switch_workspace_mode(WorkspaceMode::Font);
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: "previous".to_owned(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(10.0, 10.0),
                    Point::new(80.0, 10.0),
                    Point::new(40.0, 90.0),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        });
        app.switch_glyph(1);
        app.onion_skin = true;

        let (character, rings) = app.glyph_rings(0).unwrap();
        assert_eq!(character, 'A');
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 3);
    }

    #[test]
    fn onion_skin_can_read_next_glyph_document() {
        let mut app = test_app();
        app.switch_workspace_mode(WorkspaceMode::Font);
        app.switch_glyph(2);
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: "next".to_owned(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(10.0, 10.0),
                    Point::new(80.0, 10.0),
                    Point::new(40.0, 90.0),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        });
        app.switch_glyph(1);
        app.onion_next = true;

        let (character, rings) = app.glyph_rings(2).unwrap();
        assert_eq!(character, 'C');
        assert_eq!(rings.len(), 1);
    }

    #[test]
    fn proof_outline_is_cached_until_the_glyph_changes() {
        let mut app = test_app();
        app.switch_workspace_mode(WorkspaceMode::Font);
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: "proof-cache".to_owned(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(0.0, 0.0),
                    Point::new(100.0, 0.0),
                    Point::new(100.0, 100.0),
                    Point::new(0.0, 100.0),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        });
        let (_, first) = app.proof_glyph_rings(0).unwrap();
        let (_, second) = app.proof_glyph_rings(0).unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        app.doc.faces[0].baked.as_mut().unwrap().rings[0][0].x = 10.0;
        let (_, changed) = app.proof_glyph_rings(0).unwrap();
        assert!(!Arc::ptr_eq(&second, &changed));
    }

    #[test]
    fn proof_fill_adds_opposite_overlapping_faces_instead_of_knocking_them_out() {
        let mut app = test_app();
        app.switch_workspace_mode(WorkspaceMode::Font);
        app.font_project.render_style = FontRenderStyle::Fill;
        let ring = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
        ];
        for points in [ring.clone(), ring.into_iter().rev().collect()] {
            app.doc.faces.push(Face {
                start: StartAnchor {
                    peg: PegRef::Grid { row: 0, col: 0 },
                    port: Port::Top,
                },
                elements: Vec::new(),
                name: "overlap".to_owned(),
                visible: true,
                baked: Some(crate::model::BakedShape {
                    rings: vec![points],
                    bevel: 0.0,
                }),
                ignored_pegs: Vec::new(),
            });
        }
        let (_, rings) = app.proof_glyph_rings(0).unwrap();
        let area: f64 = rings.iter().map(|ring| signed_area(ring).abs()).sum();
        assert!(area > 9_000.0);
    }

    fn test_app() -> App {
        let font_project = FontProject::default();
        let glyph_count = font_project.glyphs.len();
        App {
            doc: Document::default(),
            workspace_mode: WorkspaceMode::Shape,
            shape_document: Document::default(),
            shape_undo: Vec::new(),
            shape_redo: Vec::new(),
            font_project,
            glyph_undo: vec![Vec::new(); glyph_count],
            glyph_redo: vec![Vec::new(); glyph_count],
            new_glyph_text: String::new(),
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
            language: UiLanguage::Japanese,
            drag_snapshot: None,
            grid_edit_orig: None,
            radius_edit_orig: None,
            bevel_edit_orig: None,
            draw: None,
            selection: Vec::new(),
            selected_empty_layers: std::collections::HashSet::new(),
            selected_layer_folders: std::collections::HashSet::new(),
            dock: App::default_dock_layout(),
            active_center_tab: DockTab::Canvas,
            pending_panel_tab: None,
            face_editor: None,
            face_name_snapshot: None,
            layer_item_editor: None,
            layer_item_name_snapshot: None,
            tool: Tool::Draw,
            status: String::new(),
            shape_save_path: None,
            font_save_path: None,
            workspace_save_path: None,
            save_operation: None,
            onion_skin: false,
            onion_next: false,
            onion_opacity: 0.42,
            onion_previous_color: Color32::from_rgb(116, 184, 255),
            onion_next_color: Color32::from_rgb(255, 137, 174),
            proof_text: "Hamburgefontsiv 0123".to_owned(),
            proof_size: 64.0,
            proof_tracking: 0.0,
            proof_cache: HashMap::new(),
            reference_font: None,
            reference_font_opacity: 0.28,
            reference_font_scale: 1.0,
            reference_font_offset: Vec2::ZERO,
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
        assert!(!app.segments_self_intersect(&segs));
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
        assert!(app.segments_self_intersect(&segs));
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
    fn zero_signed_area_self_crossing_still_has_shape_extent() {
        let app = test_app();
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(120.0, 120.0),
            Point::new(0.0, 120.0),
            Point::new(120.0, 0.0),
            Point::new(0.0, 0.0),
        ];
        assert!(app.has_self_intersections(&points));
        assert!(signed_area(&points).abs() < 1e-9);
        assert!(path_has_two_dimensional_extent(&points));
    }

    #[test]
    fn collinear_round_trip_remains_degenerate() {
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(120.0, 0.0),
            Point::new(30.0, 0.0),
            Point::new(0.0, 0.0),
        ];
        assert!(!path_has_two_dimensional_extent(&points));
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            start_point: anchor,
            elements: Vec::new(),
            armed: true,
            ignored: vec![3],
            undo_guard: None,
            undo_guard_origin: None,
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            start_point: Point::new(0.0, 0.0),
            elements: Vec::new(),
            armed: true,
            ignored: Vec::new(),
            undo_guard: None,
            undo_guard_origin: None,
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
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: String::new(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(1.0, 2.0),
                    Point::new(3.0, 2.0),
                    Point::new(1.0, 4.0),
                ]],
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

    #[test]
    fn baked_faces_follow_grid_spacing_without_changing_bevel_amount() {
        let mut app = test_app();
        app.doc.grid.spacing = 200.0;
        app.doc.grid.scale_anchor = ScaleAnchor::TopLeft;
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: String::new(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    Point::new(110.0, 110.0),
                    Point::new(310.0, 110.0),
                    Point::new(110.0, 310.0),
                ]],
                bevel: 32.0,
            }),
            ignored_pegs: Vec::new(),
        });
        let source = app.doc.clone();
        app.doc.grid.spacing = 300.0;

        assert!(App::remap_baked_shapes_for_grid_change(
            &mut app.doc,
            &source,
            app.canvas_center,
        ));
        let baked = app.doc.faces[0].baked.as_ref().unwrap();
        assert_eq!(baked.rings[0][0], Point::new(110.0, 110.0));
        assert_eq!(baked.rings[0][1], Point::new(410.0, 110.0));
        assert_eq!(baked.rings[0][2], Point::new(110.0, 410.0));
        assert_eq!(baked.bevel, 32.0);
    }

    #[test]
    fn baked_faces_keep_the_reference_cell_fixed_for_center_scaling() {
        let mut app = test_app();
        app.doc.grid.spacing = 200.0;
        app.doc.grid.scale_anchor = ScaleAnchor::Center;
        let center = app.canvas_center;
        app.doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Top,
            },
            elements: Vec::new(),
            name: String::new(),
            visible: true,
            baked: Some(crate::model::BakedShape {
                rings: vec![vec![
                    center,
                    Point::new(center.x - 600.0, center.y - 400.0),
                    Point::new(center.x + 200.0, center.y),
                ]],
                bevel: 0.0,
            }),
            ignored_pegs: Vec::new(),
        });
        let source = app.doc.clone();
        app.doc.grid.spacing = 300.0;

        App::remap_baked_shapes_for_grid_change(&mut app.doc, &source, center);
        let ring = &app.doc.faces[0].baked.as_ref().unwrap().rings[0];
        assert_eq!(ring[0], center);
        assert_eq!(ring[1], Point::new(center.x - 900.0, center.y - 600.0));
        assert_eq!(ring[2], Point::new(center.x + 300.0, center.y));
    }

    #[test]
    fn triangle_closure_checks_all_winding_combinations() {
        let wrap = |id| Element::Wrap {
            peg: PegRef::Custom { id },
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        };
        let variants = winding_variants(&[wrap(0), wrap(1), wrap(2)]);
        assert_eq!(variants.len(), 8);
        assert!(variants.iter().any(|variant| {
            variant
                .iter()
                .all(|element| matches!(element, Element::Wrap { s, .. } if *s < 0.0))
        }));
    }

    #[test]
    fn closure_prefers_captured_concavity_over_shorter_flipped_path() {
        let wrap = |id, s| Element::Wrap {
            peg: PegRef::Custom { id },
            s,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        };
        // 中央の円だけ逆向きに捕捉した状態が、凹みを作るユーザー意図。
        let captured = vec![wrap(0, 1.0), wrap(1, -1.0), wrap(2, 1.0)];
        let filled = vec![wrap(0, 1.0), wrap(1, 1.0), wrap(2, 1.0)];
        assert_eq!(winding_change_count(&captured, &captured), 0);
        assert_eq!(winding_change_count(&captured, &filled), 1);
        assert!(
            !closure_score_is_better(1, 1.0, Some((0, 10.0))),
            "a shorter path must not override the captured winding direction"
        );
        assert!(
            closure_score_is_better(0, 8.0, Some((0, 10.0))),
            "arc length only breaks ties with equal intent fidelity"
        );
    }

    #[test]
    fn cyclic_duplicate_start_contact_is_merged_before_closing() {
        let wrap = |id, turns| Element::Wrap {
            peg: PegRef::Custom { id },
            s: 1.0,
            turns,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        };
        let variants = winding_variants(&[wrap(0, 1), wrap(1, 0), wrap(0, 2)]);
        assert_eq!(
            variants.len(),
            4,
            "two unique contacts have four sign combinations"
        );
        assert!(variants.iter().all(|variant| variant.len() == 2));
        assert!(matches!(
            variants[0][0],
            Element::Wrap {
                peg: PegRef::Custom { id: 0 },
                turns: 3,
                ..
            }
        ));
    }

    #[test]
    fn nonconsecutive_revisited_peg_is_preserved_for_self_crossing_cycle() {
        let wrap = |id| Element::Wrap {
            peg: PegRef::Custom { id },
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        };
        let variants = winding_variants(&[wrap(0), wrap(1), wrap(0), wrap(2)]);
        assert_eq!(variants.len(), 16);
        assert!(variants.iter().all(|variant| {
            variant.len() == 4
                && matches!(
                    variant[0],
                    Element::Wrap {
                        peg: PegRef::Custom { id: 0 },
                        ..
                    }
                )
                && matches!(
                    variant[2],
                    Element::Wrap {
                        peg: PegRef::Custom { id: 0 },
                        ..
                    }
                )
        }));
    }

    #[test]
    fn revisited_peg_cycle_materializes_without_penetrating_pegs() {
        let pegs = [
            Peg {
                center: Point::new(0.0, 0.0),
                radius: 40.0,
                shape: crate::model::PegShape::Circle,
                rounding: 0.0,
            },
            Peg {
                center: Point::new(180.0, -140.0),
                radius: 40.0,
                shape: crate::model::PegShape::Circle,
                rounding: 0.0,
            },
            Peg {
                center: Point::new(180.0, 140.0),
                radius: 40.0,
                shape: crate::model::PegShape::Circle,
                rounding: 0.0,
            },
        ];
        let refs = [
            PegRef::Custom { id: 0 },
            PegRef::Custom { id: 1 },
            PegRef::Custom { id: 2 },
        ];
        let wrap = |peg| Element::Wrap {
            peg,
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        };
        let resolver = |peg_ref: &PegRef| match *peg_ref {
            PegRef::Custom { id } => pegs.get(id as usize).copied(),
            _ => None,
        };
        let obstacles: Vec<_> = refs.iter().copied().zip(pegs.iter().copied()).collect();
        let candidate =
            winding_variants(&[wrap(refs[0]), wrap(refs[1]), wrap(refs[0]), wrap(refs[2])])
                .into_iter()
                .find_map(|elements| {
                    let face = Face {
                        start: StartAnchor {
                            peg: refs[0],
                            port: Port::Top,
                        },
                        elements,
                        name: String::new(),
                        visible: true,
                        baked: None,
                        ignored_pegs: Vec::new(),
                    };
                    let segments = materialize_face_with_obstacles(&face, &resolver, &obstacles)?;
                    (!segments_penetrate_pegs(&segments, &pegs)).then_some(segments)
                })
                .expect("a cycle that revisits one peg should still have a physical solution");
        let revisited_arc_count = candidate
            .iter()
            .filter(|segment| {
                matches!(segment, Seg::Arc { center, .. } if center.distance(pegs[0].center) < 1e-8)
            })
            .count();
        assert_eq!(revisited_arc_count, 2);
    }

    #[test]
    fn one_circle_cannot_create_degenerate_closed_face() {
        let one = Face {
            start: StartAnchor {
                peg: PegRef::Custom { id: 0 },
                port: Port::Top,
            },
            elements: vec![Element::Wrap {
                peg: PegRef::Custom { id: 0 },
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: std::f64::consts::PI,
            }],
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        };
        assert!(!face_has_minimum_constraints(&one));
        let mut two = one.clone();
        two.elements.push(Element::Wrap {
            peg: PegRef::Custom { id: 1 },
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        });
        assert!(face_has_minimum_constraints(&two));

        let mut intentional = one.clone();
        if let Element::Wrap { sweep_hint, .. } = &mut intentional.elements[0] {
            *sweep_hint = std::f64::consts::TAU;
        }
        assert!(face_has_minimum_constraints(&intentional));
    }

    #[test]
    fn single_circle_requires_half_turn_and_normalizes_to_full_circle() {
        let start = StartAnchor {
            peg: PegRef::Custom { id: 0 },
            port: Port::Top,
        };
        let refs = [start.peg];
        let make_thread = |sweep: f64| Thread {
            anchor: Point::new(0.0, -50.0),
            wraps: vec![crate::geometry::Wrap {
                peg: 0,
                s: 1.0,
                entry: 0.0,
                exit: sweep,
            }],
            tip: Point::new(0.0, -50.0),
        };
        assert!(intentional_single_circle_wrap(
            &make_thread(std::f64::consts::PI - 0.01),
            start,
            &refs,
        )
        .is_none());
        let full =
            intentional_single_circle_wrap(&make_thread(std::f64::consts::PI + 0.01), start, &refs)
                .unwrap();
        assert!(matches!(
            &full,
            Element::Wrap { sweep_hint, turns: 0, .. }
                if (*sweep_hint - std::f64::consts::TAU).abs() < 1e-9
        ));
        let face = Face {
            start,
            elements: vec![full],
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let resolver = |peg_ref: &PegRef| {
            (*peg_ref == start.peg).then_some(Peg {
                center: Point::new(0.0, 0.0),
                radius: 50.0,
                shape: crate::model::PegShape::Circle,
                rounding: 0.0,
            })
        };
        let segs = materialize_face(&face, &resolver).unwrap();
        let mut points = Vec::new();
        let app = test_app();
        app.flatten_segments_for_appearance(&segs, &mut points, 0.2);
        assert!(signed_area(&points).abs() > 7_000.0);
    }

    #[test]
    fn undo_guard_releases_after_pointer_moves_even_inside_peg() {
        let peg = Peg {
            center: Point::new(0.0, 0.0),
            radius: 50.0,
            shape: crate::model::PegShape::Circle,
            rounding: 0.0,
        };
        assert!(!should_release_undo_guard(
            Point::new(1.0, 0.0),
            Some(Point::new(0.0, 0.0)),
            Some(&peg),
            6.0,
        ));
        assert!(should_release_undo_guard(
            Point::new(8.0, 0.0),
            Some(Point::new(0.0, 0.0)),
            Some(&peg),
            6.0,
        ));
    }

    fn assert_simple_cycle_can_close(centers: &[Point], radii: &[f64]) {
        let pegs: Vec<Peg> = centers
            .iter()
            .zip(radii)
            .map(|(center, radius)| Peg {
                center: *center,
                radius: *radius,
                shape: crate::model::PegShape::Circle,
                rounding: 0.0,
            })
            .collect();
        let refs: Vec<PegRef> = (0..pegs.len())
            .map(|id| PegRef::Custom { id: id as u64 })
            .collect();
        let seed: Vec<Element> = refs
            .iter()
            .map(|peg| Element::Wrap {
                peg: *peg,
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: std::f64::consts::PI,
            })
            .collect();
        let resolver = |peg_ref: &PegRef| match *peg_ref {
            PegRef::Custom { id } => pegs.get(id as usize).copied(),
            _ => None,
        };
        let obstacles: Vec<(PegRef, Peg)> =
            refs.iter().copied().zip(pegs.iter().copied()).collect();
        let obstacle_pegs: Vec<Peg> = pegs.clone();
        let valid = winding_variants(&seed).into_iter().any(|elements| {
            let face = Face {
                start: StartAnchor {
                    peg: refs[0],
                    port: Port::Top,
                },
                elements,
                name: String::new(),
                visible: true,
                baked: None,
                ignored_pegs: Vec::new(),
            };
            let Some(segs) = materialize_face_with_obstacles(&face, &resolver, &obstacles) else {
                return false;
            };
            !segments_penetrate_pegs(&segs, &obstacle_pegs)
        });
        assert!(
            valid,
            "at least one winding combination must form a valid cycle"
        );
    }

    #[test]
    fn two_equal_circles_can_form_capsule_cycle() {
        assert_simple_cycle_can_close(
            &[Point::new(0.0, 0.0), Point::new(200.0, 0.0)],
            &[60.0, 60.0],
        );
    }

    #[test]
    fn three_equal_circles_can_form_triangle_cycle() {
        assert_simple_cycle_can_close(
            &[
                Point::new(0.0, 0.0),
                Point::new(200.0, 0.0),
                Point::new(100.0, 170.0),
            ],
            &[60.0, 60.0, 60.0],
        );
    }

    #[test]
    fn two_sub_sized_circles_can_form_capsule_cycle() {
        assert_simple_cycle_can_close(
            &[Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
            &[24.0, 24.0],
        );
    }
}
