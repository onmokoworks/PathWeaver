//! Gradients: a paint that varies across the shape. Stored in document
//! coordinates as a matrix from the gradient's own space, so it moves,
//! scales and rotates with its path (`Stroke::transformed`).
//!
//! Gradient space: a linear gradient runs from (0, 0) to (1, 0); a radial
//! one is the unit circle around (0, 0). `matrix` places that space in the
//! document, the same way SVG's `gradientTransform` does with
//! `gradientUnits="userSpaceOnUse"`.
use kurbo::{Affine, Point, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

fn default_one() -> f64 {
    1.
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position along the gradient, 0..1.
    pub offset: f64,
    pub color: [u8; 3],
    #[serde(default = "default_one")]
    pub opacity: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
    /// Gradient space → document.
    pub matrix: [f64; 6],
}

pub const MAX_STOPS: usize = 64;

impl Gradient {
    /// Linear gradient from `a` to `b` (the perpendicular keeps the same length).
    pub fn linear(a: Point, b: Point, stops: Vec<GradientStop>) -> Self {
        let d = b - a;
        Self { kind: GradientKind::Linear, stops, matrix: [d.x, d.y, -d.y, d.x, a.x, a.y] }
    }
    /// Circular gradient around `center` with `radius`.
    pub fn radial(center: Point, radius: f64, stops: Vec<GradientStop>) -> Self {
        Self { kind: GradientKind::Radial, stops, matrix: [radius, 0., 0., radius, center.x, center.y] }
    }
    pub fn affine(&self) -> Affine {
        Affine::new(self.matrix)
    }
    pub fn transformed(&self, m: Affine) -> Self {
        Self { matrix: (m * self.affine()).as_coeffs(), ..self.clone() }
    }
    /// Start (linear) or centre (radial) in the document.
    pub fn start(&self) -> Point {
        self.affine() * Point::ZERO
    }
    /// End (linear) or a point on the rim (radial) in the document.
    pub fn end(&self) -> Point {
        self.affine() * Point::new(1., 0.)
    }
    pub fn validate(&self) -> Result<(), String> {
        let m = self.matrix;
        let det = m[0] * m[3] - m[1] * m[2];
        if self.stops.is_empty()
            || self.stops.len() > MAX_STOPS
            || self.stops.iter().any(|s| {
                !s.offset.is_finite() || !(0.0..=1.0).contains(&s.offset) || !s.opacity.is_finite() || !(0.0..=1.0).contains(&s.opacity)
            })
            || m.iter().any(|v| !v.is_finite() || v.abs() > 1e7)
            || !det.is_finite()
            || det.abs() < 1e-12
        {
            return Err("グラデーションが不正です".into());
        }
        Ok(())
    }
    /// Stops in order of position (ties keep their order, like SVG).
    pub fn sorted_stops(&self) -> Vec<GradientStop> {
        let mut s = self.stops.clone();
        s.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        s
    }
    /// Position (0..1, padded at both ends) of a document point.
    pub fn position(&self, p: Point) -> f64 {
        let q = self.affine().inverse() * p;
        let t = match self.kind {
            GradientKind::Linear => q.x,
            GradientKind::Radial => q.to_vec2().hypot(),
        };
        if t.is_finite() { t.clamp(0., 1.) } else { 0. }
    }
    /// Colour (0..1) and opacity at gradient position `t`, straight (not premultiplied).
    pub fn at(&self, t: f64) -> ([f32; 3], f32) {
        let stops = self.sorted_stops();
        let rgb = |s: &GradientStop| s.color.map(|c| c as f32 / 255.);
        let Some(first) = stops.first() else { return ([0.; 3], 0.) };
        if t <= first.offset {
            return (rgb(first), first.opacity as f32);
        }
        for w in stops.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if t <= b.offset {
                let k = if b.offset - a.offset > 1e-12 { ((t - a.offset) / (b.offset - a.offset)) as f32 } else { 1. };
                let (ca, cb) = (rgb(a), rgb(b));
                return (
                    [0, 1, 2].map(|i| ca[i] + (cb[i] - ca[i]) * k),
                    a.opacity as f32 + (b.opacity - a.opacity) as f32 * k,
                );
            }
        }
        let last = stops.last().unwrap();
        (rgb(last), last.opacity as f32)
    }
    pub fn sample(&self, p: Point) -> ([f32; 3], f32) {
        self.at(self.position(p))
    }
    /// The colour a solid paint falls back to (the first stop).
    pub fn first_color(&self) -> [u8; 3] {
        self.sorted_stops().first().map(|s| s.color).unwrap_or([0; 3])
    }
    /// Angle (degrees) of the gradient's axis in the document.
    pub fn angle(&self) -> f64 {
        let d: Vec2 = self.end() - self.start();
        d.y.atan2(d.x).to_degrees()
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The SVG element for `g` with this `id` (`fill="url(#id)"` paints with it).
/// Used by the SVG export; the canvas builds the same element in Vue.
pub fn gradient_svg(g: &Gradient, id: &str) -> String {
    let m = g.matrix;
    let head = match g.kind {
        GradientKind::Linear => format!("<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\""),
        GradientKind::Radial => format!("<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"1\""),
    };
    let mut out = format!(
        "{head} gradientTransform=\"matrix({:.6} {:.6} {:.6} {:.6} {:.4} {:.4})\">",
        m[0], m[1], m[2], m[3], m[4], m[5]
    );
    for s in g.sorted_stops() {
        out.push_str(&format!(
            "<stop offset=\"{:.6}\" stop-color=\"{}\" stop-opacity=\"{:.6}\"/>",
            s.offset.clamp(0., 1.),
            hex(s.color),
            s.opacity.clamp(0., 1.)
        ));
    }
    out.push_str(match g.kind {
        GradientKind::Linear => "</linearGradient>",
        GradientKind::Radial => "</radialGradient>",
    });
    out
}

/// `fill` and `fill-opacity` attributes for a layer, plus the gradient
/// element it needs (empty for a solid colour). `id` must be unique in the file.
pub fn fill_attrs(color: [u8; 3], opacity: f64, gradient: Option<&Gradient>, id: &str) -> (String, String) {
    let o = opacity.clamp(0., 1.);
    match gradient {
        Some(g) => (gradient_svg(g, id), format!("fill=\"url(#{id})\" fill-opacity=\"{o:.6}\"")),
        None => (String::new(), format!("fill=\"{}\" fill-opacity=\"{o:.6}\"", hex(color))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bw() -> Vec<GradientStop> {
        vec![
            GradientStop { offset: 0., color: [0, 0, 0], opacity: 1. },
            GradientStop { offset: 1., color: [255, 255, 255], opacity: 0.5 },
        ]
    }

    #[test]
    fn linear_samples_along_its_axis_and_pads() {
        let g = Gradient::linear(Point::new(10., 0.), Point::new(110., 0.), bw());
        assert_eq!(g.sample(Point::new(0., 50.)).0, [0.; 3]);
        let (c, a) = g.sample(Point::new(60., -30.));
        assert!((c[0] - 0.5).abs() < 1e-3 && (a - 0.75).abs() < 1e-3);
        assert_eq!(g.sample(Point::new(500., 0.)).0, [1.; 3]);
        assert!((g.angle()).abs() < 1e-9);
    }

    #[test]
    fn radial_samples_by_distance_and_follows_transforms() {
        let g = Gradient::radial(Point::new(0., 0.), 10., bw());
        assert!((g.position(Point::new(0., 5.)) - 0.5).abs() < 1e-9);
        let moved = g.transformed(Affine::translate((100., 0.)) * Affine::scale(2.));
        assert!((moved.position(Point::new(110., 0.)) - 0.5).abs() < 1e-9);
        assert_eq!(moved.start(), Point::new(100., 0.));
    }

    #[test]
    fn svg_lists_sorted_stops() {
        let mut g = Gradient::linear(Point::ZERO, Point::new(1., 0.), bw());
        g.stops.reverse();
        let s = gradient_svg(&g, "g1");
        assert!(s.starts_with("<linearGradient id=\"g1\""));
        assert!(s.find("#000000").unwrap() < s.find("#ffffff").unwrap());
        let (defs, attrs) = fill_attrs([1, 2, 3], 0.5, None, "x");
        assert!(defs.is_empty() && attrs.contains("#010203"));
    }

    #[test]
    fn degenerate_gradients_are_refused() {
        let g = Gradient::linear(Point::ZERO, Point::ZERO, bw());
        assert!(g.validate().is_err());
        let mut g = Gradient::linear(Point::ZERO, Point::new(1., 0.), bw());
        g.stops[0].offset = 2.;
        assert!(g.validate().is_err());
    }
}
