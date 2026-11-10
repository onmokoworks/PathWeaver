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
    /// A newly inserted handle follows the existing profile until first edited.
    /// Older documents omit this field and keep their explicit interpolation knots.
    #[serde(default)]
    pub interpolated: bool,
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
/// End shape of an open path's stroke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cap {
    #[default]
    Butt,
    Round,
    Square,
}
impl Cap {
    pub fn is_butt(&self) -> bool {
        *self == Cap::Butt
    }
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

/// Live text: the characters and their font stay editable. The shape is the
/// glyph outlines laid out by the host (CoreText on the Mac) and stored in
/// `Stroke::contours`, already placed by `matrix` — so fill, outline and the
/// appearance graph work on text like on any closed shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub font: String,
    /// Font size in document units.
    pub size: f64,
    #[serde(default = "default_line_height")]
    pub line_height: f64,
    /// Local text space (origin = top-left of the first line box) → world.
    pub matrix: [f64; 6],
}
fn default_line_height() -> f64 {
    1.2
}

/// A placed bitmap. The pixels live once in `Document::assets[asset]`; the
/// stroke keeps only the reference and where it sits. `contours` holds the
/// placed rectangle (for picking, bounds and snapping).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub asset: String,
    /// Pixel size; local space is 0..width × 0..height.
    pub width: f64,
    pub height: f64,
    pub matrix: [f64; 6],
    #[serde(default = "default_one")]
    pub opacity: f64,
    /// 下絵: a tracing reference — not exported, not picked on the canvas.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reference: bool,
    /// 抜き: black (Unmult — brightness becomes coverage) or white (paper
    /// becomes clear, ink stays) keyed out. The pixels in `assets` stay as
    /// they were; the web side derives the keyed copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knockout: Option<Knockout>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Knockout {
    Black,
    White,
}
impl Knockout {
    pub fn key(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::White => "white",
        }
    }
}
/// Key of the keyed copy of `asset` handed in for export (`asset#black`).
pub fn knockout_asset(asset: &str, k: Knockout) -> String {
    format!("{}#{}", asset, k.key())
}
fn default_one() -> f64 {
    1.
}
impl Placed {
    pub fn rect(&self) -> Vec<Point> {
        let m = kurbo::Affine::new(self.matrix);
        [(0., 0.), (self.width, 0.), (self.width, self.height), (0., self.height)]
            .into_iter()
            .map(|(x, y)| m * Point::new(x, y))
            .collect()
    }
}

/// A live blend (Illustrator's Blend): the steps between two source paths
/// are generated paths (`Stroke::blend_of`), rebuilt whenever a source changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Blend {
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub steps: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    /// Generated step of the blend with this id (not edited directly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_of: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Text>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<Placed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<super::appearance::Graph>,
    /// A guide made from this path (Illustrator's Make Guides): drawn as a thin
    /// line, snapped to, never painted or exported.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guide: bool,
    /// Layer folder (`Document::folders`), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<u64>,
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
    #[serde(default, skip_serializing_if = "Cap::is_butt")]
    pub start_cap: Cap,
    #[serde(default, skip_serializing_if = "Cap::is_butt")]
    pub end_cap: Cap,
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
        if let Some(t) = &mut result.text {
            t.matrix = (matrix * kurbo::Affine::new(t.matrix)).as_coeffs();
        }
        if let Some(i) = &mut result.image {
            i.matrix = (matrix * kurbo::Affine::new(i.matrix)).as_coeffs();
        }
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
            if matches!(target, WidthTarget::Key(id) if self.widths.iter().any(|k| k.id == id && k.interpolated)) {
                continue;
            }
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
            blend_of: None,
            text: None,
            image: None,
            appearance: None,
            guide: false,
            folder: None,
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
            start_cap: Cap::Butt,
            end_cap: Cap::Butt,
            start_cap_locked: false,
            end_cap_locked: false,
        }
    }
    pub fn width_mut(&mut self, target: WidthTarget) -> Option<&mut Width> {
        // Resolve a sampled handle before making it an explicit knot. This also
        // accounts for other width edits performed since the handle was inserted.
        if let WidthTarget::Key(id) = target {
            let index = self.widths.iter().position(|k| k.id == id)?;
            if self.widths[index].interpolated {
                let width = self.width(target)?;
                self.widths[index].width = width;
                self.widths[index].interpolated = false;
            }
        }
        match target {
            WidthTarget::Start => Some(&mut self.start_width),
            WidthTarget::End => Some(&mut self.end_width),
            WidthTarget::Key(id) => self.widths.iter_mut().find(|k| k.id == id).map(|k| &mut k.width),
        }
    }
    pub fn width(&self, target: WidthTarget) -> Option<Width> {
        match target {
            WidthTarget::Start => Some(self.start_width),
            WidthTarget::End => Some(self.end_width),
            WidthTarget::Key(id) => {
                let key = self.widths.iter().find(|k| k.id == id)?;
                if key.interpolated {
                    super::outline::width_at(self, self.segment_index(key.segment)?, key.t)
                } else {
                    Some(key.width)
                }
            }
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
    /// The displayed stroke width is the widest part of the editable profile.
    /// Using the maximum (rather than the start) also supports tapered zero tips.
    pub fn line_width(&self) -> f64 {
        if self.contours.is_some() {
            return self.edge_width;
        }
        let end = if self.closed { self.start_width } else { self.end_width };
        std::iter::once(self.start_width)
            .chain(std::iter::once(end))
            .chain(self.widths.iter().filter(|k| !k.interpolated).map(|k| k.width))
            .map(|w| w.left + w.right)
            .fold(0., f64::max)
    }
    /// Resize without flattening the profile, moving its centerline, or losing
    /// left/right asymmetry. Only an entirely zero profile needs initialization.
    pub fn resize_line_width(&mut self, value: f64) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=1000.0).contains(&value) {
            return Err("線幅は0〜1000の範囲で入力してください".into());
        }
        if self.contours.is_some() {
            self.edge_width = value;
            return Ok(());
        }
        let current = self.line_width();
        if current <= f64::EPSILON {
            self.set_line_width(value);
            return Ok(());
        }
        let factor = value / current;
        let key_widths: Vec<_> = self.widths.iter().map(|k| {
            self.width(WidthTarget::Key(k.id)).unwrap_or(k.width)
        }).collect();
        // Validate before mutation: an inactive end on a closed path can be
        // wider than the visible profile and must not be silently clipped.
        if std::iter::once(self.start_width)
            .chain(std::iter::once(self.end_width))
            .chain(key_widths.iter().copied())
            .any(|w| (w.left * factor).max(w.right * factor) > 1000.)
        {
            return Err("幅点が上限1000を超えるため、線幅を小さくしてください".into());
        }
        self.edge_width = value;
        for (key, width) in self.widths.iter_mut().zip(key_widths) {
            key.width = width;
        }
        for w in std::iter::once(&mut self.start_width)
            .chain(std::iter::once(&mut self.end_width))
            .chain(self.widths.iter_mut().map(|k| &mut k.width))
        {
            w.left *= factor;
            w.right *= factor;
        }
        Ok(())
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
        // Standard Bézier semantics (as in Illustrator, SVG and fonts): a
        // collapsed handle puts the control point on the anchor. A corner anchor
        // between two curves therefore stays a visible corner; an earlier
        // chord-aligned substitute made collinear corners look smooth and
        // changed how the neighbouring curves bulged.
        let (out, incoming) = (a.outgoing, b.incoming);
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
        // Collapsed handle at an end: the limit direction comes from the next
        // distinct control point (p2 - p0 at the start, p3 - p1 at the end).
        let limit = if t < 0.5 {
            [c.p1 - c.p0, c.p2 - c.p0, c.p3 - c.p0]
        } else {
            [c.p3 - c.p2, c.p3 - c.p1, c.p3 - c.p0]
        };
        if let Some(d) = limit.into_iter().find(|d| d.hypot() > 1e-8) {
            return d / d.hypot();
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

/// A layer folder. Paths reference it by id; the stacking order is still the
/// order of `Document::strokes`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: u64,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub open: bool,
    /// Enclosing folder (folders nest); `None` = top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
}
fn default_true() -> bool {
    true
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
    pub snap_tangent: bool,
    #[serde(default)]
    pub snap_normal: bool,
    #[serde(default)]
    pub guides: Vec<Guide>,
    pub format: String,
    pub version: u32,
    pub next_id: u64,
    #[serde(default = "default_angle_step")]
    pub angle_step: f64,
    #[serde(default)]
    pub appearance_presets: Vec<super::appearance_assets::Preset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<Folder>,
    /// Placed images by key (data URLs). The web side leaves this out of most
    /// core calls, so a big picture is not copied on every edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blends: Vec<Blend>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub assets: std::collections::BTreeMap<String, String>,
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
            snap_tangent: false,
            snap_normal: false,
            guides: vec![],
            format: "pathweaver".into(),
            version: 3,
            next_id: 1,
            angle_step: default_angle_step(),
            appearance_presets: vec![],
            folders: vec![],
            blends: vec![],
            assets: Default::default(),
            strokes: vec![],
        }
    }
}
impl Document {
    /// Visible on the canvas and in exports: the path and its folder are shown.
    pub fn shown(&self, s: &Stroke) -> bool {
        // The path and every enclosing folder must be visible.
        let mut at = s.folder;
        let mut depth = 0;
        while let Some(id) = at {
            let Some(f) = self.folders.iter().find(|x| x.id == id) else { break };
            if !f.visible {
                return false;
            }
            at = f.parent;
            depth += 1;
            if depth > 64 {
                break;
            }
        }
        s.visible
    }
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
        if self.folders.len() > 1000 {
            return Err("フォルダが多すぎます".into());
        }
        for (i, f) in self.folders.iter().enumerate() {
            if self.folders[..i].iter().any(|g| g.id == f.id) || self.strokes.iter().any(|s| s.id == f.id) {
                return Err("フォルダのIDが重複しています".into());
            }
            if f.id >= self.next_id {
                return Err("フォルダのIDが不正です".into());
            }
        }
        for f in &self.folders {
            // parents exist and never loop back
            let mut at = f.parent;
            let mut depth = 0;
            while let Some(id) = at {
                if id == f.id || depth > 64 {
                    return Err("フォルダの入れ子が循環しています".into());
                }
                at = self.folders.iter().find(|x| x.id == id).ok_or("存在しない親フォルダがあります")?.parent;
                depth += 1;
            }
        }
        if self.strokes.iter().any(|s| s.folder.is_some_and(|f| !self.folders.iter().any(|x| x.id == f))) {
            return Err("存在しないフォルダに属するパスがあります".into());
        }
        if self.appearance_presets.len() > 32 { return Err("ノードセットは32個までです".into()); }
        for preset in &self.appearance_presets { preset.validate()?; }
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
        if self.blends.len() > 1000 || self.blends.iter().any(|b| b.steps == 0 || b.steps > 1000) {
            return Err("ブレンドが不正です".into());
        }
        if self.strokes.len() > 1000 {
            return Err("パスが多すぎます".into());
        }
        for s in &self.strokes {
            check_id(s.id)?;
            if let Some(graph) = &s.appearance { graph.validate()?; }
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
            let bad_matrix = |m: &[f64; 6]| m.iter().any(|v| !v.is_finite() || v.abs() > 1e7);
            if let Some(t) = &s.text {
                if t.text.chars().count() > 10000
                    || t.font.chars().count() > 200
                    || !t.size.is_finite()
                    || !(0.1..=10000.0).contains(&t.size)
                    || !t.line_height.is_finite()
                    || !(0.1..=10.0).contains(&t.line_height)
                    || bad_matrix(&t.matrix)
                {
                    return Err("テキストが不正です".into());
                }
            }
            if let Some(i) = &s.image {
                if i.asset.is_empty()
                    || i.asset.len() > 64
                    || !(i.width.is_finite() && i.height.is_finite() && i.width > 0. && i.height > 0.)
                    || !(0.0..=1.0).contains(&i.opacity)
                    || bad_matrix(&i.matrix)
                {
                    return Err("配置画像が不正です".into());
                }
                if !self.assets.is_empty() && !self.assets.contains_key(&i.asset) {
                    return Err("配置画像のデータがありません".into());
                }
            }
            if self.assets.values().any(|v| !v.starts_with("data:image/")) {
                return Err("画像データが不正です".into());
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
                interpolated: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    fn doc_with_nested() -> Document {
        let mut d = Document::example();
        let outer = d.id();
        let inner = d.id();
        d.folders = vec![
            Folder { id: outer, name: "外".into(), visible: true, open: true, parent: None },
            Folder { id: inner, name: "内".into(), visible: true, open: true, parent: Some(outer) },
        ];
        d.strokes[0].folder = Some(inner);
        d
    }
    #[test]
    fn nested_folders_hide_their_contents_and_round_trip() {
        let mut d = doc_with_nested();
        assert!(d.validate().is_ok());
        assert!(d.shown(&d.strokes[0]));
        d.folders[0].visible = false; // hiding the outer folder hides the inner one's paths
        assert!(!d.shown(&d.strokes[0]));
        assert!(!crate::outline::svg(&d).unwrap().contains("<path") || d.strokes.len() > 1);
        let json = serde_json::to_string(&d).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.folders, d.folders);
    }
    #[test]
    fn folder_loops_and_missing_parents_are_rejected() {
        let mut d = doc_with_nested();
        let (a, b) = (d.folders[0].id, d.folders[1].id);
        d.folders[0].parent = Some(b);
        assert!(d.validate().unwrap_err().contains("循環"));
        d.folders[0].parent = Some(a);
        assert!(d.validate().is_err());
        d.folders[0].parent = Some(9_999_999);
        assert!(d.validate().is_err());
    }
}
