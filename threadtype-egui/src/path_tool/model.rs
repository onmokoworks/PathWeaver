use kurbo::{Point, Vec2};
use serde::{Deserialize, Serialize};
fn default_fill() -> Option<[u8; 3]> {
    Some([224, 224, 224])
}
fn default_edge_width() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Preserve a zero derivative created by exact curve subdivision.
    #[serde(default)]
    pub exact_incoming: bool,
    #[serde(default)]
    pub exact_outgoing: bool,
    pub id: u64,
    pub p: Point,
    pub incoming: Vec2,
    pub outgoing: Vec2,
    pub smooth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Width {
    pub left: f64,
    pub right: f64,
    pub linked: bool,
    #[serde(default)]
    pub angle: f64,
}
impl Default for Width {
    fn default() -> Self {
        Self {
            left: 24.0,
            right: 24.0,
            linked: true,
            angle: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WidthKey {
    pub id: u64,
    pub segment: u64,
    pub t: f64,
    pub width: Width,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidthTarget {
    Start,
    End,
    Key(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Join {
    #[default]
    Miter,
    Round,
    Bevel,
}
fn default_miter_limit() -> f64 {
    10.0
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CornerKind {
    #[default]
    Circular,
    Clothoid,
    Bevel,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    pub node: u64,
    pub radius: f64,
    pub kind: CornerKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    #[serde(default)]
    pub closed: bool,
    /// Closed, normalized contours after expansion or a Boolean operation.
    #[serde(default)]
    pub contours: Option<Vec<Vec<Point>>>,
    #[serde(default = "default_fill")]
    pub fill: Option<[u8; 3]>,
    #[serde(default)]
    pub edge: Option<[u8; 3]>,
    #[serde(default = "default_edge_width")]
    pub edge_width: f64,
    #[serde(default)]
    pub frame_angle: f64,
    #[serde(default)]
    pub corners: Vec<Corner>,
    #[serde(default)]
    pub join: Join,
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f64,
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub nodes: Vec<Node>,
    pub widths: Vec<WidthKey>,
    pub start_width: Width,
    pub end_width: Width,
    /// World angle of the cut line: 0 horizontal, 90 vertical.
    pub start_angle: f64,
    pub end_angle: f64,
    #[serde(default)]
    pub start_cap_locked: bool,
    #[serde(default)]
    pub end_cap_locked: bool,
}
impl Stroke {
    /// Transform the editable centerline and its width vectors together.
    pub fn transformed(&self, matrix: kurbo::Affine, scale_widths: bool) -> Self {
        let vector = |v: Vec2| matrix * v.to_point() - matrix * Point::ZERO;
        let mut result = self.clone();
        if let Some(contours) = &mut result.contours {
            for p in contours.iter_mut().flatten() {
                *p = matrix * *p;
            }
        }
        let axis = vector(Vec2::new(self.frame_angle.cos(), self.frame_angle.sin()));
        result.frame_angle = axis.y.atan2(axis.x);
        let x = vector(Vec2::new(1., 0.));
        let y = vector(Vec2::new(0., 1.));
        let factor = (x.x * y.y - x.y * y.x).abs().sqrt();
        if scale_widths {
            result.edge_width = (result.edge_width * factor).min(10000.);
        }
        for c in &mut result.corners {
            c.radius = (c.radius * factor).min(10000.);
        }
        for n in &mut result.nodes {
            n.p = matrix * n.p;
            n.incoming = vector(n.incoming);
            n.outgoing = vector(n.outgoing);
        }
        for target in self.width_targets() {
            if let Some((i, t)) = self.attachment(target) {
                let old = *self.width(target).as_ref().unwrap();
                let tangent = self.tangent(i, t);
                let a = old.angle.to_radians();
                let d = Vec2::new(-tangent.y, tangent.x) * a.cos() + tangent * a.sin();
                let transformed = vector(d);
                let w = result.width_mut(target).unwrap();
                if scale_widths {
                    w.left = (old.left * transformed.hypot()).clamp(0., 1000.);
                    w.right = (old.right * transformed.hypot()).clamp(0., 1000.);
                    let t = vector(tangent);
                    let t = t / t.hypot().max(1e-12);
                    w.angle = transformed
                        .dot(t)
                        .atan2(transformed.dot(Vec2::new(-t.y, t.x)))
                        .to_degrees();
                }
            }
        }
        for (angle, locked) in [
            (&mut result.start_angle, self.start_cap_locked),
            (&mut result.end_angle, self.end_cap_locked),
        ] {
            if locked {
                let a = angle.to_radians();
                let d = vector(Vec2::new(a.cos(), a.sin()));
                *angle = d.y.atan2(d.x).to_degrees().rem_euclid(180.);
            }
        }
        result
    }
    pub fn new(id: u64, name: String) -> Self {
        Self {
            closed: false,
            contours: None,
            fill: None,
            edge: default_fill(),
            edge_width: default_edge_width(),
            frame_angle: 0.,
            corners: vec![],
            join: Join::Miter,
            miter_limit: default_miter_limit(),
            id,
            name,
            visible: true,
            nodes: vec![],
            widths: vec![],
            start_width: Width::default(),
            end_width: Width::default(),
            start_angle: 90.0,
            end_angle: 90.0,
            start_cap_locked: false,
            end_cap_locked: false,
        }
    }
    pub fn width_mut(&mut self, target: WidthTarget) -> Option<&mut Width> {
        match target {
            WidthTarget::Start => Some(&mut self.start_width),
            WidthTarget::End => Some(&mut self.end_width),
            WidthTarget::Key(id) => self
                .widths
                .iter_mut()
                .find(|k| k.id == id)
                .map(|k| &mut k.width),
        }
    }
    pub fn width(&self, target: WidthTarget) -> Option<Width> {
        match target {
            WidthTarget::Start => Some(self.start_width),
            WidthTarget::End => Some(self.end_width),
            WidthTarget::Key(id) => self.widths.iter().find(|k| k.id == id).map(|k| k.width),
        }
    }
    pub fn segment_count(&self) -> usize {
        if self.closed && self.nodes.len() > 1 {
            self.nodes.len()
        } else {
            self.nodes.len().saturating_sub(1)
        }
    }
    pub fn set_line_width(&mut self, value: f64) {
        self.edge_width = value;
        self.start_width.left = value * 0.5;
        self.start_width.right = value * 0.5;
        self.end_width.left = value * 0.5;
        self.end_width.right = value * 0.5;
        for k in &mut self.widths {
            k.width.left = value * 0.5;
            k.width.right = value * 0.5;
        }
    }
    pub fn segment_index(&self, id: u64) -> Option<usize> {
        self.nodes
            .iter()
            .take(self.segment_count())
            .position(|n| n.id == id)
    }
    pub fn cubic(&self, i: usize) -> kurbo::CubicBez {
        let a = &self.nodes[i];
        let b = &self.nodes[(i + 1) % self.nodes.len()];
        let chord = b.p - a.p;
        let length = chord.hypot();
        // A collapsed handle is a corner with a chord-aligned departure,
        // not a zero derivative whose direction comes from the remote handle.
        // Ramp the implicit length from zero so starting a pen drag is continuous.
        // Keep stored handles collapsed; only explicit edits create UI handles.
        let implicit = |other: Vec2| {
            if length > 1e-8 {
                chord * (other.hypot().min(length / 3.) / length)
            } else {
                Vec2::ZERO
            }
        };
        let out = if !a.exact_outgoing && a.outgoing.hypot() <= 1e-8 {
            implicit(b.incoming)
        } else {
            a.outgoing
        };
        let incoming = if !b.exact_incoming && b.incoming.hypot() <= 1e-8 {
            -implicit(a.outgoing)
        } else {
            b.incoming
        };
        kurbo::CubicBez::new(a.p, a.p + out, b.p + incoming, b.p)
    }
    pub fn eval(&self, i: usize, t: f64) -> Point {
        let c = self.cubic(i);
        let u = 1.0 - t;
        let v = c.p0.to_vec2() * (u * u * u)
            + c.p1.to_vec2() * (3.0 * u * u * t)
            + c.p2.to_vec2() * (3.0 * u * t * t)
            + c.p3.to_vec2() * (t * t * t);
        v.to_point()
    }
    pub fn tangent(&self, i: usize, t: f64) -> Vec2 {
        let c = self.cubic(i);
        let u = 1.0 - t;
        let v = (c.p1 - c.p0) * (3.0 * u * u)
            + (c.p2 - c.p1) * (6.0 * u * t)
            + (c.p3 - c.p2) * (3.0 * t * t);
        if v.hypot() > 1e-8 {
            return v / v.hypot();
        }
        let d = self.eval(i, (t + 1e-4).min(1.0)) - self.eval(i, (t - 1e-4).max(0.0));
        if d.hypot() > 1e-8 {
            d / d.hypot()
        } else {
            Vec2::new(1.0, 0.0)
        }
    }
    pub fn attachment(&self, target: WidthTarget) -> Option<(usize, f64)> {
        if self.nodes.len() < 2 {
            return None;
        }
        match target {
            WidthTarget::Start => Some((0, 0.0)),
            WidthTarget::End => Some((self.nodes.len() - 2, 1.0)),
            WidthTarget::Key(id) => {
                let k = self.widths.iter().find(|k| k.id == id)?;
                Some((self.segment_index(k.segment)?, k.t))
            }
        }
    }
    pub fn width_targets(&self) -> Vec<WidthTarget> {
        let mut v = vec![WidthTarget::Start];
        if !self.closed {
            v.push(WidthTarget::End);
        }
        v.extend(self.widths.iter().map(|k| WidthTarget::Key(k.id)));
        v
    }
    /// Exact de Casteljau split. Keys stay at the same physical positions.
    pub fn split(&mut self, i: usize, t: f64, node_id: u64) {
        let a = self.nodes[i].clone();
        let c = self.cubic(i);
        let (p0, p1, p2, p3) = (c.p0, c.p1, c.p2, c.p3);
        let q0 = p0.lerp(p1, t);
        let q1 = p1.lerp(p2, t);
        let q2 = p2.lerp(p3, t);
        let r0 = q0.lerp(q1, t);
        let r1 = q1.lerp(q2, t);
        let p = r0.lerp(r1, t);
        self.nodes[i].outgoing = q0 - p0;
        self.nodes[i].exact_outgoing = true;
        let next = (i + 1) % self.nodes.len();
        self.nodes[next].incoming = q2 - p3;
        self.nodes[next].exact_incoming = true;
        self.nodes.insert(
            i + 1,
            Node {
                exact_incoming: true,
                exact_outgoing: true,
                id: node_id,
                p,
                incoming: r0 - p,
                outgoing: r1 - p,
                smooth: true,
            },
        );
        for k in &mut self.widths {
            if k.segment == a.id {
                if k.t < t {
                    k.t /= t;
                } else {
                    k.segment = node_id;
                    k.t = (k.t - t) / (1.0 - t);
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub vertical: bool,
    pub position: f64,
}
fn default_grid() -> f64 {
    50.
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(default = "default_grid")]
    pub grid_spacing: f64,
    #[serde(default)]
    pub snap_grid: bool,
    #[serde(default)]
    pub snap_guides: bool,
    #[serde(default)]
    pub guides: Vec<Guide>,
    pub format: String,
    pub version: u32,
    pub next_id: u64,
    #[serde(default = "default_angle_step")]
    pub angle_step: f64,
    pub strokes: Vec<Stroke>,
}
fn default_angle_step() -> f64 {
    15.
}
impl Default for Document {
    fn default() -> Self {
        Self {
            grid_spacing: default_grid(),
            snap_grid: false,
            snap_guides: true,
            guides: vec![],
            format: "pathweaver".into(),
            version: 3,
            next_id: 1,
            angle_step: default_angle_step(),
            strokes: vec![],
        }
    }
}
impl Document {
    pub fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn upgrade(&mut self) -> Result<(), String> {
        if self.format != "pathweaver" {
            return Err("プロジェクト形式が異なります".into());
        }
        if self.version == 1 {
            for s in &mut self.strokes {
                if s.nodes.len() >= 2 {
                    for end in [false, true] {
                        let (i, t, old) = if end {
                            (s.nodes.len() - 2, 1., s.end_angle)
                        } else {
                            (0, 0., s.start_angle)
                        };
                        let tangent = s.tangent(i, t);
                        let world =
                            (tangent.y.atan2(tangent.x).to_degrees() + 90. - old).rem_euclid(180.);
                        if end {
                            s.end_angle = world;
                            s.end_cap_locked = true;
                        } else {
                            s.start_angle = world;
                            s.start_cap_locked = true;
                        }
                    }
                } else {
                    s.start_angle = 90.;
                    s.end_angle = 90.;
                }
            }
            self.version = 2;
        }
        if self.version == 2 {
            self.version = 3;
            self.validate()?;
            let mut migrated = Vec::new();
            for source in &self.strokes {
                let mut line = source.clone();
                if source.contours.is_none() {
                    line.fill = None;
                    line.edge = source.fill;
                    if let Some(color) = source.edge {
                        let mut border = source.clone();
                        border.id = self.next_id;
                        self.next_id += 1;
                        border.name.push_str(" / 旧輪郭線");
                        border.contours = Some(super::outline::build(source)?.contours);
                        border.nodes.clear();
                        border.widths.clear();
                        border.corners.clear();
                        border.fill = None;
                        border.edge = Some(color);
                        migrated.push(line);
                        migrated.push(border);
                        continue;
                    }
                }
                migrated.push(line);
            }
            self.strokes = migrated;
            self.version = 3;
        }
        self.validate()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.format != "pathweaver" || self.version != 3 {
            return Err("対応していないプロジェクト形式です".into());
        }
        if !self.angle_step.is_finite() || !(1.0..=90.0).contains(&self.angle_step) {
            return Err("回転のスナップ間隔が不正です".into());
        }
        if !self.grid_spacing.is_finite()
            || !(1.0..=10000.0).contains(&self.grid_spacing)
            || self.guides.len() > 1000
            || self
                .guides
                .iter()
                .any(|g| !g.position.is_finite() || g.position.abs() > 1e6)
        {
            return Err("グリッドまたはガイドが不正です".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut check_id = |id: u64| {
            if id > 0 && id < self.next_id && ids.insert(id) {
                Ok(())
            } else {
                Err("IDが不正です".to_owned())
            }
        };
        let valid_width = |w: Width| {
            w.angle.is_finite()
                && w.angle.abs() <= 360.0
                && w.left.is_finite()
                && w.right.is_finite()
                && (0.0..=1000.0).contains(&w.left)
                && (0.0..=1000.0).contains(&w.right)
        };
        if self.strokes.len() > 1000 {
            return Err("パスが多すぎます".into());
        }
        for s in &self.strokes {
            check_id(s.id)?;
            if !s.edge_width.is_finite() || !(0.0..=10000.0).contains(&s.edge_width) {
                return Err("輪郭線の幅が不正です".into());
            }
            if let Some(contours) = &s.contours {
                if contours.iter().map(Vec::len).sum::<usize>() > 200000
                    || contours.iter().any(|c| {
                        c.len() < 3
                            || c.iter().any(|p| {
                                !p.x.is_finite()
                                    || !p.y.is_finite()
                                    || p.x.abs().max(p.y.abs()) > 1e6
                            })
                    })
                {
                    return Err("閉じた輪郭が不正です".into());
                }
            }
            if !s.frame_angle.is_finite() {
                return Err("変形枠の角度が不正です".into());
            }
            if s.corners.iter().any(|c| {
                !c.radius.is_finite()
                    || !(0.0..=10000.0).contains(&c.radius)
                    || !s.nodes.iter().any(|n| n.id == c.node)
            }) {
                return Err("角の半径が不正です".into());
            }
            if !s.miter_limit.is_finite() || !(1.0..=500.0).contains(&s.miter_limit) {
                return Err("マイター制限が不正です".into());
            }
            if s.nodes.len() > 10000 || s.widths.len() > 10000 {
                return Err("制御点が多すぎます".into());
            }
            if !valid_width(s.start_width) || !valid_width(s.end_width) {
                return Err("端点の幅が不正です".into());
            }
            for angle in [s.start_angle, s.end_angle] {
                if !angle.is_finite() || !(0.0..180.0).contains(&angle) {
                    return Err("切り口の角度が不正です".into());
                }
            }
            for n in &s.nodes {
                check_id(n.id)?;
                if [
                    n.p.x,
                    n.p.y,
                    n.incoming.x,
                    n.incoming.y,
                    n.outgoing.x,
                    n.outgoing.y,
                ]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1e6)
                {
                    return Err("座標が不正です".into());
                }
            }
            for k in &s.widths {
                check_id(k.id)?;
                if s.segment_index(k.segment).is_none()
                    || !k.t.is_finite()
                    || !(0.0..=1.0).contains(&k.t)
                    || !valid_width(k.width)
                {
                    return Err("幅コントローラーが不正です".into());
                }
            }
        }
        Ok(())
    }
    pub fn example() -> Self {
        let mut d = Self::default();
        let mut s = Stroke::new(d.id(), "01 / Ribbon".into());
        for (x, y, ix, iy, ox, oy) in [
            (160., 370., 0., 0., 130., -240.),
            (410., 280., -130., -180., 140., 180.),
            (700., 260., -140., 240., 0., 0.),
        ] {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: d.id(),
                p: Point::new(x, y),
                incoming: Vec2::new(ix, iy),
                outgoing: Vec2::new(ox, oy),
                smooth: true,
            });
        }
        s.start_width = Width {
            left: 12.,
            right: 12.,
            linked: true,
            angle: 0.0,
        };
        s.end_width = Width {
            left: 16.,
            right: 16.,
            linked: true,
            angle: 0.0,
        };
        for (i, t, left, right) in [(0, 0.48, 48., 48.), (1, 0.48, 66., 35.)] {
            s.widths.push(WidthKey {
                id: d.id(),
                segment: s.nodes[i].id,
                t,
                width: Width {
                    left,
                    right,
                    linked: left == right,
                    angle: 0.0,
                },
            });
        }
        s.start_angle = 0.0;
        s.start_cap_locked = true;
        s.end_angle = 0.0;
        s.end_cap_locked = true;
        d.strokes.push(s);
        d
    }
}
