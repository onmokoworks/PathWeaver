use super::model::{Document, Join, Stroke, Width, WidthTarget};
use kurbo::{Point, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub segment: usize,
    pub t: f64,
    pub p: Point,
    pub distance: f64,
}

pub fn samples(stroke: &Stroke) -> Vec<Sample> {
    fn recurse(s: &Stroke, i: usize, a: f64, b: f64, depth: u32, out: &mut Vec<Sample>) {
        let p = s.eval(i, a);
        let q = s.eval(i, b);
        let m = s.eval(i, (a + b) * 0.5);
        let e = m
            .distance(p.lerp(q, 0.5))
            .max(s.eval(i, a + (b - a) * 0.25).distance(p.lerp(q, 0.25)))
            .max(s.eval(i, a + (b - a) * 0.75).distance(p.lerp(q, 0.75)));
        if depth < 3 || (depth < 14 && (p.distance(q) > 1.5 || e > 0.015)) {
            recurse(s, i, a, (a + b) * 0.5, depth + 1, out);
            recurse(s, i, (a + b) * 0.5, b, depth + 1, out);
        } else {
            out.push(Sample {
                segment: i,
                t: b,
                p: q,
                distance: 0.0,
            });
        }
    }
    let mut out = vec![];
    for i in 0..stroke.segment_count() {
        if i == 0 {
            out.push(Sample {
                segment: i,
                t: 0.0,
                p: stroke.eval(i, 0.0),
                distance: 0.0,
            });
        }
        recurse(stroke, i, 0., 1., 0, &mut out);
    }
    for i in 1..out.len() {
        out[i].distance = out[i - 1].distance + out[i].p.distance(out[i - 1].p);
    }
    out
}

pub fn at_distance(samples: &[Sample], segment: usize, t: f64) -> f64 {
    let target = segment as f64 + t;
    for pair in samples.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let x = a.segment as f64 + a.t;
        let y = b.segment as f64 + b.t;
        if target <= y {
            return a.distance
                + (b.distance - a.distance) * ((target - x) / (y - x).max(1e-12)).clamp(0., 1.);
        }
    }
    samples.last().map_or(0., |p| p.distance)
}

/// Shape-preserving cubic Hermite interpolation (weighted harmonic interior slopes).
fn interpolate(keys: &[(f64, f64)], x: f64) -> f64 {
    if keys.len() == 1 {
        return keys[0].1;
    }
    let delta = |i: usize| (keys[i + 1].1 - keys[i].1) / (keys[i + 1].0 - keys[i].0);
    let slope = |i: usize| {
        if i == 0 {
            return delta(0);
        }
        if i == keys.len() - 1 {
            return delta(i - 1);
        }
        let a = delta(i - 1);
        let b = delta(i);
        if a * b <= 0. {
            0.
        } else {
            let h0 = keys[i].0 - keys[i - 1].0;
            let h1 = keys[i + 1].0 - keys[i].0;
            let w0 = 2. * h1 + h0;
            let w1 = h1 + 2. * h0;
            (w0 + w1) / (w0 / a + w1 / b)
        }
    };
    let i = keys
        .partition_point(|k| k.0 < x)
        .saturating_sub(1)
        .min(keys.len() - 2);
    let h = keys[i + 1].0 - keys[i].0;
    let t = ((x - keys[i].0) / h).clamp(0., 1.);
    (2. * t * t * t - 3. * t * t + 1.) * keys[i].1
        + (t * t * t - 2. * t * t + t) * h * slope(i)
        + (-2. * t * t * t + 3. * t * t) * keys[i + 1].1
        + (t * t * t - t * t) * h * slope(i + 1)
}

pub struct Profile {
    left: Vec<(f64, f64)>,
    right: Vec<(f64, f64)>,
    angle: Vec<(f64, f64)>,
}
impl Profile {
    pub fn new(stroke: &Stroke, samples: &[Sample]) -> Self {
        let end = samples.last().map_or(0., |s| s.distance);
        let mut keys = vec![
            (0., stroke.start_width),
            (
                end,
                if stroke.closed {
                    stroke.start_width
                } else {
                    stroke.end_width
                },
            ),
        ];
        for k in stroke.widths.iter().filter(|k| !k.interpolated) {
            if let Some(i) = stroke.segment_index(k.segment) {
                let d = at_distance(samples, i, k.t);
                if d > 0.01 && d < end - 0.01 {
                    keys.push((d, k.width));
                }
            }
        }
        keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        keys.dedup_by(|a, b| (a.0 - b.0).abs() < 0.01);
        Self {
            left: keys.iter().map(|k| (k.0, k.1.left)).collect(),
            right: keys.iter().map(|k| (k.0, k.1.right)).collect(),
            angle: keys.iter().map(|k| (k.0, k.1.angle)).collect(),
        }
    }
    pub fn width(&self, d: f64) -> Width {
        let left = interpolate(&self.left, d);
        let right = interpolate(&self.right, d);
        Width {
            left,
            right,
            linked: (left - right).abs() <= 1e-8,
            angle: interpolate(&self.angle, d),
        }
    }
}

pub fn normal(t: Vec2) -> Vec2 {
    Vec2::new(-t.y, t.x)
}
pub fn cap_direction(s: &Stroke, end: bool) -> Vec2 {
    let (i, t, angle, locked) = if end {
        (s.nodes.len() - 2, 1., s.end_angle, s.end_cap_locked)
    } else {
        (0, 0., s.start_angle, s.start_cap_locked)
    };
    if locked {
        let a = angle.to_radians();
        Vec2::new(a.cos(), a.sin())
    } else {
        normal(s.tangent(i, t))
    }
}
pub fn cap_angle(s: &Stroke, end: bool) -> f64 {
    let d = cap_direction(s, end);
    d.y.atan2(d.x).to_degrees().rem_euclid(180.)
}
pub fn width_direction(tangent: Vec2, angle: f64) -> Vec2 {
    let a = angle.to_radians();
    normal(tangent) * a.cos() + tangent * a.sin()
}

fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// End treatment is confined to two half-widths and at most 20% of the
/// centerline. Each end returns a replacement prefix; the middle stays exact.
fn cap_patch(
    stroke: &Stroke,
    points: &[Sample],
    left: &[Point],
    right: &[Point],
    end: bool,
) -> (usize, Vec<Point>, Vec<Point>) {
    // Ordinary butt caps use the original offset endpoints. Custom clipping
    // must never run unless that particular end was explicitly enabled.
    if stroke.closed
        || !(if end {
            stroke.end_cap_locked
        } else {
            stroke.start_cap_locked
        })
    {
        let index = if end { points.len() - 1 } else { 0 };
        return (0, vec![left[index]], vec![right[index]]);
    }
    let total = points.last().unwrap().distance;
    let (i, t, w) = if end {
        (stroke.nodes.len() - 2, 1., stroke.end_width)
    } else {
        (0, 0., stroke.start_width)
    };
    let center = stroke.eval(i, t);
    let inward = stroke.tangent(i, t) * if end { -1. } else { 1. };
    let mut direction = cap_direction(stroke, end);
    let width_normal = width_direction(stroke.tangent(i, t), w.angle);
    if direction.dot(width_normal) < 0. {
        direction = -direction;
    }
    let terminal_length = if end {
        total - at_distance(points, stroke.nodes.len() - 2, 0.)
    } else {
        at_distance(points, 0, 1.)
    };
    let limit = (2. * w.left.max(w.right).max(1.))
        .min(total * 0.2)
        .min(terminal_length * 0.4);
    let oriented = |side: &[Point]| -> Vec<Point> {
        if end {
            side.iter().rev().copied().collect()
        } else {
            side.to_vec()
        }
    };
    let a = oriented(left);
    let b = oriented(right);
    let distances: Vec<_> = if end {
        points.iter().rev().map(|p| total - p.distance).collect()
    } else {
        points.iter().map(|p| p.distance).collect()
    };
    let k = distances
        .partition_point(|d| *d < limit)
        .min(points.len() - 1)
        .max(1);
    let centerline: Vec<_> = if end {
        points.iter().rev().map(|p| p.p).collect()
    } else {
        points.iter().map(|p| p.p).collect()
    };
    let folded = |side: &[Point]| {
        (1..=k).any(|j| (side[j] - side[j - 1]).dot(centerline[j] - centerline[j - 1]) < -1e-8)
    };
    let has_fold = folded(&a) || folded(&b);
    let exact = |side: &[Point], positive: bool| -> Option<Vec<Point>> {
        // Do not follow a remote intersection when the cut is nearly parallel.
        if has_fold || cross(inward, direction).abs() < 0.01 {
            return None;
        }
        let valid = |p: Point| {
            let along = (p - center).dot(direction) * if positive { 1. } else { -1. };
            along >= -1e-8 && p.distance(center) <= w.left.max(w.right) * 10. + limit
        };
        let mut previous = side[0] - inward * (w.left.max(w.right) * 10. + limit);
        for (j, next) in side.iter().enumerate().take(k + 1) {
            let x = cross(previous - center, direction);
            let y = cross(*next - center, direction);
            if x * y <= 0. && (x - y).abs() > 1e-10 {
                let hit = previous.lerp(*next, x / (x - y));
                if !valid(hit) {
                    return None;
                }
                let mut patch = vec![hit];
                patch.extend_from_slice(&side[j..=k]);
                return Some(patch);
            }
            previous = *next;
        }
        None
    };
    if let (Some(a_patch), Some(b_patch)) = (exact(&a, true), exact(&b, false)) {
        return (k, a_patch, b_patch);
    }
    // Keep a full-width section, never taper the end to hide a failed cut.
    // Use the local approach when the endpoint offset folds back on itself.
    let approach = a[k].lerp(b[k], 0.5) - center;
    if cross(approach, direction) * cross(approach, a[k] - b[k]) < 0. {
        direction = -direction;
    }
    let join = |side: &[Point], half_width: f64, sign: f64| {
        // A path shorter than its own stroke width cannot accommodate two
        // independent oblique sections. Bound only this degenerate case.
        let half_width = if total < 2. * w.left.max(w.right) {
            half_width.min(limit * 0.45)
        } else {
            half_width
        };
        let cap = center + direction * half_width * sign;
        let tip = side[k];
        let v = tip - side[k - 1];
        let tangent = if v.hypot() > 1e-9 {
            v / v.hypot()
        } else {
            inward
        };
        let handle = (cap.distance(tip) * 0.3).min(limit * 0.3);
        let chord = tip - cap;
        let chord_direction = chord / chord.hypot().max(1e-9);
        let end_tangent = if tangent.dot(chord_direction) > 0.3 {
            tangent
        } else {
            chord_direction
        };
        let c1 = cap + chord_direction * handle;
        let c2 = tip - end_tangent * handle;
        let count = ((cap.distance(c1) + c1.distance(c2) + c2.distance(tip)) / 1.0)
            .ceil()
            .clamp(8., 256.) as usize;
        (0..=count)
            .map(|j| {
                let t = j as f64 / count as f64;
                let u = 1. - t;
                (cap.to_vec2() * u.powi(3)
                    + c1.to_vec2() * (3. * u * u * t)
                    + c2.to_vec2() * (3. * u * t * t)
                    + tip.to_vec2() * t.powi(3))
                .to_point()
            })
            .collect()
    };
    (k, join(&a, w.left, 1.), join(&b, w.right, -1.))
}

#[derive(Clone)]
pub struct PaintLayer {
    pub color: [u8; 3],
    pub opacity: f64,
    pub outline: Outline,
    /// A bitmap from the raster stage; `outline` is then its rectangle.
    pub image: Option<std::sync::Arc<super::raster::Image>>,
}
impl Default for PaintLayer {
    fn default() -> Self { Self { color: [0; 3], opacity: 1., outline: Outline::default(), image: None } }
}
#[derive(Clone, Default)]
pub struct Outline {
    pub layers: Vec<PaintLayer>,
    pub samples: Vec<Sample>,
    /// Pre-normalization stroke boundary, retained for geometry diagnostics.
    pub ring: Vec<Point>,
    pub contours: Vec<Vec<Point>>,
    pub vertices: Vec<Point>,
    pub indices: Vec<u32>,
    pub boundary: Vec<[Point; 2]>,
}

/// Add only the outside join. The inside offset edges overlap naturally;
/// NonZero filling removes their overlap without carving a triangular hole.
fn join_side(
    center: Point,
    incoming: Vec2,
    outgoing: Vec2,
    offset_a: Vec2,
    offset_b: Vec2,
    style: Join,
    limit: f64,
) -> Vec<Point> {
    let turn = cross(incoming, outgoing);
    let a = center + offset_a;
    let b = center + offset_b;
    if turn.abs() < 1e-8 || cross(incoming, offset_a) * turn >= 0. {
        return vec![a, b];
    }
    let miter = a + incoming * (cross(b - a, outgoing) / turn);
    let radius = offset_a.hypot().max(offset_b.hypot());
    if style == Join::Miter && miter.distance(center) <= radius * limit + 1e-8 {
        return vec![a, miter, b];
    }
    if style != Join::Round || radius < 1e-9 {
        return vec![a, b];
    }
    let start = offset_a.y.atan2(offset_a.x);
    let mut sweep = (offset_b.y.atan2(offset_b.x) - start).rem_euclid(std::f64::consts::TAU);
    if turn < 0. {
        sweep -= std::f64::consts::TAU;
    }
    let count = (sweep.abs() * radius / 0.75).ceil().clamp(2., 2048.) as usize;
    (0..=count)
        .map(|i| {
            let t = i as f64 / count as f64;
            let angle = start + sweep * t;
            let r = offset_a.hypot() * (1. - t) + offset_b.hypot() * t;
            center + Vec2::new(angle.cos(), angle.sin()) * r
        })
        .collect()
}

pub fn build(stroke: &Stroke) -> Result<Outline, String> {
    if let Some(contours) = &stroke.contours {
        return tessellate(contours.clone(), vec![], vec![]);
    }
    if !stroke.corners.is_empty() {
        return build(&super::corners::rounded(stroke));
    }
    let original_points = samples(stroke);
    let mut points = original_points.clone();
    if points.len() < 2 || points.last().unwrap().distance < 0.001 {
        return Ok(Outline::default());
    }
    let profile = Profile::new(stroke, &points);
    let mut left = Vec::new();
    let mut right = Vec::new();
    points.clear();
    for p in &original_points {
        let w = profile.width(p.distance);
        let tangent = stroke.tangent(p.segment, p.t);
        let n = width_direction(tangent, w.angle);
        if p.t == 1. && (stroke.closed || p.segment + 2 < stroke.nodes.len()) {
            let next = stroke.tangent((p.segment + 1) % stroke.segment_count(), 0.);
            let other = width_direction(next, w.angle);
            let a = join_side(
                p.p,
                tangent,
                next,
                n * w.left,
                other * w.left,
                stroke.join,
                stroke.miter_limit,
            );
            let b = join_side(
                p.p,
                tangent,
                next,
                -n * w.right,
                -other * w.right,
                stroke.join,
                stroke.miter_limit,
            );
            let count = a.len().max(b.len());
            for i in 0..count {
                points.push(*p);
                left.push(a[i.min(a.len() - 1)]);
                right.push(b[i.min(b.len() - 1)]);
            }
        } else {
            points.push(*p);
            left.push(p.p + n * w.left);
            right.push(p.p - n * w.right);
        }
    }
    let (start_count, start_left, start_right) = cap_patch(stroke, &points, &left, &right, false);
    let (end_count, end_left, end_right) = cap_patch(stroke, &points, &left, &right, true);
    // Union the swept cross-sections. A folded inner offset must never subtract ink.
    let mut pieces = Vec::new();
    let mut add = |mut c: Vec<Point>| {
        let area: f64 = c
            .iter()
            .zip(c.iter().cycle().skip(1))
            .take(c.len())
            .map(|(a, b)| a.x * b.y - a.y * b.x)
            .sum();
        if area.abs() > 1e-12 {
            if area < 0. {
                c.reverse();
            }
            pieces.push(c);
        }
    };
    for i in start_count..points.len() - 1 - end_count {
        add(vec![left[i], right[i], right[i + 1]]);
        add(vec![left[i], right[i + 1], left[i + 1]]);
    }
    if start_count > 0 {
        let mut c = start_left.clone();
        c.extend(start_right.iter().rev().copied());
        add(c);
    }
    if end_count > 0 {
        let mut c = end_left.clone();
        c.extend(end_right.iter().rev().copied());
        add(c);
    }
    let assemble = |side: Vec<Point>, mut start: Vec<Point>, end: Vec<Point>| {
        let end_index = side.len() - 1 - end_count;
        if start_count < end_index {
            start.extend_from_slice(&side[start_count + 1..end_index]);
        }
        start.extend(end.into_iter().rev());
        start
    };
    left = assemble(left, start_left, end_left);
    right = assemble(right, start_right, end_right);
    // Round / square ends sit on the actual end cross-section (asymmetric
    // widths and rotated cuts included), bulging outward along the path.
    if !stroke.closed && stroke.nodes.len() >= 2 {
        let last_segment = stroke.segment_count().saturating_sub(1);
        for (cap, a, b, outward) in [
            (stroke.start_cap, left[0], right[0], -stroke.tangent(0, 0.)),
            (stroke.end_cap, *left.last().unwrap(), *right.last().unwrap(), stroke.tangent(last_segment, 1.)),
        ] {
            let m = a.midpoint(b);
            let r = a.distance(b) * 0.5;
            if cap == super::model::Cap::Butt || r < 1e-6 {
                continue;
            }
            let u = (a - m) / r;
            let mut t = outward - u * u.dot(outward);
            if t.hypot() < 1e-9 {
                t = Vec2::new(-u.y, u.x);
            }
            let t = t / t.hypot();
            match cap {
                super::model::Cap::Square => add(vec![a, b, b + t * r, a + t * r]),
                _ => {
                    let count = ((r * std::f64::consts::PI / 2.).ceil() as usize).clamp(8, 96);
                    add((0..=count)
                        .map(|k| {
                            let th = std::f64::consts::PI * k as f64 / count as f64;
                            m + (u * th.cos() + t * th.sin()) * r
                        })
                        .collect());
                }
            }
        }
    }
    let mut ring = left;
    ring.extend(right.into_iter().rev());
    let contours = union_all(pieces);
    tessellate(contours, original_points, ring)
}

/// Union many small overlapping pieces (the swept cross-sections) as a
/// tournament: neighbours first, then the merged results. One overlay of all
/// pieces at once gets very slow when a wide stroke folds over itself
/// (thousands of piled-up triangles intersect each other); merged neighbours
/// have already lost their inner edges.
fn union_all(pieces: Vec<Vec<Point>>) -> Vec<Vec<Point>> {
    use i_overlay::core::overlay_rule::OverlayRule;
    const CHUNK: usize = 48;
    if pieces.len() <= CHUNK {
        return boolean_contours(&pieces, &[], OverlayRule::Subject);
    }
    // Neighbouring chunks overlap by one cross-section (two triangles): each
    // overlay rounds its own result, so chunks that merely touched could
    // leave hairline slivers along the seam.
    let mut level: Vec<Vec<Vec<Point>>> = (0..pieces.len())
        .step_by(CHUNK)
        .map(|i| boolean_contours(&pieces[i..(i + CHUNK + 2).min(pieces.len())], &[], OverlayRule::Subject))
        .collect();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [a, b] => boolean_contours(a, b, OverlayRule::Union),
                [a] => a.clone(),
                _ => unreachable!(),
            })
            .collect();
    }
    level.pop().unwrap_or_default()
}

pub fn boolean_contours(
    a: &[Vec<Point>],
    b: &[Vec<Point>],
    rule: i_overlay::core::overlay_rule::OverlayRule,
) -> Vec<Vec<Point>> {
    use i_overlay::{core::fill_rule::FillRule, float::single::SingleFloatOverlay};
    let convert = |c: &[Vec<Point>]| {
        c.iter()
            .map(|r| r.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    convert(a)
        .overlay(&convert(b), rule, FillRule::NonZero)
        .into_iter()
        .flatten()
        .map(|c| c.into_iter().map(|p| Point::new(p[0], p[1])).collect())
        .collect()
}

/// Rings under a fill rule (an imported shape's nonzero / evenodd), as the
/// normalized contours a filled shape stores.
pub fn fill_rule_contours(rings: &[Vec<Point>], even_odd: bool) -> Vec<Vec<Point>> {
    use i_overlay::{core::{fill_rule::FillRule, overlay_rule::OverlayRule}, float::single::SingleFloatOverlay};
    let rings: Vec<Vec<[f64; 2]>> = rings.iter().filter(|r| r.len() > 2).map(|r| r.iter().map(|p| [p.x, p.y]).collect()).collect();
    let rule = if even_odd { FillRule::EvenOdd } else { FillRule::NonZero };
    rings
        .overlay(&Vec::<Vec<[f64; 2]>>::new(), OverlayRule::Subject, rule)
        .into_iter()
        .flatten()
        .map(|c| c.into_iter().map(|p| Point::new(p[0], p[1])).collect::<Vec<_>>())
        .filter(|c| c.len() > 2)
        .collect()
}

/// Like `boolean_contours`, but keeps each separate region (outer ring + its
/// holes) apart.
pub fn boolean_shapes(
    a: &[Vec<Point>],
    b: &[Vec<Point>],
    rule: i_overlay::core::overlay_rule::OverlayRule,
) -> Vec<Vec<Vec<Point>>> {
    use i_overlay::{core::fill_rule::FillRule, float::single::SingleFloatOverlay};
    let convert = |c: &[Vec<Point>]| {
        c.iter()
            .map(|r| r.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    convert(a)
        .overlay(&convert(b), rule, FillRule::NonZero)
        .into_iter()
        .map(|shape| shape.into_iter().map(|c| c.into_iter().map(|p| Point::new(p[0], p[1])).collect()).collect())
        .collect()
}

thread_local! {
    /// Set by `contours_only`: callers that only need contours (the web UI
    /// draws SVG paths) skip GPU triangulation, which dominated render time.
    static CONTOURS_ONLY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// Run `f` without triangulating outlines (vertices/indices stay empty).
pub fn contours_only<T>(f: impl FnOnce() -> T) -> T {
    let previous = CONTOURS_ONLY.with(|c| c.replace(true));
    let result = f();
    CONTOURS_ONLY.with(|c| c.set(previous));
    result
}

pub(super) fn tessellate(
    contours: Vec<Vec<Point>>,
    samples: Vec<Sample>,
    _ring: Vec<Point>,
) -> Result<Outline, String> {
    if CONTOURS_ONLY.with(|c| c.get()) {
        return Ok(Outline {samples, ring: _ring, contours, ..Outline::default()});
    }
    use lyon::math::point;
    use lyon::path::Path;
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    };
    let mut builder = Path::builder();
    for c in &contours {
        if c.len() < 3 {
            continue;
        }
        builder.begin(point(c[0].x as f32, c[0].y as f32));
        for p in &c[1..] {
            builder.line_to(point(p.x as f32, p.y as f32));
        }
        builder.close();
    }
    let mut buffers: VertexBuffers<Point, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &builder.build(),
            &FillOptions::default().with_fill_rule(FillRule::NonZero),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                Point::new(v.position().x as f64, v.position().y as f64)
            }),
        )
        .map_err(|e| format!("輪郭を生成できません: {e:?}"))?;
    let boundary = contours
        .iter()
        .flat_map(|c| {
            c.iter()
                .zip(c.iter().cycle().skip(1))
                .take(c.len())
                .map(|(a, b)| [*a, *b])
        })
        .collect();
    Ok(Outline {
        layers: vec![],
        boundary,
        samples,
        contours,
        ring: _ring,
        vertices: buffers.vertices,
        indices: buffers.indices,
    })
}

/// Resolve the path interior and its own stroke separately, then union only for hit tests.
pub fn paint(s: &Stroke) -> Result<Outline, String> {
    let center = if s.corners.is_empty() {
        s.clone()
    } else {
        super::corners::rounded(s)
    };
    let layers = paint_layers(s)?;
    let contours = layers
        .iter()
        .flat_map(|l| l.outline.contours.clone())
        .collect::<Vec<_>>();
    let mut result = tessellate(
        boolean_contours(
            &contours,
            &[],
            i_overlay::core::overlay_rule::OverlayRule::Subject,
        ),
        samples(&center),
        vec![],
    )?;
    result.layers = layers;
    Ok(result)
}

/// The painted layers (fill, stroke, then the appearance graph) without the
/// union used for hit testing.
pub fn paint_layers(s: &Stroke) -> Result<Vec<PaintLayer>, String> {
    let mut layers = Vec::new();
    let center = if s.corners.is_empty() {
        s.clone()
    } else {
        super::corners::rounded(s)
    };
    if let Some(color) = s.fill {
        let contours = if let Some(c) = &s.contours {
            c.clone()
        } else {
            let mut points = samples(&center).iter().map(|p| p.p).collect::<Vec<_>>();
            if points.len() > 1 && points.first() == points.last() {
                points.pop();
            }
            boolean_contours(
                &[points],
                &[],
                i_overlay::core::overlay_rule::OverlayRule::Subject,
            )
        };
        layers.push(PaintLayer {
            color,
            opacity: 1.,
            outline: tessellate(contours, vec![], vec![])?,
            image: None,
        });
    }
    if let Some(color) = s.edge {
        let outline = if let Some(contours) = &s.contours {
            let mut rings = vec![];
            for c in contours {
                let mut path = Stroke::new(0, String::new());
                path.closed = true;
                path.join = s.join;
                path.miter_limit = s.miter_limit;
                path.set_line_width(s.edge_width);
                path.nodes = c
                    .iter()
                    .enumerate()
                    .map(|(i, p)| super::model::Node {
                        id: i as u64 + 1,
                        p: *p,
                        incoming: Vec2::ZERO,
                        outgoing: Vec2::ZERO,
                        smooth: false,
                        exact_incoming: false,
                        exact_outgoing: false,
                    })
                    .collect();
                rings.extend(build(&path)?.contours);
            }
            tessellate(
                boolean_contours(
                    &rings,
                    &[],
                    i_overlay::core::overlay_rule::OverlayRule::Subject,
                ),
                vec![],
                vec![],
            )?
        } else {
            build(s)?
        };
        layers.push(PaintLayer { color, opacity: 1., outline, image: None });
    }
    if let Some(graph) = &s.appearance {
        let original = layers.iter().map(|layer| super::appearance::Layer {
            color: layer.color, opacity: layer.opacity, contours: layer.outline.contours.clone(), image: layer.image.clone(),
        }).collect::<Vec<_>>();
        layers = super::appearance::evaluate(graph, &original)?.into_iter().map(|layer| {
            Ok(PaintLayer { color: layer.color, opacity: layer.opacity, outline: tessellate(layer.contours, vec![], vec![])?, image: layer.image })
        }).collect::<Result<Vec<_>, String>>()?;
    }
    Ok(layers)
}

pub fn nearest(stroke: &Stroke, p: Point) -> Option<(usize, f64, f64)> {
    let samples = samples(stroke);
    let mut best = (0, 0., f64::INFINITY);
    for pair in samples.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let v = b.p - a.p;
        let t = ((p - a.p).dot(v) / v.hypot2().max(1e-12)).clamp(0., 1.);
        let d = p.distance(a.p.lerp(b.p, t));
        if d < best.2 {
            let q = (a.segment as f64 + a.t) * (1. - t) + (b.segment as f64 + b.t) * t;
            let i = (q.floor() as usize).min(stroke.segment_count() - 1);
            best = (i, q - i as f64, d);
        }
    }
    if best.2.is_finite() {
        Some(best)
    } else {
        None
    }
}

/// Evaluate the same arc-length profile used to render the stroke.
pub fn width_at(s: &Stroke, segment: usize, t: f64) -> Option<Width> {
    if segment >= s.segment_count() || !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return None;
    }
    let points = samples(s);
    Some(Profile::new(s, &points).width(at_distance(&points, segment, t)))
}

/// Insert a handle without changing any interpolation knots. Promoting the
/// handle in Stroke::width_mut happens only when the user actually edits it.
pub fn insert_width_key(s: &mut Stroke, id: u64, segment: usize, t: f64) -> bool {
    let Some(width) = width_at(s, segment, t) else { return false; };
    s.widths.push(super::model::WidthKey {
        id,
        segment: s.nodes[segment].id,
        t,
        width,
        interpolated: true,
    });
    true
}

pub fn width_location(s: &Stroke, target: WidthTarget) -> Option<(Point, Vec2)> {
    let (i, t) = s.attachment(target)?;
    Some((
        s.eval(i, t),
        width_direction(s.tangent(i, t), s.width(target)?.angle),
    ))
}

pub fn svg(doc: &Document) -> Result<String, String> {
    svg_frame(doc, None, None, 16.)
}

/// SVG of the drawing seen through `frame` (an artboard: x, y, w, h; `None` =
/// everything painted, effects included, plus `margin`), at its size in
/// units, optionally over a solid `background`.
pub fn svg_frame(doc: &Document, frame: Option<[f64; 4]>, background: Option<[u8; 3]>, margin: f64) -> Result<String, String> {
    // Export the normalized contours shared with the canvas and expansion.
    let mut paths = String::new();
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    // Compound paths paint as one shape (holes included) at their place.
    for s in super::compound::painted(doc) {
        let s = &*s;
        if let Some(img) = &s.image {
            // Placed bitmap: kept as one <image> with its placement; a 下絵
            // (tracing reference) is never exported.
            // 抜き: the caller hands in the keyed copy next to the original.
            let keyed = img.knockout.and_then(|k| doc.assets.get(&crate::model::knockout_asset(&img.asset, k)));
            let Some(href) = keyed.or_else(|| doc.assets.get(&img.asset)) else { continue };
            if img.reference || img.opacity <= 0. {
                continue;
            }
            for p in img.rect() {
                min.x = min.x.min(p.x);
                min.y = min.y.min(p.y);
                max.x = max.x.max(p.x);
                max.y = max.y.max(p.y);
            }
            let m = img.matrix;
            paths.push_str(&format!(
                "  <image width=\"{:.4}\" height=\"{:.4}\" opacity=\"{:.6}\" preserveAspectRatio=\"none\" transform=\"matrix({:.6} {:.6} {:.6} {:.6} {:.4} {:.4})\" href=\"{}\"/>\n",
                img.width, img.height, img.opacity, m[0], m[1], m[2], m[3], m[4], m[5], href
            ));
            continue;
        }
        let rendered = paint(s)?;
        for layer in &rendered.layers {
            if layer.opacity <= 0. { continue; }
            if let Some(img) = &layer.image {
                min.x = min.x.min(img.x0);
                min.y = min.y.min(img.y0);
                max.x = max.x.max(img.x1());
                max.y = max.y.max(img.y1());
                paths.push_str(&format!(
                    "  <image x=\"{:.4}\" y=\"{:.4}\" width=\"{:.4}\" height=\"{:.4}\" opacity=\"{:.6}\" preserveAspectRatio=\"none\" href=\"{}\"/>\n",
                    img.x0, img.y0, img.x1() - img.x0, img.y1() - img.y0, layer.opacity.clamp(0., 1.), img.data_url()
                ));
                continue;
            }
            let [r, g, b] = layer.color;
            paths.push_str(&format!(
                "  <path fill=\"#{r:02x}{g:02x}{b:02x}\" fill-opacity=\"{:.6}\" fill-rule=\"nonzero\" d=\"", layer.opacity.clamp(0., 1.)
            ));
            for contour in &layer.outline.contours {
                for (i, p) in contour.iter().enumerate() {
                    min.x = min.x.min(p.x);
                    min.y = min.y.min(p.y);
                    max.x = max.x.max(p.x);
                    max.y = max.y.max(p.y);
                    paths.push_str(&format!(
                        "{} {:.4} {:.4} ",
                        if i == 0 { "M" } else { "L" },
                        p.x,
                        p.y
                    ));
                }
                paths.push_str("Z ");
            }
            paths.push_str("\"/>\n");
        }
    }
    if !min.x.is_finite() {
        min = Point::ZERO;
        max = Point::new(100., 100.);
    }
    let m = margin.max(0.);
    let [x, y, w, h] = frame.unwrap_or([min.x - m, min.y - m, max.x - min.x + 2. * m, max.y - min.y + 2. * m]);
    if !(w > 0. && h > 0.) {
        return Err("書き出す範囲が空です".into());
    }
    let bg = background.map_or(String::new(), |[r, g, b]| format!("  <rect x=\"{x:.4}\" y=\"{y:.4}\" width=\"{w:.4}\" height=\"{h:.4}\" fill=\"#{r:02x}{g:02x}{b:02x}\"/>\n"));
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.4}\" height=\"{h:.4}\" viewBox=\"{x:.4} {y:.4} {w:.4} {h:.4}\">\n{bg}{paths}</svg>\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, WidthKey};
    #[test]
    fn fill_is_path_interior_and_stroke_is_centerline_band() {
        let mut d = line();
        let s = &mut d.strokes[0];
        s.nodes[1].outgoing = Vec2::ZERO;
        let mut third = s.nodes[1].clone();
        third.id = 100;
        third.p = Point::new(200., 200.);
        s.nodes.push(third);
        s.closed = true;
        s.fill = None;
        s.edge = Some([255, 100, 0]);
        s.set_line_width(10.);
        let stroke = paint(s).unwrap();
        assert_eq!(stroke.layers.len(), 1);
        assert!(filled(&stroke, Point::new(100., 2.)));
        assert!(!filled(&stroke, Point::new(150., 50.)));
        s.fill = Some([100, 100, 100]);
        let both = paint(s).unwrap();
        assert_eq!(both.layers.len(), 2);
        assert!(filled(&both, Point::new(150., 50.)));
        s.edge = None;
        let fill = paint(s).unwrap();
        assert!(!filled(&fill, Point::new(100., -2.)));
        assert!(filled(&fill, Point::new(150., 50.)));
        s.closed = false;
        assert!(filled(&paint(s).unwrap(), Point::new(150., 50.))); // open fill closes with a straight edge
        s.fill = None;
        s.edge = Some([255, 100, 0]);
        s.nodes[1].incoming = Vec2::new(-100., -100.);
        let first = paint(s).unwrap();
        s.set_line_width(30.);
        let second = paint(s).unwrap();
        assert!(second.vertices.iter().any(|p| p.y < -40.));
        assert_ne!(first.contours, second.contours);
    }
    #[test]
    fn version_two_band_colors_migrate_without_losing_old_border() {
        let mut d = line();
        d.version = 2;
        d.strokes[0].fill = Some([30, 40, 50]);
        d.strokes[0].edge = Some([80, 90, 100]);
        d.strokes[0].edge_width = 3.;
        d.upgrade().unwrap();
        assert_eq!(d.version, 3);
        assert_eq!(d.strokes.len(), 2);
        assert_eq!(d.strokes[0].fill, None);
        assert_eq!(d.strokes[0].edge, Some([30, 40, 50]));
        assert!(d.strokes[1].contours.is_some());
        assert_eq!(d.strokes[1].edge, Some([80, 90, 100]));
        d.validate().unwrap();
        let again = d.clone();
        d.upgrade().unwrap();
        assert_eq!(d, again);
    }
    #[test]
    fn round_and_square_caps_extend_the_ends() {
        let area = |c: &[Vec<Point>]| c.iter().map(|r| r.iter().zip(r.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f64>() / 2.).sum::<f64>().abs();
        let mut d = line();
        let s = &mut d.strokes[0];
        s.start_cap_locked = false;
        s.end_cap_locked = false;
        s.set_line_width(24.);
        let butt = build(s).unwrap().contours;
        let xs = |c: &[Vec<Point>]| c.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        assert!((xs(&butt).0 - 0.).abs() < 1e-6 && (xs(&butt).1 - 200.).abs() < 1e-6);
        s.start_cap = crate::model::Cap::Round;
        s.end_cap = crate::model::Cap::Round;
        let round = build(s).unwrap().contours;
        assert!((xs(&round).0 + 12.).abs() < 0.05 && (xs(&round).1 - 212.).abs() < 0.05);
        assert!((area(&round) - area(&butt) - std::f64::consts::PI * 144.).abs() < 6.);
        s.start_cap = crate::model::Cap::Square;
        s.end_cap = crate::model::Cap::Butt;
        let square = build(s).unwrap().contours;
        assert!((xs(&square).0 + 12.).abs() < 1e-6 && (xs(&square).1 - 200.).abs() < 1e-6);
        assert!((area(&square) - area(&butt) - 24. * 12.).abs() < 1e-3);
        // saved and loaded
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"start_cap\":\"Square\""));
        let back: crate::model::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.strokes[0].start_cap, crate::model::Cap::Square);
    }
    #[test]
    fn corner_between_two_curves_stays_a_corner() {
        // Pen: drag (curve) → click (corner) → drag (curve) on one line.
        let mut d = line();
        let s = &mut d.strokes[0];
        s.nodes[0].outgoing = Vec2::new(60., -80.);
        let mut c = s.nodes[1].clone();
        c.id = 99;
        c.p = Point::new(400., 0.);
        c.incoming = Vec2::new(-60., -80.);
        s.nodes.push(c);
        let arrive = s.tangent(0, 1.);
        let leave = s.tangent(1, 0.);
        assert!(arrive.dot(leave) < 0.99, "corner was smoothed: {arrive:?} {leave:?}");
        assert!(build(s).is_ok());
    }
    #[test]
    fn corner_directions_and_subdivision_are_consistent() {
        for which in 0..3 {
            let mut d = line();
            let s = &mut d.strokes[0];
            if which == 1 {
                s.nodes[0].outgoing = Vec2::new(40., 120.);
            }
            if which == 2 {
                s.nodes[1].incoming = Vec2::new(-80., -90.);
            }
            // Standard Bézier: a collapsed handle aims the end tangent at the
            // other segment's control point (Illustrator/SVG behaviour).
            let unit = |v: Vec2| v / v.hypot();
            let start = if which == 2 { unit(Vec2::new(120., -90.)) } else if which == 1 { unit(Vec2::new(40., 120.)) } else { Vec2::new(1., 0.) };
            let end = if which == 1 { unit(Vec2::new(160., -120.)) } else if which == 2 { unit(Vec2::new(80., 90.)) } else { Vec2::new(1., 0.) };
            assert!((s.tangent(0, 0.) - start).hypot() < 1e-6, "{which} start {:?}", s.tangent(0, 0.));
            assert!((s.tangent(0, 1.) - end).hypot() < 1e-6, "{which} end {:?}", s.tangent(0, 1.));
            let old = s.clone();
            s.split(0, 0.37, 900);
            for j in 0..=100 {
                let t = j as f64 / 100.;
                let actual = if t <= 0.37 {
                    s.eval(0, t / 0.37)
                } else {
                    s.eval(1, (t - 0.37) / 0.63)
                };
                assert!(
                    actual.distance(old.eval(0, t)) < 1e-7,
                    "split changed curve: {which} at {t}"
                );
            }
        }
    }
    #[test]
    fn folded_ribbon_keeps_cross_sections_filled() {
        let mut d = Document::default();
        let mut s = Stroke::new(d.id(), "fold".into());
        for (x, y, ix, iy, ox, oy) in [
            (95., 334., 0., 0., 115., 15.),
            (342., 165., -75., -4., 305., 15.),
            (485., 394., 140., -225., 95., -125.),
            (859., 587., -160., -295., 0., 0.),
        ] {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: d.id(),
                p: Point::new(x, y),
                incoming: Vec2::new(ix, iy),
                outgoing: Vec2::new(ox, oy),
                smooth: false,
            });
        }
        s.start_width.left = 65.;
        s.start_width.right = 100.;
        s.end_width.left = 32.;
        s.end_width.right = 32.;
        let o = build(&s).unwrap();
        let profile = Profile::new(&s, &o.samples);
        for sample in o
            .samples
            .iter()
            .skip(5)
            .take(o.samples.len() - 10)
            .step_by(3)
        {
            let w = profile.width(sample.distance);
            let n = width_direction(s.tangent(sample.segment, sample.t), w.angle);
            for fraction in [-0.8, -0.4, 0., 0.4, 0.8] {
                let p = sample.p + n * fraction * if fraction < 0. { w.right } else { w.left };
                assert!(filled(&o, p), "missing ink at {p:?}");
            }
        }
        if let Ok(path) = std::env::var("PATHWEAVER_FOLD_FIXTURE") {
            std::fs::write(path,serde_json::to_string(&serde_json::json!({"vertices":o.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":o.indices,"contours":o.contours.iter().map(|c|c.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()).collect::<Vec<_>>()})).unwrap()).unwrap();
        }
    }
    #[test]
    fn boolean_operations_preserve_holes_and_disconnected_components() {
        use i_overlay::core::overlay_rule::OverlayRule::*;
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            vec![
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + h),
                Point::new(x, y + h),
            ]
        };
        let a = vec![rect(0., 0., 100., 100.)];
        let b = vec![rect(25., 25., 50., 50.)];
        let cut = boolean_contours(&a, &b, Difference);
        assert_eq!(cut.len(), 2);
        let o = tessellate(cut, vec![], vec![]).unwrap();
        assert!(filled(&o, Point::new(10., 10.)));
        assert!(!filled(&o, Point::new(50., 50.)));
        for (op, outer, inner) in [
            (Union, true, true),
            (Intersect, false, true),
            (Xor, true, false),
        ] {
            let o = tessellate(boolean_contours(&a, &b, op), vec![], vec![]).unwrap();
            assert_eq!(filled(&o, Point::new(10., 10.)), outer);
            assert_eq!(filled(&o, Point::new(50., 50.)), inner);
        }
        assert_eq!(
            boolean_contours(&a, &[rect(200., 0., 100., 100.)], Union).len(),
            2
        );
    }
    fn line() -> Document {
        let mut d = Document::default();
        let mut s = Stroke::new(d.id(), "line".into());
        s.start_cap_locked = true;
        s.end_cap_locked = true;
        for x in [0., 200.] {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: d.id(),
                p: Point::new(x, 0.),
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
            });
        }
        d.strokes.push(s);
        d
    }
    #[test]
    fn corner_styles_and_miter_limit_have_distinct_correct_coverage() {
        let mut d = line();
        let id = d.id();
        let s = &mut d.strokes[0];
        s.nodes[0].p = Point::new(100., 100.);
        s.nodes[1].p = Point::new(300., 100.);
        let mut end = s.nodes[1].clone();
        end.id = id;
        end.p = Point::new(300., 300.);
        s.nodes.push(end);
        s.end_angle = 0.;
        for style in [Join::Miter, Join::Round, Join::Bevel] {
            s.join = style;
            let o = build(s).unwrap();
            assert_eq!(filled(&o, Point::new(320., 80.)), style == Join::Miter);
            assert_eq!(filled(&o, Point::new(314., 86.)), style != Join::Bevel);
            assert!(filled(&o, Point::new(280., 110.)), "inside join {style:?}");
            assert!(!filled(&o, Point::new(270., 130.)));
        }
        s.join = Join::Miter;
        s.miter_limit = 1.;
        assert!(!filled(&build(s).unwrap(), Point::new(320., 80.)));
        s.miter_limit = 10.;
        let restored: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(restored, d);
        if let Ok(path) = std::env::var("PATHWEAVER_JOIN_FIXTURE") {
            let mut fixtures = Vec::new();
            for join in [Join::Miter, Join::Round, Join::Bevel] {
                d.strokes[0].join = join;
                let o = build(&d.strokes[0]).unwrap();
                fixtures.push(serde_json::json!({"join": join, "vertices": o.vertices.iter().map(|p| [p.x,p.y]).collect::<Vec<_>>(), "indices": o.indices}));
            }
            std::fs::write(path, serde_json::to_string(&fixtures).unwrap()).unwrap();
        }
    }
    #[test]
    fn joins_support_both_turn_directions_and_legacy_documents() {
        for sign in [-1., 1.] {
            let incoming = Vec2::new(1., 0.);
            let outgoing = Vec2::new(0., sign);
            let a = Vec2::new(0., -sign * 12.);
            let b = Vec2::new(12., 0.);
            let miter = join_side(Point::ZERO, incoming, outgoing, a, b, Join::Miter, 10.);
            assert!(miter.contains(&Point::new(12., -sign * 12.)));
            let round = join_side(Point::ZERO, incoming, outgoing, a, b, Join::Round, 10.);
            assert!(round
                .iter()
                .all(|p| (p.distance(Point::ZERO) - 12.).abs() < 1e-8));
        }
        let mut value = serde_json::to_value(line()).unwrap();
        let s = value["strokes"][0].as_object_mut().unwrap();
        s.remove("join");
        s.remove("miter_limit");
        let mut d: Document = serde_json::from_value(value).unwrap();
        assert_eq!(d.strokes[0].join, Join::Miter);
        assert_eq!(d.strokes[0].miter_limit, 10.);
        d.strokes[0].miter_limit = 0.;
        assert!(d.validate().is_err());
    }
    #[test]
    fn ordinary_caps_preserve_full_width_without_custom_clipping() {
        assert!(!Stroke::new(1, "new".into()).start_cap_locked);
        assert!(!Stroke::new(1, "new".into()).end_cap_locked);
        let mut d = extreme_curve(0);
        let s = &mut d.strokes[0];
        s.start_cap_locked = false;
        s.end_cap_locked = false;
        let o = build(s).unwrap();
        let n = normal(s.tangent(0, 0.));
        assert!(o.ring[0].distance(s.nodes[0].p + n * s.start_width.left) < 1e-8);
        assert!(
            o.ring
                .last()
                .unwrap()
                .distance(s.nodes[0].p - n * s.start_width.right)
                < 1e-8
        );
        let original = o.ring;
        s.start_angle = 0.;
        s.end_angle = 135.;
        assert_eq!(build(s).unwrap().ring, original);
        s.start_cap_locked = true;
        assert_ne!(build(s).unwrap().ring, original);
        s.start_cap_locked = false;
        assert_eq!(build(s).unwrap().ring, original);
    }
    #[test]
    fn diagonal_caps_do_not_taper_normal_length_paths() {
        let mut d = line();
        d.strokes[0].start_angle = 45.;
        let o = build(&d.strokes[0]).unwrap();
        assert!((o.ring[0].distance(*o.ring.last().unwrap()) - 48. * 2_f64.sqrt()).abs() < 1e-6);
        let mut d = extreme_curve(0);
        d.strokes[0].start_angle = 45.;
        let o = build(&d.strokes[0]).unwrap();
        assert!(o.ring[0].distance(*o.ring.last().unwrap()) >= 48. - 1e-6);
        assert_eq!(proper_intersections(&o.ring), 0);
        if let Ok(path) = std::env::var("PATHWEAVER_DIAGONAL_FIXTURE") {
            let mut d = line();
            let s = &mut d.strokes[0];
            s.nodes[0].p = Point::new(40., 80.);
            s.nodes[0].outgoing = Vec2::new(100., 0.);
            s.nodes[1].p = Point::new(240., 130.);
            s.nodes[1].incoming = Vec2::new(-40., -80.);
            s.end_angle = 45.;
            s.start_cap_locked = false;
            let o = build(s).unwrap();
            std::fs::write(path,serde_json::to_string(&serde_json::json!({"vertices":o.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":o.indices})).unwrap()).unwrap();
        }
    }
    fn filled(o: &Outline, p: Point) -> bool {
        o.indices.as_chunks::<3>().0.iter().any(|t| {
            let a = o.vertices[t[0] as usize];
            let b = o.vertices[t[1] as usize];
            let c = o.vertices[t[2] as usize];
            let x = cross(b - a, p - a);
            let y = cross(c - b, p - b);
            let z = cross(a - c, p - c);
            (x >= 0. && y >= 0. && z >= 0.) || (x <= 0. && y <= 0. && z <= 0.)
        })
    }
    fn extreme_curve(kind: usize) -> Document {
        let mut d = line();
        let s = &mut d.strokes[0];
        s.nodes[0].p = Point::new(150., 180.);
        s.nodes[0].outgoing = Vec2::new(-5., -70.);
        s.nodes[1].p = Point::new(410., 280.);
        s.nodes[1].incoming = Vec2::new(-110., -100.);
        s.start_angle = 90.;
        s.end_angle = 90.;
        if kind == 1 {
            s.nodes[0].p = Point::new(80., 210.);
            s.nodes[0].outgoing = Vec2::new(130., -220.);
            s.nodes[1].p = Point::new(200., 350.);
            s.nodes[1].incoming = Vec2::new(-80., -40.);
        }
        d
    }
    fn proper_intersections(ring: &[Point]) -> usize {
        let mut count = 0;
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            for j in i + 2..ring.len() {
                if i == 0 && j + 1 == ring.len() {
                    continue;
                }
                let c = ring[j];
                let d = ring[(j + 1) % ring.len()];
                if cross(b - a, c - a) * cross(b - a, d - a) < -1e-8
                    && cross(d - c, a - c) * cross(d - c, b - c) < -1e-8
                {
                    count += 1;
                }
            }
        }
        count
    }
    #[test]
    fn changing_one_cap_does_not_rebuild_the_other_cap() {
        let mut d = extreme_curve(0);
        let before = build(&d.strokes[0]).unwrap();
        d.strokes[0].start_angle = 15.;
        let after = build(&d.strokes[0]).unwrap();
        let endpoint = d.strokes[0].nodes[1].p;
        let near_end = |p: &&Point| p.distance(endpoint) < 40.;
        let a: Vec<_> = before.ring.iter().filter(near_end).collect();
        let b: Vec<_> = after.ring.iter().filter(near_end).collect();
        assert_eq!(a, b);
    }
    #[test]
    fn tiny_path_cap_regions_remain_disjoint_and_finite() {
        for length in [0.01, 0.1, 1., 5.] {
            let mut d = line();
            d.strokes[0].nodes[1].p.x = length;
            d.strokes[0].start_angle = 0.;
            d.strokes[0].end_angle = 15.;
            let o = build(&d.strokes[0]).unwrap();
            assert!(!o.indices.is_empty());
            assert_eq!(proper_intersections(&o.ring), 0);
            assert!(o.ring.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        }
    }
    #[test]
    fn extreme_caps_have_no_terminal_loops_or_remote_clipping() {
        let mut fixtures = vec![];
        for kind in 0..2 {
            for angle in [0., 15., 45., 75., 89., 90., 91., 105., 135., 165.] {
                let mut d = extreme_curve(kind);
                d.strokes[0].start_angle = angle;
                d.strokes[0].end_angle = 180. - angle;
                if angle == 0. {
                    d.strokes[0].end_angle = 0.;
                }
                let s = &d.strokes[0];
                let o = build(s).unwrap();
                let profile = Profile::new(s, &o.samples);
                let mut raw_left = vec![];
                let mut raw_right = vec![];
                for p in &o.samples {
                    let w = profile.width(p.distance);
                    let n = width_direction(s.tangent(p.segment, p.t), w.angle);
                    raw_left.push(p.p + n * w.left);
                    raw_right.push(p.p - n * w.right);
                }
                let mut raw = raw_left;
                raw.extend(raw_right.into_iter().rev());
                if kind == 0 {
                    assert_eq!(proper_intersections(&o.ring), 0, "cap loop at {angle}");
                }
                // Preserve distant geometry, including any pre-existing offset
                // cusp; cap treatment must not add another crossing.
                assert!(
                    proper_intersections(&o.ring) <= proper_intersections(&raw),
                    "new cap crossing kind={kind} angle={angle}"
                );
                let total = o.samples.last().unwrap().distance;
                for sample in o
                    .samples
                    .iter()
                    .filter(|p| p.distance > total * 0.21 && p.distance < total * 0.79)
                {
                    let w = profile.width(sample.distance);
                    let n = width_direction(s.tangent(sample.segment, sample.t), w.angle);
                    for p in [sample.p + n * w.left, sample.p - n * w.right] {
                        assert!(
                            o.ring.iter().any(|q| q.distance(p) < 1e-8),
                            "remote contour lost"
                        );
                    }
                }
                if angle == 90. {
                    fixtures.push(serde_json::json!({"kind":kind,"vertices":o.vertices.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>(),"indices":o.indices,"ring":o.ring.iter().map(|p|[p.x,p.y]).collect::<Vec<_>>()}));
                }
            }
        }
        if let Ok(path) = std::env::var("PATHWEAVER_CAP_FIXTURE") {
            std::fs::write(path, serde_json::to_vec(&fixtures).unwrap()).unwrap();
        }
    }
    #[test]
    fn antialias_edges_border_ink_not_internal_triangle_seams() {
        let d = Document::example();
        let o = build(&d.strokes[0]).unwrap();
        assert!(!o.boundary.is_empty());
        for [a, b] in &o.boundary {
            let v = *b - *a;
            if v.hypot() < 0.1 {
                continue;
            }
            let n = normal(v / v.hypot()) * 0.01;
            let mid = a.lerp(*b, 0.5);
            assert_ne!(
                filled(&o, mid + n),
                filled(&o, mid - n),
                "internal edge {a:?} {b:?}"
            );
        }
    }
    #[test]
    fn loop_keeps_counter_open_and_crossing_keeps_ink() {
        let mut d = line();
        let s = &mut d.strokes[0];
        s.nodes.clear();
        // Open path with coincident endpoints, forming an O using four cubics.
        let k = 55.228474983;
        for (i, (x, y, ix, iy, ox, oy)) in [
            (100., 0., 0., -k, 0., k),
            (0., 100., k, 0., -k, 0.),
            (-100., 0., 0., k, 0., -k),
            (0., -100., -k, 0., k, 0.),
            (100., 0., 0., -k, 0., k),
        ]
        .into_iter()
        .enumerate()
        {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: i as u64 + 2,
                p: Point::new(x, y),
                incoming: Vec2::new(ix, iy),
                outgoing: Vec2::new(ox, oy),
                smooth: true,
            });
        }
        let o = build(s).unwrap();
        assert!(!filled(&o, Point::ZERO));
        assert!(filled(&o, Point::new(0., 100.)));
        s.nodes.clear();
        for (i, (x, y)) in [(0., 0.), (160., 160.), (0., 160.), (160., 0.)]
            .into_iter()
            .enumerate()
        {
            s.nodes.push(Node {
                exact_incoming: false,
                exact_outgoing: false,
                id: i as u64 + 2,
                p: Point::new(x, y),
                incoming: Vec2::ZERO,
                outgoing: Vec2::ZERO,
                smooth: false,
            });
        }
        let o = build(s).unwrap();
        assert!(filled(&o, Point::new(80., 80.)));
    }
    #[test]
    fn overlapping_caps_remain_finite_without_resetting_angles() {
        let mut d = line();
        let s = &mut d.strokes[0];
        s.nodes[1].p.x = 5.;
        s.start_angle = 75.;
        s.start_cap_locked = true;
        s.end_angle = 105.;
        s.end_cap_locked = true;
        assert!(build(s).is_ok());
    }
    #[test]
    fn caps_are_flat_at_all_supported_angles() {
        for angle in (0..180).step_by(15) {
            let mut d = line();
            let s = &mut d.strokes[0];
            s.start_angle = angle as f64;
            s.start_cap_locked = true;
            s.end_angle = (180 - angle) as f64 % 180.;
            s.end_cap_locked = true;
            let o = build(s).unwrap();
            assert!(o.ring.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
            assert!(cross(o.ring[0] - s.nodes[0].p, cap_direction(s, false)).abs() < 1e-6);
            assert!(
                cross(
                    *o.ring.last().unwrap() - s.nodes[0].p,
                    cap_direction(s, false)
                )
                .abs()
                    < 1e-6
            );
            assert!(!o.indices.is_empty());
        }
    }
    #[test]
    fn interpolation_is_nonnegative_and_has_no_overshoot() {
        let keys = [(0., 0.), (15., 100.), (16., 2.), (100., 40.)];
        for i in 0..10000 {
            let x = i as f64 / 100.;
            let j = keys.partition_point(|k| k.0 < x).saturating_sub(1).min(2);
            let v = interpolate(&keys, x);
            assert!(
                v >= keys[j].1.min(keys[j + 1].1) - 1e-8
                    && v <= keys[j].1.max(keys[j + 1].1) + 1e-8
            );
        }
    }
    #[test]
    fn split_preserves_curve_and_width_key_positions() {
        let mut d = Document::example();
        let id = d.id();
        let s = &mut d.strokes[0];
        let before = s.clone();
        let key = before.eval(0, before.widths[0].t);
        s.split(0, 0.37, id);
        let k = &s.widths[0];
        assert!(
            s.eval(s.segment_index(k.segment).unwrap(), k.t)
                .distance(key)
                < 1e-8
        );
        for j in 0..101 {
            let t = j as f64 / 100.;
            let (i, u) = if t < 0.37 {
                (0, t / 0.37)
            } else {
                (1, (t - 0.37) / 0.63)
            };
            assert!(s.eval(i, u).distance(before.eval(0, t)) < 1e-8);
        }
    }
    #[test]
    fn asymmetric_width_does_not_move_centerline() {
        let mut d = line();
        let id = d.id();
        let s = &mut d.strokes[0];
        let p = s.eval(0, 0.5);
        s.widths.push(WidthKey {
            interpolated: false,
            id,
            segment: s.nodes[0].id,
            t: 0.5,
            width: Width {
                left: 60.,
                right: 5.,
                linked: false,
                angle: 0.0,
            },
        });
        let pts = samples(s);
        let w = Profile::new(s, &pts).width(at_distance(&pts, 0, 0.5));
        assert!((w.left - 60.).abs() < 1e-8 && (w.right - 5.).abs() < 1e-8);
        assert_eq!(s.eval(0, 0.5), p);
    }
    #[test]
    fn inserting_width_handles_preserves_entire_interpolated_profile() {
        let mut d = Document::example();
        let a = d.id();
        let b = d.id();
        let s = &mut d.strokes[0];
        s.start_width.angle = 10.;
        s.widths[0].width.angle = -35.;
        let before = s.clone();
        assert!(insert_width_key(s, a, 0, 0.21));
        assert!(insert_width_key(s, b, 1, 0.67));
        for segment in 0..s.segment_count() {
            for j in 0..101 {
                let t = j as f64 / 100.;
                assert_eq!(width_at(s, segment, t), width_at(&before, segment, t));
            }
        }
        let points = samples(s);
        let old = Profile::new(&before, &points);
        let new = Profile::new(s, &points);
        for point in &points {
            assert_eq!(old.width(point.distance), new.width(point.distance));
        }
        d.validate().unwrap();
        let restored: Document = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
        assert_eq!(restored, d);
        assert_eq!(svg(&restored).unwrap(), svg(&d).unwrap());
    }
    #[test]
    fn sampled_width_handles_follow_edits_until_promoted() {
        let mut d = Document::example();
        let a = d.id();
        let b = d.id();
        let s = &mut d.strokes[0];
        insert_width_key(s, a, 0, 0.2);
        insert_width_key(s, b, 0, 0.7);
        s.start_width.left = 2.;
        let current = width_at(s, 0, 0.2).unwrap();
        assert_eq!(s.width(WidthTarget::Key(a)), Some(current));
        let w = s.width_mut(WidthTarget::Key(a)).unwrap();
        assert_eq!(*w, current);
        w.left += 10.;
        assert!(!s.widths.iter().find(|k| k.id == a).unwrap().interpolated);
        assert!(s.widths.iter().find(|k| k.id == b).unwrap().interpolated);
        assert_eq!(s.width(WidthTarget::Key(b)), width_at(s, 0, 0.7));
        let changed = s.clone();
        let moved = s.transformed(kurbo::Affine::translate((20., 30.)), true);
        assert!(moved.widths.iter().find(|k| k.id == b).unwrap().interpolated);
        for j in 0..101 {
            let t = j as f64 / 100.;
            let a = width_at(&changed, 0, t).unwrap();
            let b = width_at(&moved, 0, t).unwrap();
            assert!((a.left - b.left).abs() < 1e-8);
            assert!((a.right - b.right).abs() < 1e-8);
        }
        d.validate().unwrap();
    }
    #[test]
    fn resizing_width_preserves_asymmetry_tapers_and_sampled_handles() {
        let mut d = Document::example();
        let id = d.id();
        let s = &mut d.strokes[0];
        s.start_width.left = 0.;
        s.start_width.right = 0.;
        s.widths[0].width.angle = 20.;
        insert_width_key(s, id, 0, 0.27);
        let before = s.clone();
        let maximum = s.line_width();
        assert!(maximum > 0.);
        s.resize_line_width(maximum * 2.).unwrap();
        assert!((s.line_width() - maximum * 2.).abs() < 1e-8);
        assert_eq!(s.start_width.left, 0.);
        assert_eq!(s.start_width.right, 0.);
        for segment in 0..s.segment_count() {
            for j in 0..101 {
                let t = j as f64 / 100.;
                let a = width_at(&before, segment, t).unwrap();
                let b = width_at(s, segment, t).unwrap();
                assert!((a.left * 2. - b.left).abs() < 1e-8);
                assert!((a.right * 2. - b.right).abs() < 1e-8);
                assert_eq!(a.angle, b.angle);
            }
        }
        assert!(s.widths.iter().find(|k| k.id == id).unwrap().interpolated);
        assert_eq!(before.nodes, s.nodes);
        d.validate().unwrap();
    }
    #[test]
    fn resizing_zero_width_and_invalid_values_is_safe() {
        let mut d = line();
        let s = &mut d.strokes[0];
        s.set_line_width(0.);
        s.resize_line_width(50.).unwrap();
        assert_eq!(s.start_width.left, 25.);
        assert_eq!(s.end_width.right, 25.);
        let before = s.clone();
        for v in [f64::NAN, f64::INFINITY, -1., 1001.] {
            assert!(s.resize_line_width(v).is_err());
            assert_eq!(*s, before);
        }
        s.resize_line_width(1000.).unwrap();
        d.validate().unwrap();
    }
    #[test]
    fn old_width_keys_remain_explicit_without_new_field() {
        let d = Document::example();
        let mut data = serde_json::to_value(&d).unwrap();
        for stroke in data["strokes"].as_array_mut().unwrap() {
            for key in stroke["widths"].as_array_mut().unwrap() {
                key.as_object_mut().unwrap().remove("interpolated");
            }
        }
        let restored: Document = serde_json::from_value(data).unwrap();
        assert_eq!(restored, d);
        assert_eq!(svg(&restored).unwrap(), svg(&d).unwrap());
    }
    #[test]
    fn project_roundtrip_and_validation() {
        let d = Document::example();
        let data = serde_json::to_string(&d).unwrap();
        let restored: Document = serde_json::from_str(&data).unwrap();
        assert_eq!(d, restored);
        restored.validate().unwrap();
        let mut broken = d;
        broken.strokes[0].start_angle = f64::NAN;
        assert!(broken.validate().is_err());
    }
    #[test]
    fn export_is_filled_finite_and_repeatable() {
        let d = Document::example();
        let svg = svg(&d).unwrap();
        assert!(svg.contains("fill-rule=\"nonzero\""));
        assert!(!svg.contains("NaN"));
        assert!(!svg.contains("stroke-width"));
        assert_eq!(svg, super::svg(&d).unwrap());
    }
    #[test]
    fn zero_length_path_is_empty() {
        let mut d = line();
        d.strokes[0].nodes[1].p = Point::ZERO;
        assert!(build(&d.strokes[0]).unwrap().ring.is_empty());
    }
}
