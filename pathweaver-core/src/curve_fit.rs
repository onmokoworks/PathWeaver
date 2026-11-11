//! Fitting cubic curves to closed polylines (outlines made of many short
//! straight pieces), so an exported outline is a few curves instead of
//! hundreds of lines. Sharp turns stay corners; each smooth run is fitted by
//! least squares with tangent continuity (Schneider, "An Algorithm for
//! Automatically Fitting Digitized Curves", Graphics Gems 1990).
use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveDeriv, Point, Vec2};

/// A turn sharper than this (radians) at one vertex is a corner.
const CORNER: f64 = 0.7;

fn unit(v: Vec2) -> Vec2 {
    let l = v.hypot();
    if l > 1e-12 { v / l } else { Vec2::ZERO }
}

/// Closed rings as curves within `tolerance` of their vertices.
pub fn fit_rings(rings: &[Vec<Point>], tolerance: f64) -> BezPath {
    let mut out = BezPath::new();
    for ring in rings {
        let mut r: Vec<Point> = Vec::with_capacity(ring.len());
        for &p in ring {
            if r.last().is_none_or(|q: &Point| q.distance(p) > 1e-9) {
                r.push(p);
            }
        }
        while r.len() > 1 && r[0].distance(*r.last().unwrap()) <= 1e-9 {
            r.pop();
        }
        if r.len() < 3 {
            continue;
        }
        fit_ring(&r, tolerance, &mut out);
    }
    out
}

fn fit_ring(r: &[Point], tol: f64, out: &mut BezPath) {
    let n = r.len();
    let turn = |i: usize| {
        let a = r[i] - r[(i + n - 1) % n];
        let b = r[(i + 1) % n] - r[i];
        (a.x * b.y - a.y * b.x).atan2(a.dot(b)).abs()
    };
    let corners: Vec<usize> = (0..n).filter(|&i| turn(i) > CORNER).collect();
    let mut curves = vec![];
    if corners.is_empty() {
        // smooth all round: start anywhere with the same tangent at both ends
        let mut run = r.to_vec();
        run.push(r[0]);
        let t = unit(r[1] - r[n - 1]);
        fit_run(&run, t, -t, tol, 0, &mut curves);
    } else {
        for (k, &c) in corners.iter().enumerate() {
            let next = corners[(k + 1) % corners.len()];
            let len = (next + n - c) % n;
            let len = if len == 0 { n } else { len };
            let run: Vec<Point> = (0..=len).map(|j| r[(c + j) % n]).collect();
            let (t0, t1) = (end_tangent(&run), -end_tangent(&run.iter().rev().copied().collect::<Vec<_>>()));
            fit_run(&run, t0, t1, tol, 0, &mut curves);
        }
    }
    out.move_to(curves[0].p0);
    for c in curves {
        // pieces shorter than the written precision would print as repeats
        if [c.p1, c.p2, c.p3].iter().all(|q| q.distance(c.p0) < 1e-3) {
            continue;
        }
        if c.p1 == c.p0 && c.p2 == c.p3 {
            out.line_to(c.p3);
        } else {
            out.curve_to(c.p1, c.p2, c.p3);
        }
    }
    out.close_path();
}

/// Tangent leaving the first point: the quadratic through the first three
/// points (chord-length spaced) — closer than the first chord when the
/// polyline samples a curve.
fn end_tangent(p: &[Point]) -> Vec2 {
    let first = unit(p[1] - p[0]);
    if p.len() < 3 {
        return first;
    }
    let (d1, d2) = (p[0].distance(p[1]), p[1].distance(p[2]));
    let (s1, s2) = (d1, d1 + d2);
    let t = unit((p[1] - p[0]) * (s2 / (s1 * (s2 - s1))) - (p[2] - p[0]) * (s1 / (s2 * (s2 - s1))));
    if t.dot(first) > 0.85 { t } else { first }
}

fn line(a: Point, b: Point) -> CubicBez {
    CubicBez::new(a, a, b, b)
}

/// Fit `p` (first and last points kept) leaving along `t0` and arriving
/// against `t1` (both unit, pointing into the run).
fn fit_run(p: &[Point], t0: Vec2, t1: Vec2, tol: f64, depth: u32, out: &mut Vec<CubicBez>) {
    let (a, b) = (p[0], *p.last().unwrap());
    if p.len() == 2 {
        out.push(line(a, b));
        return;
    }
    // straight within tolerance: one line
    let chord = b - a;
    let len = chord.hypot();
    if len > 1e-9 && p.iter().all(|q| ((*q - a).cross(chord) / len).abs() <= tol && (*q - a).dot(chord) >= -tol && (*q - a).dot(chord) <= len * len + tol * len) {
        out.push(line(a, b));
        return;
    }
    let mut u = chord_params(p);
    let mut bez = generate(p, &u, t0, t1);
    let (mut err, mut split) = max_error(p, &bez, &u);
    if err <= tol {
        out.push(bez);
        return;
    }
    if err <= tol * 16. {
        for _ in 0..6 {
            u = reparameterize(p, &u, &bez);
            bez = generate(p, &u, t0, t1);
            (err, split) = max_error(p, &bez, &u);
            if err <= tol {
                out.push(bez);
                return;
            }
        }
    }
    if depth > 24 || p.len() < 3 {
        out.push(bez);
        return;
    }
    let split = split.clamp(1, p.len() - 2);
    let mut center = unit(p[split + 1] - p[split - 1]);
    if center == Vec2::ZERO {
        center = unit(p[split] - p[split - 1]);
    }
    fit_run(&p[..=split], t0, -center, tol, depth + 1, out);
    fit_run(&p[split..], center, t1, tol, depth + 1, out);
}

fn chord_params(p: &[Point]) -> Vec<f64> {
    let mut u = vec![0.; p.len()];
    for i in 1..p.len() {
        u[i] = u[i - 1] + p[i].distance(p[i - 1]);
    }
    let total = u[p.len() - 1].max(1e-12);
    u.iter_mut().for_each(|v| *v /= total);
    u
}

fn generate(p: &[Point], u: &[f64], t0: Vec2, t1: Vec2) -> CubicBez {
    let (a, b) = (p[0], *p.last().unwrap());
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0., 0., 0., 0., 0.);
    for (q, &t) in p.iter().zip(u) {
        let s = 1. - t;
        let (b0, b1, b2, b3) = (s * s * s, 3. * s * s * t, 3. * s * t * t, t * t * t);
        let (a0, a1) = (t0 * b1, t1 * b2);
        c00 += a0.dot(a0);
        c01 += a0.dot(a1);
        c11 += a1.dot(a1);
        let tmp = *q - (a.to_vec2() * (b0 + b1) + b.to_vec2() * (b2 + b3)).to_point();
        x0 += a0.dot(tmp);
        x1 += a1.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let seg = a.distance(b);
    let (mut l, mut r) = if det.abs() > 1e-12 { ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det) } else { (0., 0.) };
    if !(l > seg * 1e-6 && r > seg * 1e-6) || l > seg * 4. || r > seg * 4. {
        l = seg / 3.;
        r = seg / 3.;
    }
    CubicBez::new(a, a + t0 * l, b + t1 * r, b)
}

fn max_error(p: &[Point], bez: &CubicBez, u: &[f64]) -> (f64, usize) {
    let mut worst = (0., p.len() / 2);
    for i in 1..p.len() - 1 {
        let d = bez.eval(u[i]).distance(p[i]);
        if d > worst.0 {
            worst = (d, i);
        }
    }
    // between samples too: a curve must not bulge away from a long piece
    for i in 0..p.len() - 1 {
        let m = bez.eval((u[i] + u[i + 1]) * 0.5);
        let (a, b) = (p[i], p[i + 1]);
        let v = b - a;
        let t = ((m - a).dot(v) / v.hypot2().max(1e-12)).clamp(0., 1.);
        let d = m.distance(a.lerp(b, t));
        if d > worst.0 {
            worst = (d, if i == 0 { 1 } else { i });
        }
    }
    worst
}

fn reparameterize(p: &[Point], u: &[f64], bez: &CubicBez) -> Vec<f64> {
    let d1 = bez.deriv();
    let d2 = d1.deriv();
    p.iter()
        .zip(u)
        .map(|(q, &t)| {
            let diff = bez.eval(t) - *q;
            let f1 = d1.eval(t).to_vec2();
            let f2 = d2.eval(t).to_vec2();
            let den = f1.dot(f1) + diff.dot(f2);
            if den.abs() < 1e-12 { t } else { (t - diff.dot(f1) / den).clamp(0., 1.) }
        })
        .collect()
}
