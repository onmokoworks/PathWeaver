//! Paint effects. Opacity stays vector; blur, glow and soft shadows are real
//! Gaussian blurs on pixels (raster.rs), output as one smooth bitmap each.
use super::{
    appearance::Layer,
    appearance_effects::{offset_contours, OffsetJoin},
    outline::boolean_contours,
};
use i_overlay::core::overlay_rule::OverlayRule;
use kurbo::{Point, Vec2};

const MAX_LAYERS: usize = 64;
const MAX_VERTICES: usize = 200_000;

fn check_layers(layers: &[Layer]) -> Result<(), String> {
    if layers.len() > MAX_LAYERS
        || layers
            .iter()
            .flat_map(|l| &l.contours)
            .map(Vec::len)
            .sum::<usize>()
            > MAX_VERTICES
    {
        return Err("外観が複雑すぎます。ぼかしや重ねるノードを減らしてください".into());
    }
    if layers.iter().any(|l| {
        !l.opacity.is_finite()
            || !(0. ..=1.).contains(&l.opacity)
            || l.contours
                .iter()
                .flatten()
                .any(|p| !p.x.is_finite() || !p.y.is_finite())
    }) {
        return Err("外観レイヤーに不正な値があります".into());
    }
    Ok(())
}
fn check_range(value: f64, min: f64, max: f64) -> Result<(), String> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err("外観効果の設定が範囲外です".into())
    }
}
fn silhouette(layers: &[Layer]) -> Vec<Vec<Point>> {
    let rings = layers
        .iter()
        .filter(|l| l.opacity > 0.)
        .flat_map(|l| l.contours.iter().cloned())
        .collect::<Vec<_>>();
    boolean_contours(&rings, &[], OverlayRule::Subject)
}

/// Multiply existing alpha; RGB values and input geometry remain unchanged.
pub fn opacity(layers: &[Layer], amount: f64) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(amount, 0., 1.)?;
    Ok(layers
        .iter()
        .cloned()
        .map(|mut l| {
            l.opacity *= amount;
            l
        })
        .collect())
}

pub fn blur(layers: &[Layer], radius: f64) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(radius, 0., 100.)?;
    if radius < 1e-6 {
        return Ok(layers.to_vec());
    }
    // A real Gaussian blur on pixels: one smooth bitmap, no stepped bands.
    Ok(super::raster::soft_blur(layers, radius)?.map(|img| vec![img.layer(1.)]).unwrap_or_default())
}

/// The artwork moved by (dx, dy): shapes are offset, bitmaps resampled.
fn moved(layers: &[Layer], d: Vec2) -> Result<Vec<Layer>, String> {
    layers
        .iter()
        .map(|l| {
            let mut l = l.clone();
            for p in l.contours.iter_mut().flatten() {
                *p += d;
            }
            if let Some(img) = &l.image {
                l.image = Some(std::sync::Arc::new(super::raster::transform(img, |p| p + d, |p| p - d)?));
            }
            Ok(l)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn shadow(
    layers: &[Layer],
    dx: f64,
    dy: f64,
    spread: f64,
    color: [u8; 3],
    amount: f64,
    radius: f64,
) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(dx, -100_000., 100_000.)?;
    check_range(dy, -100_000., 100_000.)?;
    check_range(spread, -1000., 1000.)?;
    check_range(amount, 0., 1.)?;
    check_range(radius, 0., 100.)?;
    if amount == 0. {
        return Ok(layers.to_vec());
    }
    let d = Vec2::new(dx, dy);
    // Shapes: their silhouette grown by `spread`; bitmaps keep their alpha.
    let shapes: Vec<Layer> = layers.iter().filter(|l| l.image.is_none()).cloned().collect();
    let mut source = vec![Layer {
        color,
        opacity: 1.,
        contours: offset_contours(&silhouette(&shapes), spread, OffsetJoin::Round)?,
        image: None,
        gradient: None,
    }];
    source.extend(layers.iter().filter(|l| l.image.is_some()).cloned());
    let source = moved(&source, d)?;
    let mut result = if radius < 1e-6 && source.iter().all(|l| l.image.is_none()) {
        vec![Layer { opacity: amount, ..source[0].clone() }]
    } else {
        super::raster::soft_silhouette(&source, radius, color, amount)?.map(|img| vec![img.layer(1.)]).unwrap_or_default()
    };
    result.retain(|l| !l.contours.is_empty());
    result.extend_from_slice(layers);
    check_layers(&result)?;
    Ok(result)
}

/// Which halos a glow draws, and whether the source stays on top.
#[derive(Clone, Copy, Debug)]
pub struct GlowMode {
    pub outer: bool,
    pub inner: bool,
    pub keep_source: bool,
}

/// A real glow: the artwork's alpha blurred (Gaussian) into smooth light.
/// Outer light sits behind the artwork, inner light over it (clipped to it).
pub fn glow(
    layers: &[Layer],
    size: f64,
    color: [u8; 3],
    amount: f64,
    mode: GlowMode,
) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(size, 0., 100.)?;
    check_range(amount, 0., 1.)?;
    let source_layers = || if mode.keep_source { layers.to_vec() } else { Vec::new() };
    if size < 1e-6 || amount == 0. || !(mode.outer || mode.inner) {
        return Ok(source_layers());
    }
    let (outer, inner) = super::raster::glow(layers, size, color, amount, mode.outer, mode.inner, !mode.keep_source)?;
    let mut result = Vec::new();
    result.extend(outer.map(|i| i.layer(1.)));
    result.extend(source_layers());
    result.extend(inner.map(|i| i.layer(1.)));
    check_layers(&result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> Layer {
        Layer {
            color: [100, 150, 200],
            opacity: 0.8,
            contours: vec![vec![
                Point::new(0., 0.),
                Point::new(100., 0.),
                Point::new(100., 100.),
                Point::new(0., 100.),
            ]],
            image: None, gradient: None,
        }
    }
    fn contains(rings: &[Vec<Point>], p: Point) -> bool {
        let mut winding = 0;
        for ring in rings {
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                let cross = (b - a).cross(p - a);
                if a.y <= p.y && b.y > p.y && cross > 0. {
                    winding += 1;
                }
                if a.y > p.y && b.y <= p.y && cross < 0. {
                    winding -= 1;
                }
            }
        }
        winding != 0
    }
    /// Composite alpha at a point: shapes by containment, bitmaps by pixel.
    fn alpha_at(layers: &[Layer], p: Point) -> f64 {
        layers.iter().fold(0., |a, l| {
            let v = match &l.image {
                Some(i) => {
                    let (x, y) = (((p.x - i.x0) * i.scale).floor(), ((p.y - i.y0) * i.scale).floor());
                    if x < 0. || y < 0. || x >= i.width as f64 || y >= i.height as f64 {
                        0.
                    } else {
                        i.rgba[(y as usize * i.width + x as usize) * 4 + 3] as f64 / 255. * l.opacity
                    }
                }
                None if contains(&l.contours, p) => l.opacity,
                None => 0.,
            };
            v + a * (1. - v)
        })
    }
    /// Alpha along y=50 from x0 to x1 never jumps (no visible steps or rings).
    fn assert_smooth(layers: &[Layer], x0: f64, x1: f64, max_step: f64) {
        let mut last = alpha_at(layers, Point::new(x0, 50.));
        let mut x = x0 + 1.;
        while x < x1 {
            let a = alpha_at(layers, Point::new(x, 50.));
            assert!((a - last).abs() < max_step, "x={x} {last} -> {a}");
            last = a;
            x += 1.;
        }
    }
    #[test]
    fn opacity_multiplies_and_preserves_geometry_color() {
        let original = vec![square()];
        let result = opacity(&original, 0.25).unwrap();
        assert_eq!(result[0].opacity, 0.2);
        assert_eq!(result[0].color, original[0].color);
        assert_eq!(result[0].contours, original[0].contours);
        assert_eq!(original[0].opacity, 0.8);
        assert!(opacity(&original, f64::NAN).is_err());
    }
    #[test]
    fn shadow_is_below_source_and_offsets_its_silhouette() {
        let original = square();
        let result = shadow(std::slice::from_ref(&original), 20., 30., 0., [1, 2, 3], 0.6, 0.).unwrap();
        assert_eq!(result.last().unwrap(), &original);
        assert_eq!(result[0].color, [1, 2, 3]);
        assert_eq!(result[0].opacity, 0.6);
        assert!(contains(&result[0].contours, Point::new(115., 125.)));
        assert!(!contains(&result[0].contours, Point::new(5., 5.)));
    }
    #[test]
    fn soft_shadow_is_one_smooth_bitmap_below_the_source() {
        let shadowed = shadow(&[square()], 20., 30., 0., [1, 2, 3], 0.6, 8.).unwrap();
        assert_eq!(shadowed.len(), 2);
        let img = shadowed[0].image.as_ref().unwrap();
        assert!(img.soft);
        assert!(alpha_at(&shadowed[..1], Point::new(70., 80.)) > 0.5);
        assert_smooth(&shadowed[..1], 100., 140., 0.06);
    }
    #[test]
    fn blur_is_a_real_gaussian_on_pixels() {
        let original = square();
        assert_eq!(blur(std::slice::from_ref(&original), 0.).unwrap(), vec![original.clone()]);
        let blurred = blur(&[original], 10.).unwrap();
        assert_eq!(blurred.len(), 1, "one bitmap, not stepped bands");
        let img = blurred[0].image.as_ref().unwrap();
        assert!(img.soft);
        let center = alpha_at(&blurred, Point::new(50., 50.));
        let near = alpha_at(&blurred, Point::new(103., 50.));
        let far = alpha_at(&blurred, Point::new(112., 50.));
        assert!(center > near && near > far && far > 0., "{center} {near} {far}");
        assert!((center - 0.8).abs() < 0.02);
        // colour kept (premultiplied: rgb ≈ colour × alpha)
        let i = ((50. - img.y0) * img.scale) as usize * img.width + ((50. - img.x0) * img.scale) as usize;
        let a = img.rgba[i * 4 + 3] as f64;
        assert!((img.rgba[i * 4] as f64 - 100. * a / 255.).abs() < 3.);
        assert!(blur(&[square()], 101.).is_err());
    }
    #[test]
    fn large_blur_falls_off_smoothly() {
        let blurred = blur(&[square()], 60.).unwrap();
        assert_smooth(&blurred, 50., 200., 0.03);
    }
    #[test]
    fn glow_modes_are_smooth_light_behind_and_inside() {
        let mode = |outer, inner, keep_source| GlowMode { outer, inner, keep_source };
        let original = square();
        // inner: source, then light clipped to the shape
        let inner = glow(std::slice::from_ref(&original), 10., [255, 0, 0], 0.5, mode(false, true, true)).unwrap();
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0], original);
        assert!(alpha_at(&inner[1..], Point::new(98., 50.)) > 0.1);
        assert_eq!(alpha_at(&inner[1..], Point::new(103., 50.)), 0.);
        assert!(alpha_at(&inner[1..], Point::new(98., 50.)) > alpha_at(&inner[1..], Point::new(80., 50.)));
        // outer: light behind, source on top
        let outer = glow(&[square()], 10., [255, 0, 0], 0.5, mode(true, false, true)).unwrap();
        assert_eq!(outer.len(), 2);
        assert_eq!(outer[1], square());
        assert!(alpha_at(&outer[..1], Point::new(104., 50.)) > 0.1);
        assert_smooth(&outer[..1], 100., 130., 0.08);
        // both: light, source, inner light
        let both = glow(&[square()], 10., [255, 0, 0], 0.5, mode(true, true, true)).unwrap();
        assert_eq!(both.len(), 3);
        assert_eq!(both[1], square());
        assert!(glow(&[square()], 100., [255, 0, 0], 0.5, mode(true, true, true)).is_ok());
        // halo only: the shape area stays open
        let halo = glow(&[square()], 10., [255, 0, 0], 0.5, mode(true, false, false)).unwrap();
        assert!(!halo.contains(&square()));
        assert!(alpha_at(&halo, Point::new(50., 50.)) < 0.01);
        assert!(alpha_at(&halo, Point::new(103., 50.)) > 0.);
        let inner_only = glow(&[square()], 10., [255, 0, 0], 0.5, mode(false, true, false)).unwrap();
        assert_eq!(inner_only.len(), 1);
    }
    #[test]
    fn graph_opacity_matches_canvas_layers_and_svg() {
        use super::super::{
            appearance::{Graph, Kind},
            model::Document,
            outline,
        };
        let mut doc = Document::example();
        doc.strokes.truncate(1);
        let mut graph = Graph::default();
        let id = graph.add(Kind::Opacity, [200., 100.]).unwrap();
        graph.node_mut(id).unwrap().opacity = 0.25;
        graph.connect(1, id, 0).unwrap();
        graph.connect(id, graph.output, 0).unwrap();
        doc.strokes[0].appearance = Some(graph);
        let painted = outline::paint(&doc.strokes[0]).unwrap();
        assert!(!painted.layers.is_empty());
        assert!(painted.layers.iter().all(|l| l.opacity == 0.25));
        let svg = outline::svg(&doc).unwrap();
        assert!(svg.contains("fill-opacity=\"0.250000\""));
        doc.strokes[0]
            .appearance
            .as_mut()
            .unwrap()
            .node_mut(id)
            .unwrap()
            .opacity = 0.;
        assert!(!outline::svg(&doc).unwrap().contains("<path"));
    }
    #[test]
    fn paint_effects_enforce_layer_budget() {
        let source = vec![square(); MAX_LAYERS + 1];
        assert!(opacity(&source, 0.5).is_err());
        assert!(blur(&source, 5.).is_err());
    }
}
