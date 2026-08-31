use crate::geometry::Peg;
use kurbo::Point;
use serde::{Deserialize, Serialize};

pub const CURRENT_DOCUMENT_VERSION: u32 = 5;
pub const CURRENT_FONT_PROJECT_VERSION: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PegRef {
    Grid { row: i32, col: i32 },
    Sub { row: i32, col: i32 },
    Custom { id: u64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomPeg {
    pub id: u64,
    pub center: Point,
    pub radius: f64,
    #[serde(default = "default_true")]
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Port {
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
}

impl Port {
    pub const ALL: [Port; 8] = [
        Port::Top,
        Port::TopRight,
        Port::Right,
        Port::BottomRight,
        Port::Bottom,
        Port::BottomLeft,
        Port::Left,
        Port::TopLeft,
    ];

    /// 円周上の角度（画面座標、y 下向き）
    pub fn angle(self) -> f64 {
        use std::f64::consts::PI;
        match self {
            Port::Top => -PI / 2.0,
            Port::TopRight => -PI / 4.0,
            Port::Right => 0.0,
            Port::BottomRight => PI / 4.0,
            Port::Bottom => PI / 2.0,
            Port::BottomLeft => 3.0 * PI / 4.0,
            Port::Left => PI,
            Port::TopLeft => -3.0 * PI / 4.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartAnchor {
    pub peg: PegRef,
    pub port: Port,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleAnchor {
    TopLeft,
    Center,
}

fn default_sweep_hint() -> f64 {
    std::f64::consts::PI
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Element {
    Wrap {
        peg: PegRef,
        s: f64,
        turns: u32,
        /// 把持オフセット（摩擦モデル）。自然な接線離脱角からのズレ。
        /// 0 なら純粋なタウトストリングの接線に一致する
        #[serde(default)]
        exit_offset: f64,
        /// 確定時の自然掃引量（ラジアン）。グリッド変更後の再実体化で
        /// 掃引が 0/2π を跨いだとき、これに最も近い枝を選ぶことで
        /// 「突然ほぼ一周の弧が生える」破綻を防ぐ
        #[serde(default = "default_sweep_hint")]
        sweep_hint: f64,
    },
    Pin {
        pos: Point,
    },
}

/// マージ（パス結合）で焼き込まれた形状。ユニオン結果のリング群を保持し、
/// ベベル量は描画時に適用する（スライダーで非破壊に変えられる）
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BakedShape {
    /// ユニオン結果のリング列（外周＋穴、NonZero）
    pub rings: Vec<Vec<Point>>,
    /// ベベル（面取り）量。0 なら角のまま
    pub bevel: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub start: StartAnchor,
    pub elements: Vec<Element>,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Some のときはパラメトリック要素ではなく焼き込みリングを描く（マージ済み）
    #[serde(default)]
    pub baked: Option<BakedShape>,
    /// Commandで意図的に貫通させた円。完成後・再読込後も同じ経路を
    /// 検証できるよう、描画中だけでなく面に永続化する。
    #[serde(default)]
    pub ignored_pegs: Vec<PegRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerFolder {
    pub id: u64,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub expanded: bool,
    /// `Document::faces` のインデックス。面の削除時にDocument側で詰め直す。
    #[serde(default)]
    pub face_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmptyLayer {
    pub id: u64,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub folder_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSettings {
    pub spacing: f64,
    #[serde(default = "default_true")]
    pub lines_enabled: bool,
    #[serde(default = "default_true")]
    pub snap_to_lines: bool,
    #[serde(default = "default_true")]
    pub pegs_enabled: bool,
    pub radius: f64,
    pub catch: f64,
    pub sub_radius: f64,
    pub subs_enabled: bool,
    pub scale_link: bool,
    pub scale_anchor: ScaleAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PegShape {
    Circle,
    RoundedSquare,
}

impl Default for PegShape {
    fn default() -> Self {
        Self::Circle
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PegAppearance {
    #[serde(default)]
    pub shape: PegShape,
    #[serde(default = "default_peg_rounding")]
    pub rounding: f64,
}

fn default_peg_rounding() -> f64 {
    12.0
}

impl Default for PegAppearance {
    fn default() -> Self {
        Self {
            shape: PegShape::Circle,
            rounding: default_peg_rounding(),
        }
    }
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            spacing: 200.0,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            radius: 60.0,
            catch: 6.0,
            sub_radius: 24.0,
            subs_enabled: false,
            scale_link: false,
            scale_anchor: ScaleAnchor::TopLeft,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub grid: GridSettings,
    pub custom_pegs: Vec<CustomPeg>,
    pub next_peg_id: u64,
    pub faces: Vec<Face>,
    #[serde(default)]
    pub peg_appearance: PegAppearance,
    #[serde(default)]
    pub disabled_pegs: Vec<PegRef>,
    #[serde(default)]
    pub layer_folders: Vec<LayerFolder>,
    #[serde(default)]
    pub empty_layers: Vec<EmptyLayer>,
    #[serde(default)]
    pub next_layer_item_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            version: CURRENT_DOCUMENT_VERSION,
            grid: GridSettings::default(),
            custom_pegs: Vec::new(),
            next_peg_id: 0,
            faces: Vec::new(),
            peg_appearance: PegAppearance::default(),
            disabled_pegs: Vec::new(),
            layer_folders: Vec::new(),
            empty_layers: Vec::new(),
            next_layer_item_id: 1,
        }
    }
}

impl Document {
    pub fn allocate_layer_item_id(&mut self) -> u64 {
        let id = self.next_layer_item_id.max(1);
        self.next_layer_item_id = id.saturating_add(1);
        id
    }

    pub fn remove_face_from_layer_metadata(&mut self, removed: usize) {
        for folder in &mut self.layer_folders {
            folder.face_indices.retain(|index| *index != removed);
            for index in &mut folder.face_indices {
                if *index > removed {
                    *index -= 1;
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlyphRecord {
    pub character: char,
    pub name: String,
    pub advance_width: u16,
    /// デザイン箱の左端からグリフ輪郭までの追加余白（font units）。
    #[serde(default)]
    pub left_side_bearing: i16,
    pub document: Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontMetricsGuides {
    /// フォント座標の x=0 に対応するキャンバス上の位置。
    pub origin_x: f64,
    #[serde(default = "default_metrics_right_x")]
    pub right_x: f64,
    /// 以下はすべてキャンバス座標（yは下向き）。
    pub ascender_y: f64,
    pub x_height_y: f64,
    pub baseline_y: f64,
    pub descender_y: f64,
}

impl FontMetricsGuides {
    pub fn scale(self) -> f64 {
        1000.0 / (self.descender_y - self.ascender_y).max(40.0)
    }

    pub fn horizontal_scale(self) -> f64 {
        // 左右端は配置・送りの箱であり、作図輪郭そのものを横方向だけ
        // 引き伸ばさない。縦と同じ倍率でアスペクト比を保持する。
        self.scale()
    }

    pub fn ascender_units(self) -> i16 {
        ((self.baseline_y - self.ascender_y) * self.scale()).round() as i16
    }

    pub fn descender_units(self) -> i16 {
        -((self.descender_y - self.baseline_y) * self.scale()).round() as i16
    }

    pub fn x_height_units(self) -> i16 {
        ((self.baseline_y - self.x_height_y) * self.scale()).round() as i16
    }
}

impl Default for FontMetricsGuides {
    fn default() -> Self {
        Self {
            origin_x: 70.0,
            right_x: default_metrics_right_x(),
            ascender_y: 70.0,
            x_height_y: 310.0,
            baseline_y: 710.0,
            descender_y: 870.0,
        }
    }
}

fn default_metrics_right_x() -> f64 {
    550.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FontProject {
    pub version: u32,
    pub family_name: String,
    pub style_name: String,
    #[serde(default)]
    pub metrics: FontMetricsGuides,
    #[serde(default)]
    pub render_style: FontRenderStyle,
    #[serde(default = "default_thread_width")]
    pub thread_width: f64,
    pub active_glyph: usize,
    pub glyphs: Vec<GlyphRecord>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FontRenderStyle {
    /// 作図時の糸を一定幅の実輪郭へ変換する。
    #[default]
    Thread,
    /// 閉じた領域を通常どおり塗る。
    Fill,
    /// 塗りの外周へ糸幅を足す。単色フォントでは同色で合成される。
    FillAndThread,
}

fn default_thread_width() -> f64 {
    3.0
}

pub fn default_font_characters() -> Vec<char> {
    let mut chars: Vec<char> = ('A'..='Z').collect();
    chars.extend('a'..='z');
    chars.extend('0'..='9');
    chars.extend([
        ' ', '.', ',', ':', ';', '!', '?', '-', '_', '(', ')', '[', ']', '{', '}', '\'', '"', '/',
        '\\', '&', '@', '#', '%', '+', '=', '*',
    ]);
    chars
}

pub fn glyph_name(character: char) -> String {
    match character {
        ' ' => "space".to_owned(),
        '.' => "period".to_owned(),
        ',' => "comma".to_owned(),
        ':' => "colon".to_owned(),
        ';' => "semicolon".to_owned(),
        '!' => "exclam".to_owned(),
        '?' => "question".to_owned(),
        '-' => "hyphen".to_owned(),
        '_' => "underscore".to_owned(),
        '(' => "parenleft".to_owned(),
        ')' => "parenright".to_owned(),
        '[' => "bracketleft".to_owned(),
        ']' => "bracketright".to_owned(),
        '{' => "braceleft".to_owned(),
        '}' => "braceright".to_owned(),
        '\'' => "quotesingle".to_owned(),
        '"' => "quotedbl".to_owned(),
        '/' => "slash".to_owned(),
        '\\' => "backslash".to_owned(),
        '&' => "ampersand".to_owned(),
        '@' => "at".to_owned(),
        '#' => "numbersign".to_owned(),
        '%' => "percent".to_owned(),
        '+' => "plus".to_owned(),
        '=' => "equal".to_owned(),
        '*' => "asterisk".to_owned(),
        '0' => "zero".to_owned(),
        '1' => "one".to_owned(),
        '2' => "two".to_owned(),
        '3' => "three".to_owned(),
        '4' => "four".to_owned(),
        '5' => "five".to_owned(),
        '6' => "six".to_owned(),
        '7' => "seven".to_owned(),
        '8' => "eight".to_owned(),
        '9' => "nine".to_owned(),
        value if value.is_ascii_uppercase() => value.to_string(),
        value if value.is_ascii_lowercase() => value.to_string(),
        value => format!("uni{:04X}", value as u32),
    }
}

impl Default for FontProject {
    fn default() -> Self {
        let glyphs = default_font_characters()
            .into_iter()
            .map(|character| GlyphRecord {
                character,
                name: glyph_name(character),
                advance_width: if character == ' ' { 300 } else { 600 },
                left_side_bearing: 0,
                document: Document::default(),
            })
            .collect();
        FontProject {
            version: CURRENT_FONT_PROJECT_VERSION,
            family_name: "ThreadType".to_owned(),
            style_name: "Regular".to_owned(),
            metrics: FontMetricsGuides::default(),
            render_style: FontRenderStyle::default(),
            thread_width: default_thread_width(),
            active_glyph: 0,
            glyphs,
        }
    }
}

impl FontProject {
    /// 古いプロジェクトへ、後から標準セットに加わったグリフを安全に補う。
    /// 既存グリフのDocument・送り幅・並びは変更しない。
    pub fn ensure_default_glyphs(&mut self) {
        let active_character = self
            .glyphs
            .get(self.active_glyph)
            .map(|glyph| glyph.character);
        for character in default_font_characters() {
            if self.glyphs.iter().any(|glyph| glyph.character == character) {
                continue;
            }
            self.glyphs.push(GlyphRecord {
                character,
                name: glyph_name(character),
                advance_width: if character == ' ' { 300 } else { 600 },
                left_side_bearing: 0,
                document: Document::default(),
            });
        }
        if let Some(character) = active_character {
            if let Some(index) = self
                .glyphs
                .iter()
                .position(|glyph| glyph.character == character)
            {
                self.active_glyph = index;
            }
        }
    }
}

pub fn resolve_peg(
    grid: &GridSettings,
    customs: &[CustomPeg],
    canvas_center: Point,
    appearance: PegAppearance,
    peg: &PegRef,
) -> Option<Peg> {
    match (grid.scale_anchor, *peg) {
        (ScaleAnchor::TopLeft, PegRef::Grid { row, col }) => Some(Peg {
            center: Point::new(
                110.0 + (col as f64) * grid.spacing,
                110.0 + (row as f64) * grid.spacing,
            ),
            radius: grid.radius + grid.catch,
            shape: appearance.shape,
            rounding: appearance.rounding + grid.catch,
        }),
        (ScaleAnchor::TopLeft, PegRef::Sub { row, col }) => Some(Peg {
            center: Point::new(
                110.0 + (col as f64 + 0.5) * grid.spacing,
                110.0 + (row as f64 + 0.5) * grid.spacing,
            ),
            radius: grid.sub_radius + grid.catch,
            shape: appearance.shape,
            rounding: appearance.rounding + grid.catch,
        }),
        (ScaleAnchor::Center, PegRef::Grid { row, col }) => Some(Peg {
            center: Point::new(
                canvas_center.x + ((col as f64) - 3.0) * grid.spacing,
                canvas_center.y + ((row as f64) - 2.0) * grid.spacing,
            ),
            radius: grid.radius + grid.catch,
            shape: appearance.shape,
            rounding: appearance.rounding + grid.catch,
        }),
        (ScaleAnchor::Center, PegRef::Sub { row, col }) => Some(Peg {
            center: Point::new(
                canvas_center.x + ((col as f64 + 0.5) - 3.0) * grid.spacing,
                canvas_center.y + ((row as f64 + 0.5) - 2.0) * grid.spacing,
            ),
            radius: grid.sub_radius + grid.catch,
            shape: appearance.shape,
            rounding: appearance.rounding + grid.catch,
        }),
        (_, PegRef::Custom { id }) => customs.iter().find(|peg| peg.id == id).map(|peg| Peg {
            center: peg.center,
            radius: peg.radius + grid.catch,
            shape: appearance.shape,
            rounding: appearance.rounding + grid.catch,
        }),
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{flatten_segs, materialize_face, signed_area};
    use crate::model::CustomPeg;

    #[test]
    fn default_font_project_contains_lowercase_and_punctuation() {
        let project = FontProject::default();
        for character in [
            'A', 'Z', 'a', 'z', '0', '9', ' ', '.', ',', '!', '?', '&', '@',
        ] {
            assert!(project
                .glyphs
                .iter()
                .any(|glyph| glyph.character == character));
        }
        assert_eq!(project.glyphs[project.active_glyph].character, 'A');
        assert_eq!(project.glyphs[project.active_glyph].left_side_bearing, 0);
        assert_eq!(
            project
                .glyphs
                .iter()
                .find(|glyph| glyph.character == ' ')
                .unwrap()
                .advance_width,
            300
        );
    }

    #[test]
    fn default_glyph_names_are_unique() {
        let project = FontProject::default();
        let mut names: Vec<&str> = project
            .glyphs
            .iter()
            .map(|glyph| glyph.name.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), project.glyphs.len());
    }

    #[test]
    fn default_font_metrics_map_to_standard_em_values() {
        let metrics = FontMetricsGuides::default();
        assert_eq!(metrics.ascender_units(), 800);
        assert_eq!(metrics.x_height_units(), 500);
        assert_eq!(metrics.descender_units(), -200);
        assert!((metrics.right_x - 550.0).abs() < 1e-9);
        assert!((metrics.horizontal_scale() - 1.25).abs() < 1e-9);
        let mut wider_box = metrics;
        wider_box.right_x = 900.0;
        assert_eq!(wider_box.horizontal_scale(), wider_box.scale());
    }

    #[test]
    fn font_project_without_metrics_uses_default_guides() {
        let mut value = serde_json::to_value(FontProject::default()).unwrap();
        value.as_object_mut().unwrap().remove("metrics");
        let restored: FontProject = serde_json::from_value(value).unwrap();
        assert_eq!(restored.metrics, FontMetricsGuides::default());
    }

    #[test]
    fn metrics_without_right_edge_uses_fixed_default_box() {
        let project = FontProject::default();
        let mut value = serde_json::to_value(project).unwrap();
        value["metrics"].as_object_mut().unwrap().remove("right_x");
        let restored: FontProject = serde_json::from_value(value).unwrap();
        assert_eq!(restored.metrics.right_x, 550.0);
    }

    #[test]
    fn legacy_glyph_without_side_bearing_defaults_to_zero() {
        let project = FontProject::default();
        let mut value = serde_json::to_value(project).unwrap();
        value["glyphs"][0]
            .as_object_mut()
            .unwrap()
            .remove("left_side_bearing");
        let restored: FontProject = serde_json::from_value(value).unwrap();
        assert_eq!(restored.glyphs[0].left_side_bearing, 0);
    }

    #[test]
    fn legacy_font_project_defaults_to_thread_output() {
        let mut value = serde_json::to_value(FontProject::default()).unwrap();
        value.as_object_mut().unwrap().remove("render_style");
        value.as_object_mut().unwrap().remove("thread_width");
        let restored: FontProject = serde_json::from_value(value).unwrap();
        assert_eq!(restored.render_style, FontRenderStyle::Thread);
        assert_eq!(restored.thread_width, 3.0);
    }

    #[test]
    fn legacy_font_project_gets_missing_uppercase_and_digits() {
        let mut project = FontProject::default();
        project
            .glyphs
            .retain(|glyph| glyph.character.is_ascii_lowercase());
        project.active_glyph = project
            .glyphs
            .iter()
            .position(|glyph| glyph.character == 'm')
            .unwrap();
        project.glyphs[project.active_glyph].document.grid.radius = 42.0;

        project.ensure_default_glyphs();

        assert!(project.glyphs.iter().any(|glyph| glyph.character == 'A'));
        assert!(project.glyphs.iter().any(|glyph| glyph.character == '0'));
        assert_eq!(project.glyphs[project.active_glyph].character, 'm');
        assert_eq!(
            project.glyphs[project.active_glyph].document.grid.radius,
            42.0
        );
    }

    #[test]
    fn sub_pegs_are_off_by_default() {
        assert!(!GridSettings::default().subs_enabled);
    }

    #[test]
    fn resolve_peg_top_left_grid_position() {
        let grid = GridSettings {
            scale_anchor: ScaleAnchor::TopLeft,
            spacing: 200.0,
            radius: 60.0,
            catch: 6.0,
            sub_radius: 24.0,
            subs_enabled: true,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };
        let peg = resolve_peg(
            &grid,
            &[],
            Point::new(999.0, 999.0),
            PegAppearance::default(),
            &PegRef::Grid { row: 2, col: 3 },
        )
        .expect("grid peg should resolve");
        assert!((peg.center.x - (110.0 + 3.0 * 200.0)).abs() < 1e-9);
        assert!((peg.center.y - (110.0 + 2.0 * 200.0)).abs() < 1e-9);
    }

    #[test]
    fn resolve_peg_top_left_sub_position() {
        let grid = GridSettings {
            scale_anchor: ScaleAnchor::TopLeft,
            spacing: 180.0,
            radius: 40.0,
            catch: 4.0,
            sub_radius: 24.0,
            subs_enabled: true,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };
        let peg = resolve_peg(
            &grid,
            &[],
            Point::new(0.0, 0.0),
            PegAppearance::default(),
            &PegRef::Sub { row: 1, col: 1 },
        )
        .expect("sub peg should resolve");
        assert!((peg.center.x - (110.0 + 1.5 * 180.0)).abs() < 1e-9);
        assert!((peg.center.y - (110.0 + 1.5 * 180.0)).abs() < 1e-9);
    }

    #[test]
    fn resolve_peg_sub_when_disabled() {
        let grid = GridSettings {
            scale_anchor: ScaleAnchor::TopLeft,
            spacing: 200.0,
            radius: 60.0,
            catch: 6.0,
            sub_radius: 24.0,
            subs_enabled: false,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };
        assert!(resolve_peg(
            &grid,
            &[],
            Point::new(0.0, 0.0),
            PegAppearance::default(),
            &PegRef::Sub { row: 0, col: 0 }
        )
        .is_some());
    }

    #[test]
    fn resolve_peg_center_anchor_keeps_reference_cell_position_when_spacing_changes() {
        let canvas_center = Point::new(500.0, 400.0);
        let small_grid = GridSettings {
            scale_anchor: ScaleAnchor::Center,
            spacing: 160.0,
            radius: 48.0,
            catch: 5.0,
            sub_radius: 22.0,
            subs_enabled: true,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };
        let large_grid = GridSettings {
            scale_anchor: ScaleAnchor::Center,
            spacing: 240.0,
            radius: 48.0,
            catch: 5.0,
            sub_radius: 22.0,
            subs_enabled: true,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };
        let first = resolve_peg(
            &small_grid,
            &[],
            canvas_center,
            PegAppearance::default(),
            &PegRef::Grid { row: 2, col: 3 },
        )
        .expect("reference grid peg should resolve");
        let second = resolve_peg(
            &large_grid,
            &[],
            canvas_center,
            PegAppearance::default(),
            &PegRef::Grid { row: 2, col: 3 },
        )
        .expect("reference grid peg should resolve");
        assert!((first.center.x - second.center.x).abs() < 1e-9);
        assert!((first.center.y - second.center.y).abs() < 1e-9);
    }

    #[test]
    fn convert_grid_face_refs_to_custom_keeps_area() {
        let canvas_center = Point::new(512.0, 384.0);
        let grid = GridSettings {
            scale_anchor: ScaleAnchor::TopLeft,
            spacing: 200.0,
            radius: 60.0,
            catch: 6.0,
            sub_radius: 24.0,
            subs_enabled: true,
            lines_enabled: true,
            snap_to_lines: true,
            pegs_enabled: true,
            scale_link: false,
        };

        let customs = vec![
            CustomPeg {
                id: 42,
                center: Point::new(110.0, 110.0),
                radius: 60.0,
                visible: true,
            },
            CustomPeg {
                id: 43,
                center: Point::new(310.0, 110.0),
                radius: 60.0,
                visible: true,
            },
        ];

        let face = Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            elements: vec![Element::Wrap {
                peg: PegRef::Grid { row: 0, col: 1 },
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: std::f64::consts::PI,
            }],
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };

        let grid_resolver =
            |r: &PegRef| resolve_peg(&grid, &[], canvas_center, PegAppearance::default(), r);
        let custom_resolver =
            |r: &PegRef| resolve_peg(&grid, &customs, canvas_center, PegAppearance::default(), r);

        let mut grid_pts = Vec::new();
        let mut custom_pts = Vec::new();
        let mut custom_face = face.clone();
        custom_face.start.peg = PegRef::Custom { id: 42 };
        for el in &mut custom_face.elements {
            if let Element::Wrap { peg, .. } = el {
                *peg = PegRef::Custom { id: 43 };
            }
        }

        let grid_face = materialize_face(&face, &grid_resolver).expect("grid face materializes");
        let custom_face =
            materialize_face(&custom_face, &custom_resolver).expect("custom face materializes");
        flatten_segs(&grid_face, &mut grid_pts);
        flatten_segs(&custom_face, &mut custom_pts);

        assert_eq!(grid_pts.len(), custom_pts.len());
        assert!((signed_area(&grid_pts) - signed_area(&custom_pts)).abs() < 1e-9);
    }

    #[test]
    fn ignored_pegs_survive_document_roundtrip() {
        let mut doc = Document::default();
        doc.faces.push(Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            elements: vec![Element::Pin {
                pos: Point::new(200.0, 100.0),
            }],
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: vec![PegRef::Sub { row: 0, col: 0 }],
        });
        let json = serde_json::to_string(&doc).unwrap();
        let restored: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.version, CURRENT_DOCUMENT_VERSION);
        assert_eq!(restored.faces[0].ignored_pegs, doc.faces[0].ignored_pegs);
    }

    #[test]
    fn legacy_document_defaults_layer_metadata() {
        let mut value = serde_json::to_value(Document::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("layer_folders");
        object.remove("empty_layers");
        object.remove("next_layer_item_id");
        let restored: Document = serde_json::from_value(value).unwrap();
        assert!(restored.layer_folders.is_empty());
        assert!(restored.empty_layers.is_empty());
        assert_eq!(restored.next_layer_item_id, 0);
    }

    #[test]
    fn removing_face_reindexes_folder_members() {
        let mut document = Document::default();
        document.layer_folders.push(LayerFolder {
            id: 1,
            name: "group".to_owned(),
            visible: true,
            expanded: true,
            face_indices: vec![0, 2, 3],
        });
        document.remove_face_from_layer_metadata(2);
        assert_eq!(document.layer_folders[0].face_indices, vec![0, 2]);
    }
}
