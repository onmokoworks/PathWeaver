//! Non-destructive contour effects used by the appearance graph.
//!
//! Parameters are in document units. Boolean operations use NonZero fill, so
//! holes and disconnected components survive offsets and pattern clipping.
use super::outline::boolean_contours;
use i_overlay::core::overlay_rule::OverlayRule;
use kurbo::{Point, Rect, Vec2};
use serde::{Deserialize, Serialize};

const MAX_POINTS: usize = 100_000;
const MAX_COORDINATE: f64 = 1.0e9;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectKind {
    Offset,
    Round,
    Band,
    Pucker,
    Simplify,
    Streak,
    Dissolve,
    Halftone,
    Mezzotint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffsetJoin {
    #[default]
    Round,
    Miter,
    Bevel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BandPosition {
    #[default]
    Outer,
    Inner,
    Center,
}

/// Grain of the mezzotint: round dots or short strokes along `angle`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grain {
    #[default]
    Dots,
    Lines,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectParams {
    pub amount: f64,
    pub detail: f64,
    pub angle: f64,
    pub spread: f64,
    pub seed: u32,
    pub join: OffsetJoin,
    pub position: BandPosition,
    pub grain: Grain,
}

impl Default for EffectParams {
    fn default() -> Self {
        Self {
            amount: 10.,
            detail: 2.,
            angle: 0.,
            spread: 12.,
            seed: 1,
            join: OffsetJoin::Round,
            position: BandPosition::Outer,
            grain: Grain::Dots,
        }
    }
}

fn validate_contours(contours: &[Vec<Point>]) -> Result<(), String> {
    if contours.iter().map(Vec::len).sum::<usize>() > MAX_POINTS {
        return Err("効果の輪郭が複雑すぎます（上限100,000点）".into());
    }
    if contours.iter().flatten().any(|p| {
        !p.x.is_finite()
            || !p.y.is_finite()
            || p.x.abs() > MAX_COORDINATE
            || p.y.abs() > MAX_COORDINATE
    }) {
        return Err("効果の輪郭に無効な座標があります".into());
    }
    Ok(())
}

impl EffectParams {
    pub fn validate(&self) -> Result<(), String> {
        if [self.amount, self.detail, self.angle, self.spread]
            .iter()
            .any(|value| !value.is_finite() || value.abs() > 10_000.)
        {
            return Err("効果の数値が範囲外です".into());
        }
        Ok(())
    }
}

fn normalized(contours: &[Vec<Point>]) -> Vec<Vec<Point>> {
    boolean_contours(contours, &[], OverlayRule::Subject)
}

/// Signed silhouette offset. A positive distance expands filled regions and
/// shrinks holes. Negative offsets remove a boundary band, so narrow regions
/// disappear instead of turning inside out after erosion.
/// One positive filled piece of an offset band (strip or join).
fn add_piece(band: &mut Vec<Vec<Point>>, total_points: &mut usize, mut polygon: Vec<Point>) -> Result<(), String> {
    let area = polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>();
    if area.abs() < 1e-12 {
        return Ok(());
    }
    // Each strip/join is a positive filled primitive. Opposite source
    // contour winding must never cancel overlapping stroke pieces.
    if area < 0. {
        polygon.reverse();
    }
    *total_points += polygon.len();
    if *total_points > MAX_POINTS * 4 {
        return Err("オフセットの輪郭が複雑すぎます".into());
    }
    band.push(polygon);
    Ok(())
}

pub fn offset_contours(
    contours: &[Vec<Point>],
    distance: f64,
    join: OffsetJoin,
) -> Result<Vec<Vec<Point>>, String> {
    validate_contours(contours)?;
    if !distance.is_finite() || distance.abs() > 10_000. {
        return Err("オフセット距離が範囲外です".into());
    }
    if distance.abs() < 1e-8 || contours.is_empty() {
        return Ok(contours.to_vec());
    }
    // Drop sub-pixel detail first (0.2 units ≈ invisible). Offsets of offsets
    // (blur bands, glow) otherwise turn every micro segment into a strip and
    // the union below explodes (minutes for Rough → Glow).
    let mut budget = 2_000_000;
    let simplified: Vec<Vec<Point>> = contours
        .iter()
        .map(|ring| simplify(ring, 0.2, &mut budget))
        .collect::<Result<_, _>>()?;
    let base = drop_slivers(normalized(&simplified));
    let radius = distance.abs();
    let mut band: Vec<Vec<Point>> = Vec::new();
    let mut total_points = 0;
    // Strips and joins are merged per ring first. Many small rings (mezzotint
    // grains, halftone dots) otherwise feed tens of thousands of overlapping
    // pieces into one union, which took tens of seconds.
    let mut merged: Vec<Vec<Point>> = Vec::new();
    for ring in &base {
        if ring.len() < 3 {
            continue;
        }
        let first_piece = band.len();
        for i in 0..ring.len() {
            let p = ring[i];
            let previous = ring[(i + ring.len() - 1) % ring.len()];
            let next = ring[(i + 1) % ring.len()];
            let incoming = p - previous;
            let outgoing = next - p;
            let in_length = incoming.hypot();
            let out_length = outgoing.hypot();
            if out_length < 1e-10 {
                continue;
            }
            let outgoing = outgoing / out_length;
            let right = Vec2::new(outgoing.y, -outgoing.x) * radius;
            add_piece(&mut band, &mut total_points, vec![p + right, next + right, next - right, p - right])?;
            if in_length < 1e-10 {
                continue;
            }
            let incoming = incoming / in_length;
            let cross = incoming.x * outgoing.y - incoming.y * outgoing.x;
            let turn = cross.atan2(incoming.dot(outgoing));
            if turn.abs() < 1e-8 {
                continue;
            }
            let before = Vec2::new(incoming.y, -incoming.x) * radius;
            // Only the outer side of a turn opens a gap between neighbouring
            // strips (the right side of a left turn and vice versa); the inner
            // side is already covered. Filling both doubled the work.
            let outer = if turn > 0. { 1. } else { -1. };
            for side in [outer] {
                let a = p + before * side;
                let b = p + right * side;
                match join {
                    OffsetJoin::Round => {
                        let start = (a.y - p.y).atan2(a.x - p.x);
                        // Segments from a 0.25 px chord tolerance instead of one per
                        // half unit of arc length (up to 128): same look, far fewer
                        // points for the union below.
                        let step = 2. * (1. - (0.25 / radius).min(1.)).acos();
                        let count = (turn.abs() / step.max(0.05)).ceil().clamp(1., 64.) as usize;
                        let mut sector = Vec::with_capacity(count + 2);
                        sector.push(p);
                        for j in 0..=count {
                            let angle = start + turn * j as f64 / count as f64;
                            sector.push(p + Vec2::new(angle.cos(), angle.sin()) * radius);
                        }
                        add_piece(&mut band, &mut total_points, sector)?;
                    }
                    OffsetJoin::Miter if cross.abs() > 1e-8 => {
                        let delta = b - a;
                        let t = (delta.x * outgoing.y - delta.y * outgoing.x) / cross;
                        let miter = a + incoming * t;
                        if miter.distance(p) <= radius * 10. {
                            add_piece(&mut band, &mut total_points, vec![p, a, miter, b])?;
                        } else {
                            add_piece(&mut band, &mut total_points, vec![p, a, b])?;
                        }
                    }
                    _ => add_piece(&mut band, &mut total_points, vec![p, a, b])?,
                }
            }
        }
        merged.extend(boolean_contours(&band[first_piece..], &[], OverlayRule::Subject));
        band.truncate(first_piece);
    }
    let output = boolean_contours(
        &base,
        &merged,
        if distance > 0. {
            OverlayRule::Union
        } else {
            OverlayRule::Difference
        },
    );
    // Overlays leave zero-area slivers along shared edges; offset again (角を丸く
    // = in then out) they would grow into visible dots.
    let output = drop_slivers(output);
    validate_contours(&output)?;
    Ok(output)
}

/// Rings with (almost) no area: rounding leftovers, never real shapes.
fn drop_slivers(mut rings: Vec<Vec<Point>>) -> Vec<Vec<Point>> {
    rings.retain(|r| {
        let area: f64 = r.iter().zip(r.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - a.y * b.x).sum::<f64>() * 0.5;
        area.abs() > 0.01
    });
    rings
}

fn bounds(contours: &[Vec<Point>]) -> Option<Rect> {
    let mut points = contours.iter().flatten();
    let first = *points.next()?;
    let mut bounds = Rect::from_points(first, first);
    for p in points {
        bounds = bounds.union_pt(*p);
    }
    Some(bounds)
}

fn circle(center: Point, radius: f64) -> Vec<Point> {
    polygon(center, radius, 12)
}
/// Regular polygon; small grains need fewer sides than dots or bites.
fn polygon(center: Point, radius: f64, sides: usize) -> Vec<Point> {
    (0..sides)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / sides as f64;
            center + Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

struct Random(u32);
impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 as f64 / 4_294_967_296.
    }
}

fn perimeter(ring: &[Point]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| a.distance(*b))
        .sum()
}

fn resample(ring: &[Point], step: f64) -> Vec<Point> {
    let length = perimeter(ring);
    if ring.len() < 3 || length < 1e-8 {
        return ring.to_vec();
    }
    let count = (length / step).ceil().clamp(3., 2048.) as usize;
    let mut output = Vec::with_capacity(count);
    let mut edge = 0;
    let mut distance = 0.;
    for i in 0..count {
        let target = length * i as f64 / count as f64;
        while edge + 1 < ring.len() {
            let edge_length = ring[edge].distance(ring[(edge + 1) % ring.len()]);
            if distance + edge_length >= target {
                break;
            }
            distance += edge_length;
            edge += 1;
        }
        let a = ring[edge];
        let b = ring[(edge + 1) % ring.len()];
        let t = ((target - distance) / a.distance(b).max(1e-9)).clamp(0., 1.);
        output.push(a.lerp(b, t));
    }
    output
}

fn point_segment_distance_squared(p: Point, a: Point, b: Point) -> f64 {
    let v = b - a;
    let t = ((p - a).dot(v) / v.hypot2().max(1e-18)).clamp(0., 1.);
    p.distance_squared(a + v * t)
}

/// Closed-ring Ramer–Douglas–Peucker, retaining at least three points.
fn simplify(ring: &[Point], tolerance: f64, budget: &mut usize) -> Result<Vec<Point>, String> {
    if ring.len() < 4 || tolerance <= 0. {
        return Ok(ring.to_vec());
    }
    let split = (1..ring.len())
        .max_by(|a, b| {
            ring[*a]
                .distance_squared(ring[0])
                .total_cmp(&ring[*b].distance_squared(ring[0]))
        })
        .unwrap_or(1);
    let mut keep = vec![false; ring.len()];
    keep[0] = true;
    keep[split] = true;
    let mut stack = vec![(0, split), (split, ring.len())];
    while let Some((a, b)) = stack.pop() {
        let mut farthest = None;
        let mut max = tolerance * tolerance;
        for i in a + 1..b {
            *budget = budget
                .checked_sub(1)
                .ok_or("単純化の計算量が上限を超えました。先に輪郭を減らしてください")?;
            let d = point_segment_distance_squared(ring[i], ring[a], ring[b % ring.len()]);
            if d > max {
                max = d;
                farthest = Some(i);
            }
        }
        if let Some(i) = farthest {
            keep[i] = true;
            stack.push((a, i));
            stack.push((i, b));
        }
    }
    let output: Vec<_> = ring
        .iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect();
    Ok(if output.len() < 3 {
        ring.to_vec()
    } else {
        output
    })
}

pub fn apply_with_params(
    kind: EffectKind,
    contours: &[Vec<Point>],
    params: &EffectParams,
) -> Result<Vec<Vec<Point>>, String> {
    params.validate()?;
    validate_contours(contours)?;
    if contours.is_empty() {
        return Ok(vec![]);
    }
    let amount = params.amount;
    let output = match kind {
        EffectKind::Offset => offset_contours(contours, amount, params.join)?,
        EffectKind::Round => {
            if amount < 0. {
                return Err("角丸の半径は0以上で指定してください".into());
            }
            offset_contours(
                &offset_contours(contours, -amount, OffsetJoin::Round)?,
                amount,
                OffsetJoin::Round,
            )?
        }
        EffectKind::Band => {
            if amount < 0. {
                return Err("縁取りの幅は0以上で指定してください".into());
            }
            let (outside, inside) = match params.position {
                BandPosition::Outer => (amount, 0.),
                BandPosition::Inner => (0., -amount),
                BandPosition::Center => (amount / 2., -amount / 2.),
            };
            boolean_contours(
                &offset_contours(contours, outside, params.join)?,
                &offset_contours(contours, inside, params.join)?,
                OverlayRule::Difference,
            )
        }
        EffectKind::Pucker => {
            if amount.abs() > 100. {
                return Err("膨張率は-100〜100%で指定してください".into());
            }
            if amount == 0. {
                return Ok(contours.to_vec());
            }
            if contours.iter().map(Vec::len).sum::<usize>() > MAX_POINTS / 2 {
                return Err("パンク・膨張の輪郭が複雑すぎます".into());
            }
            let rings: Vec<_> = contours
                .iter()
                .filter(|ring| ring.len() >= 3)
                .map(|ring| {
                    let center = ring.iter().map(|p| p.to_vec2()).sum::<Vec2>() / ring.len() as f64;
                    ring.iter()
                        .enumerate()
                        .flat_map(|(i, p)| {
                            let middle = p.lerp(ring[(i + 1) % ring.len()], 0.5);
                            [*p, middle + (middle.to_vec2() - center) * (amount / 100.)]
                        })
                        .collect()
                })
                .collect();
            normalized(&rings)
        }
        EffectKind::Simplify => {
            if amount < 0. {
                return Err("単純化の距離は0以上で指定してください".into());
            }
            let mut budget = 2_000_000;
            contours
                .iter()
                .map(|ring| simplify(ring, amount, &mut budget))
                .collect::<Result<Vec<_>, _>>()?
        }
        EffectKind::Streak => {
            if !(0. ..=100.).contains(&amount) || !(0. ..=1000.).contains(&params.detail) {
                return Err("かすれの本数または細さが範囲外です".into());
            }
            if amount == 0. || params.detail == 0. {
                return Ok(contours.to_vec());
            }
            let Some(bounds) = bounds(contours) else {
                return Ok(vec![]);
            };
            let center = bounds.center();
            let diagonal = bounds.width().hypot(bounds.height());
            let rotation = kurbo::Affine::translate(center.to_vec2())
                * kurbo::Affine::rotate(params.angle.to_radians());
            let mut random = Random(params.seed);
            let stripes: Vec<_> = (0..amount.floor() as usize)
                .map(|_| {
                    let y = (random.next() - 0.5) * diagonal;
                    let width = params.detail * (0.3 + random.next());
                    [
                        Point::new(-diagonal, y),
                        Point::new(diagonal, y),
                        Point::new(diagonal, y + width),
                        Point::new(-diagonal, y + width),
                    ]
                    .map(|p| rotation * p)
                    .to_vec()
                })
                .collect();
            boolean_contours(contours, &stripes, OverlayRule::Difference)
        }
        EffectKind::Dissolve => {
            if !(0. ..=100.).contains(&amount) || params.detail <= 0. || params.spread < 0. {
                return Err("溶解の量・粒の大きさ・散らばりが範囲外です".into());
            }
            if amount == 0. {
                return Ok(contours.to_vec());
            }
            let total: f64 = contours.iter().map(|ring| perimeter(ring)).sum();
            let step = (params.detail * 3.).max(total / 400.).max(0.01);
            let mut random = Random(params.seed);
            let mut bites = Vec::new();
            let mut specks = Vec::new();
            for ring in contours {
                for p in resample(ring, step) {
                    if bites.len() >= 1024 {
                        break;
                    }
                    if random.next() >= amount / 100. {
                        continue;
                    }
                    let radius = params.detail * (0.3 + random.next() * 0.7);
                    bites.push(circle(p, radius));
                    let angle = random.next() * std::f64::consts::TAU;
                    let distance = radius + params.spread * random.next();
                    specks.push(circle(
                        p + Vec2::new(angle.cos(), angle.sin()) * distance,
                        radius * 0.22,
                    ));
                }
            }
            let mut result = boolean_contours(contours, &bites, OverlayRule::Difference);
            result.extend(boolean_contours(&specks, contours, OverlayRule::Difference));
            normalized(&result)
        }
        EffectKind::Halftone => {
            if amount <= 0. || !(0.01..=0.5).contains(&params.detail) {
                return Err("ハーフトーンの間隔または点の比率が範囲外です".into());
            }
            let Some(bounds) = bounds(contours) else {
                return Ok(vec![]);
            };
            let radius = bounds.width().hypot(bounds.height()) / 2.;
            let step = amount.max(radius / 18.);
            let dot_radius = step * params.detail;
            let rotation = kurbo::Affine::translate(bounds.center().to_vec2())
                * kurbo::Affine::rotate(params.angle.to_radians());
            let count = (radius * 2. / step).ceil() as usize;
            let mut dots = Vec::new();
            for y in 0..=count {
                for x in 0..=count {
                    let p =
                        rotation * Point::new(-radius + x as f64 * step, -radius + y as f64 * step);
                    if p.x + dot_radius >= bounds.x0
                        && p.x - dot_radius <= bounds.x1
                        && p.y + dot_radius >= bounds.y0
                        && p.y - dot_radius <= bounds.y1
                    {
                        dots.push(circle(p, dot_radius));
                    }
                }
            }
            boolean_contours(&dots, contours, OverlayRule::Intersect)
        }
        EffectKind::Mezzotint => {
            // Random grains scattered over the bounds, kept only inside the shape.
            if !(0. ..=100.).contains(&amount) || !(0.1..=200.).contains(&params.detail) {
                return Err("メゾティントの密度または粒の大きさが範囲外です".into());
            }
            if amount == 0. {
                return Ok(vec![]);
            }
            let Some(bounds) = bounds(contours) else {
                return Ok(vec![]);
            };
            let size = params.detail;
            let (grain_area, reach) = match params.grain {
                Grain::Dots => (std::f64::consts::PI * size * size * 0.42, size),
                Grain::Lines => (size * 6. * size * 0.5, size * 3.5),
            };
            let area = bounds.width().max(size) * bounds.height().max(size);
            let wanted = amount / 100. * area / grain_area.max(1e-6);
            if wanted > MEZZOTINT_MAX as f64 * 4. {
                return Err("メゾティントの粒が多すぎます。粒を大きくするか密度を下げてください".into());
            }
            let count = (wanted.round() as usize).min(MEZZOTINT_MAX);
            let mut random = Random(params.seed.wrapping_mul(2_654_435_761).wrapping_add(17));
            let base = params.angle.to_radians();
            let mut grains = Vec::with_capacity(count);
            for _ in 0..count {
                let p = Point::new(
                    bounds.x0 - reach + random.next() * (bounds.width() + reach * 2.),
                    bounds.y0 - reach + random.next() * (bounds.height() + reach * 2.),
                );
                let scale = 0.55 + random.next() * 0.9;
                grains.push(match params.grain {
                    Grain::Dots => polygon(p, size * 0.5 * scale, if size < 6. { 8 } else { 12 }),
                    Grain::Lines => {
                        let a = base + (random.next() - 0.5) * 0.35;
                        let along = Vec2::new(a.cos(), a.sin()) * (size * 3. * scale);
                        let across = Vec2::new(-a.sin(), a.cos()) * (size * 0.25 * scale);
                        vec![p - along - across, p + along - across, p + along + across, p - along + across]
                    }
                });
            }
            boolean_contours(&grains, contours, OverlayRule::Intersect)
        }
    };
    validate_contours(&output)?;
    Ok(output)
}

/// Upper bound on generated grains (each dot is 12 vertices; stays well
/// under MAX_POINTS after clipping).
const MEZZOTINT_MAX: usize = 4000;

#[cfg(test)]
mod tests {
    use super::*;
    fn square(x: f64, y: f64, size: f64) -> Vec<Point> {
        vec![
            Point::new(x, y),
            Point::new(x + size, y),
            Point::new(x + size, y + size),
            Point::new(x, y + size),
        ]
    }
    fn area(contours: &[Vec<Point>]) -> f64 {
        contours
            .iter()
            .map(|ring| {
                ring.iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(a, b)| a.x * b.y - b.x * a.y)
                    .sum::<f64>()
                    / 2.
            })
            .sum::<f64>()
            .abs()
    }
    fn donut() -> Vec<Vec<Point>> {
        let mut hole = square(30., 30., 40.);
        hole.reverse();
        vec![square(0., 0., 100.), hole]
    }
    #[test]
    fn signed_offset_preserves_holes_and_erodes_thin_regions() {
        let source = donut();
        let bigger = offset_contours(&source, 5., OffsetJoin::Miter).unwrap();
        let smaller = offset_contours(&source, -5., OffsetJoin::Miter).unwrap();
        assert!((area(&bigger) - (110. * 110. - 30. * 30.)).abs() < 0.01);
        assert!((area(&smaller) - (90. * 90. - 50. * 50.)).abs() < 0.01);
        assert_eq!(bigger.len(), 2);
        assert_eq!(smaller.len(), 2);
        assert!(
            offset_contours(&[square(0., 0., 10.)], -6., OffsetJoin::Round)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn round_and_band_have_distinct_geometry_and_join_styles() {
        let source = vec![square(0., 0., 100.)];
        let p = EffectParams {
            amount: 10.,
            ..Default::default()
        };
        let round = apply_with_params(EffectKind::Round, &source, &p).unwrap();
        assert!(area(&round) < 10_000. && area(&round) > 9_800.);
        let outer = apply_with_params(EffectKind::Band, &source, &p).unwrap();
        let inner = apply_with_params(
            EffectKind::Band,
            &source,
            &EffectParams {
                position: BandPosition::Inner,
                ..p.clone()
            },
        )
        .unwrap();
        assert!(area(&outer) > area(&inner));
        let round = offset_contours(&source, 10., OffsetJoin::Round).unwrap();
        let bevel = offset_contours(&source, 10., OffsetJoin::Bevel).unwrap();
        let miter = offset_contours(&source, 10., OffsetJoin::Miter).unwrap();
        assert!(area(&miter) > area(&round) && area(&round) > area(&bevel));
    }
    #[test]
    fn seeded_damage_is_reproducible_and_different_seeds_change_it() {
        let source = donut();
        for kind in [EffectKind::Streak, EffectKind::Dissolve] {
            let params = EffectParams {
                amount: 40.,
                detail: 3.,
                seed: 7,
                ..Default::default()
            };
            let a = apply_with_params(kind, &source, &params).unwrap();
            let b = apply_with_params(kind, &source, &params).unwrap();
            let c = apply_with_params(kind, &source, &EffectParams { seed: 8, ..params }).unwrap();
            assert_eq!(a, b);
            assert_ne!(a, c);
            assert!(area(&a) < area(&source));
        }
    }
    #[test]
    fn mezzotint_grains_stay_inside_and_follow_density_and_seed() {
        let source = donut();
        let p = |amount: f64, seed: u32, grain: Grain| EffectParams {
            amount, detail: 3., angle: 30., seed, grain, ..Default::default()
        };
        let sparse = apply_with_params(EffectKind::Mezzotint, &source, &p(15., 1, Grain::Dots)).unwrap();
        let dense = apply_with_params(EffectKind::Mezzotint, &source, &p(60., 1, Grain::Dots)).unwrap();
        // clipped to the shape, never filling the donut's hole
        assert!(boolean_contours(&dense, &source, OverlayRule::Difference).is_empty());
        assert!(area(&sparse) > 0. && area(&sparse) < area(&dense) && area(&dense) < area(&source));
        // deterministic per seed, different across seeds
        assert_eq!(sparse, apply_with_params(EffectKind::Mezzotint, &source, &p(15., 1, Grain::Dots)).unwrap());
        assert_ne!(sparse, apply_with_params(EffectKind::Mezzotint, &source, &p(15., 2, Grain::Dots)).unwrap());
        // line grains also stay inside
        let lines = apply_with_params(EffectKind::Mezzotint, &source, &p(40., 1, Grain::Lines)).unwrap();
        assert!(area(&lines) > 0.);
        assert!(boolean_contours(&lines, &source, OverlayRule::Difference).is_empty());
        // zero density = nothing; absurd grain count is refused, not hung
        assert!(apply_with_params(EffectKind::Mezzotint, &source, &p(0., 1, Grain::Dots)).unwrap().is_empty());
        let tiny = EffectParams { amount: 100., detail: 0.1, ..Default::default() };
        assert!(apply_with_params(EffectKind::Mezzotint, &vec![square(0., 0., 5000.)], &tiny).is_err());
    }
    #[test]
    fn halftone_is_clipped_to_source_and_never_fills_holes() {
        let source = donut();
        let params = EffectParams {
            amount: 10.,
            detail: 0.3,
            angle: 15.,
            ..Default::default()
        };
        let dots = apply_with_params(EffectKind::Halftone, &source, &params).unwrap();
        assert!(dots.len() > 10);
        assert!(area(&dots) > 0. && area(&dots) < area(&source));
        assert!(boolean_contours(&dots, &source, OverlayRule::Difference).is_empty());
    }
    #[test]
    fn pucker_and_simplify_keep_closed_finite_shapes() {
        let source = vec![square(0., 0., 100.)];
        let pucker = apply_with_params(
            EffectKind::Pucker,
            &source,
            &EffectParams {
                amount: 20.,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(area(&pucker) > area(&source));
        let dense = vec![resample(&source[0], 2.)];
        let simple = apply_with_params(
            EffectKind::Simplify,
            &dense,
            &EffectParams {
                amount: 0.1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(simple[0].len(), 4);
        assert!((area(&simple) - area(&source)).abs() < 0.01);
    }
    #[test]
    fn rejects_nonfinite_and_excessive_inputs() {
        assert!(simplify(&square(0., 0., 100.), 0.1, &mut 0).is_err());
        assert!(offset_contours(&[square(0., 0., 100.)], f64::NAN, OffsetJoin::Round).is_err());
        assert!(apply_with_params(
            EffectKind::Streak,
            &[square(0., 0., 100.)],
            &EffectParams {
                amount: 1e9,
                ..Default::default()
            }
        )
        .is_err());
        assert!(offset_contours(
            &[vec![Point::new(f64::INFINITY, 0.)]],
            1.,
            OffsetJoin::Round
        )
        .is_err());
        assert!(
            offset_contours(&[vec![Point::ZERO; MAX_POINTS + 1]], 1., OffsetJoin::Round).is_err()
        );
    }
}
