use crate::geometry::Peg;
use kurbo::Point;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PegRef {
    Grid { row: i32, col: i32 },
    Sub { row: i32, col: i32 },
    Custom { id: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    Pin { pos: Point },
}

/// マージ（パス結合）で焼き込まれた形状。ユニオン結果のリング群を保持し、
/// ベベル量は描画時に適用する（スライダーで非破壊に変えられる）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BakedShape {
    /// ユニオン結果のリング列（外周＋穴、NonZero）
    pub rings: Vec<Vec<Point>>,
    /// ベベル（面取り）量。0 なら角のまま
    pub bevel: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridSettings {
    pub spacing: f64,
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
    fn default() -> Self { Self::Circle }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PegAppearance {
    #[serde(default)]
    pub shape: PegShape,
    #[serde(default = "default_peg_rounding")]
    pub rounding: f64,
}

fn default_peg_rounding() -> f64 { 12.0 }

impl Default for PegAppearance {
    fn default() -> Self { Self { shape: PegShape::Circle, rounding: default_peg_rounding() } }
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            spacing: 200.0,
            radius: 60.0,
            catch: 6.0,
            sub_radius: 24.0,
            subs_enabled: false,
            scale_link: false,
            scale_anchor: ScaleAnchor::TopLeft,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

impl Default for Document {
    fn default() -> Self {
        Document {
            version: 4,
            grid: GridSettings::default(),
            custom_pegs: Vec::new(),
            next_peg_id: 0,
            faces: Vec::new(),
            peg_appearance: PegAppearance::default(),
            disabled_pegs: Vec::new(),
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
            center: Point::new(110.0 + (col as f64) * grid.spacing, 110.0 + (row as f64) * grid.spacing),
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
        (_, PegRef::Custom { id }) => customs
            .iter()
            .find(|peg| peg.id == id)
            .map(|peg| Peg {
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
            scale_link: false,
        };
        assert!(resolve_peg(&grid, &[], Point::new(0.0, 0.0), PegAppearance::default(), &PegRef::Sub { row: 0, col: 0 }).is_some());
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
            scale_link: false,
        };
        let large_grid = GridSettings {
            scale_anchor: ScaleAnchor::Center,
            spacing: 240.0,
            radius: 48.0,
            catch: 5.0,
            sub_radius: 22.0,
            subs_enabled: true,
            scale_link: false,
        };
        let first =
            resolve_peg(&small_grid, &[], canvas_center, PegAppearance::default(), &PegRef::Grid { row: 2, col: 3 })
                .expect("reference grid peg should resolve");
        let second =
            resolve_peg(&large_grid, &[], canvas_center, PegAppearance::default(), &PegRef::Grid { row: 2, col: 3 })
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
        name: String::new(), baked: None,
        };

        let grid_resolver = |r: &PegRef| resolve_peg(&grid, &[], canvas_center, PegAppearance::default(), r);
        let custom_resolver = |r: &PegRef| resolve_peg(&grid, &customs, canvas_center, PegAppearance::default(), r);

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
        let custom_face = materialize_face(&custom_face, &custom_resolver)
            .expect("custom face materializes");
        flatten_segs(&grid_face, &mut grid_pts);
        flatten_segs(&custom_face, &mut custom_pts);

        assert_eq!(grid_pts.len(), custom_pts.len());
        assert!((signed_area(&grid_pts) - signed_area(&custom_pts)).abs() < 1e-9);
    }
}
