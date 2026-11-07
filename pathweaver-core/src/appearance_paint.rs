//! Paint effects use translucent vector bands so the canvas and SVG share geometry.
//! Blur is a bounded, eight-band approximation of a Gaussian edge, not a raster filter.
use super::{
    appearance::Layer,
    appearance_effects::{offset_contours, OffsetJoin},
    outline::boolean_contours,
};
use i_overlay::core::overlay_rule::OverlayRule;
use kurbo::{Point, Vec2};

const MAX_LAYERS: usize = 64;
const MAX_VERTICES: usize = 200_000;
const BLUR_BANDS: usize = 8;

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
fn push_layer(result: &mut Vec<Layer>, layer: Layer) -> Result<(), String> {
    if !layer.contours.is_empty() && layer.opacity > 0. {
        result.push(layer);
        check_layers(result)?;
    }
    Ok(())
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

/// CDF-based coverage at a signed distance from an edge. The logistic approximation
/// is smooth and monotonic; exact Gaussian filtering is deliberately not claimed.
fn coverage(distance_in_sigmas: f64) -> f64 {
    1. / (1. + (1.702 * distance_in_sigmas).exp())
}

pub fn blur(layers: &[Layer], radius: f64) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(radius, 0., 100.)?;
    if radius < 1e-6 {
        return Ok(layers.to_vec());
    }
    let mut result = Vec::new();
    for layer in layers.iter().filter(|l| l.opacity > 0.) {
        let source = boolean_contours(&layer.contours, &[], OverlayRule::Subject);
        if source.is_empty() {
            continue;
        }
        let mut inner = offset_contours(&source, -2. * radius, OffsetJoin::Round)?;
        push_layer(
            &mut result,
            Layer {
                color: layer.color,
                opacity: layer.opacity,
                contours: inner.clone(),
            },
        )?;
        for band in 0..BLUR_BANDS {
            let a = -2. + 4. * band as f64 / BLUR_BANDS as f64;
            let b = -2. + 4. * (band + 1) as f64 / BLUR_BANDS as f64;
            let outer = offset_contours(&source, b * radius, OffsetJoin::Round)?;
            let shell = boolean_contours(&outer, &inner, OverlayRule::Difference);
            push_layer(
                &mut result,
                Layer {
                    color: layer.color,
                    opacity: layer.opacity * coverage((a + b) * 0.5),
                    contours: shell,
                },
            )?;
            inner = outer;
        }
    }
    Ok(result)
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
    let contours = offset_contours(&silhouette(layers), spread, OffsetJoin::Round)?
        .into_iter()
        .map(|ring| ring.into_iter().map(|p| p + Vec2::new(dx, dy)).collect())
        .collect();
    let mut result = blur(
        &[Layer {
            color,
            opacity: amount,
            contours,
        }],
        radius,
    )?;
    result.extend_from_slice(layers);
    check_layers(&result)?;
    Ok(result)
}

pub fn glow(
    layers: &[Layer],
    size: f64,
    color: [u8; 3],
    amount: f64,
    inner: bool,
) -> Result<Vec<Layer>, String> {
    check_layers(layers)?;
    check_range(size, 0., 100.)?;
    check_range(amount, 0., 1.)?;
    if size < 1e-6 || amount == 0. {
        return Ok(layers.to_vec());
    }
    let source = silhouette(layers);
    let contours = if inner {
        let inset = offset_contours(&source, -size, OffsetJoin::Round)?;
        boolean_contours(&source, &inset, OverlayRule::Difference)
    } else {
        offset_contours(&source, size * 0.5, OffsetJoin::Round)?
    };
    let mut halo = blur(
        &[Layer {
            color,
            opacity: amount,
            contours,
        }],
        size * 0.5,
    )?;
    if inner {
        for l in &mut halo {
            l.contours = boolean_contours(&l.contours, &source, OverlayRule::Intersect);
        }
        halo.retain(|l| !l.contours.is_empty());
    }
    let mut result = Vec::new();
    if inner {
        result.extend_from_slice(layers);
        result.extend(halo);
    } else {
        result.extend(halo);
        result.extend_from_slice(layers);
    }
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
        }
    }
    fn bounds(layers: &[Layer]) -> kurbo::Rect {
        layers
            .iter()
            .flat_map(|l| l.contours.iter().flatten())
            .fold(kurbo::Rect::ZERO, |r, p| r.union_pt(*p))
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
    fn alpha_at(layers: &[Layer], p: Point) -> f64 {
        layers
            .iter()
            .filter(|l| contains(&l.contours, p))
            .fold(0., |a, l| l.opacity + a * (1. - l.opacity))
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
    fn blur_is_bounded_and_fades_toward_outside() {
        let original = square();
        assert_eq!(
            blur(std::slice::from_ref(&original), 0.).unwrap(),
            vec![original.clone()]
        );
        let blurred = blur(&[original], 10.).unwrap();
        assert!(blurred.len() <= BLUR_BANDS + 1);
        let b = bounds(&blurred);
        assert!(b.x0 >= -20.01 && b.x1 <= 120.01);
        let center = alpha_at(&blurred, Point::new(50., 50.));
        let near = alpha_at(&blurred, Point::new(103., 50.));
        let far = alpha_at(&blurred, Point::new(116., 50.));
        assert!(center > near && near > far && far > 0.);
        assert!(blurred.iter().all(|l| l.color == [100, 150, 200]));
        assert!(blur(&[square()], 101.).is_err());
    }
    #[test]
    fn inner_glow_is_clipped_while_outer_glow_sits_behind_source() {
        let original = square();
        let inner = glow(std::slice::from_ref(&original), 10., [255, 0, 0], 0.5, true).unwrap();
        assert_eq!(inner[0], original);
        assert!(inner[1..].iter().all(|l| l
            .contours
            .iter()
            .flatten()
            .all(|p| p.x >= -1e-6 && p.x <= 100. + 1e-6 && p.y >= -1e-6 && p.y <= 100. + 1e-6)));
        let outer = glow(&[square()], 10., [255, 0, 0], 0.5, false).unwrap();
        assert_eq!(outer.last().unwrap(), &square());
        assert!(bounds(&outer).x1 > 100.);
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
        assert!(blur(&vec![square(); 8], 5.).is_err());
    }
}
