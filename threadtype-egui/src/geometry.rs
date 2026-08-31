//! タウトストリング（張力付き紐）エンジン。
//!
//! 線は始点（アンカー）から円形ペグに巻き付きながら伸びる。ペグはスナップ点の
//! 集合ではなく、線の経路を制約する丸い障害物として扱う。
//!
//! 状態は「アンカー + 巻き付きスタック + 先端」。巻き付き（Wrap）は
//! 進入接線角 `entry` と、2π を跨いでも連続な実数として追跡する離脱接線角
//! `exit` を別々に持つ。巻き付き量は `s * (exit - entry)` で、これが 0 を
//! 割った巻きは解放される。半周以上（2π 超の多重巻き）もこの表現で扱える。

use crate::model::{Element, Face, PegRef, PegShape, Port};
use kurbo::Point;
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// 巻き付き量がこの値以下になったら解放（たるみ判定）
pub const RELEASE_EPS: f64 = 1e-4;
/// 捕捉には実効半径よりこの分だけ深く食い込む必要がある（解放条件との
/// 非対称性がヒステリシスとして働き、捕捉/解放のフリッカーを防ぐ）
pub const PENETRATION_EPS: f64 = 0.35;
const MAX_STEPS: usize = 64;
const TAU: f64 = 2.0 * PI;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peg {
    pub center: Point,
    pub radius: f64,
    pub shape: PegShape,
    pub rounding: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wrap {
    pub peg: usize,
    /// 巻き方向。+1.0 = 角度増加方向（画面座標では見た目時計回り）、-1.0 = 減少方向
    pub s: f64,
    /// 進入接線角。巻き付いた時点で確定する
    pub entry: f64,
    /// 離脱接線角。entry と連続な実数（アンラップ済み）で毎フレーム更新される
    pub exit: f64,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub anchor: Point,
    pub wraps: Vec<Wrap>,
    pub tip: Point,
}

/// 一本の外周線を構成する区間。直線（接線）と円弧の列
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Seg {
    Line {
        from: Point,
        to: Point,
    },
    /// start/end はアンラップ済み角度。end - start が符号付き掃引量（2π 超も可）
    Arc {
        center: Point,
        radius: f64,
        start: f64,
        end: f64,
    },
}

pub fn angle_from(center: Point, p: Point) -> f64 {
    (p.y - center.y).atan2(p.x - center.x)
}

pub fn on_circle(center: Point, radius: f64, angle: f64) -> Point {
    Point::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

pub fn peg_signed_distance(peg: &Peg, point: Point) -> f64 {
    let local = point - peg.center;
    match peg.shape {
        PegShape::Circle => local.hypot() - peg.radius,
        PegShape::RoundedSquare => {
            let corner = peg.rounding.clamp(0.0, peg.radius);
            let inner = peg.radius - corner;
            let qx = local.x.abs() - inner;
            let qy = local.y.abs() - inner;
            let outside = qx.max(0.0).hypot(qy.max(0.0));
            outside + qx.max(qy).min(0.0) - corner
        }
    }
}

pub fn on_peg(peg: &Peg, angle: f64) -> Point {
    if peg.shape == PegShape::Circle {
        return on_circle(peg.center, peg.radius, angle);
    }
    let dx = angle.cos();
    let dy = angle.sin();
    let mut lo = 0.0;
    let mut hi = peg.radius / dx.abs().max(dy.abs()).max(1e-9);
    for _ in 0..36 {
        let mid = (lo + hi) * 0.5;
        let point = Point::new(peg.center.x + dx * mid, peg.center.y + dy * mid);
        if peg_signed_distance(peg, point) <= 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Point::new(peg.center.x + dx * lo, peg.center.y + dy * lo)
}

fn peg_derivative(peg: &Peg, angle: f64) -> kurbo::Vec2 {
    let h = 1e-5;
    let a = on_peg(peg, angle - h);
    let b = on_peg(peg, angle + h);
    (b - a) / (2.0 * h)
}

fn tangent_angle_from_seed(point: Point, peg: &Peg, seed: f64) -> f64 {
    let objective = |angle: f64| {
        let boundary = on_peg(peg, angle);
        let tangent = peg_derivative(peg, angle);
        (point - boundary).dot(tangent).abs()
    };
    let mut best = seed;
    let mut score = objective(seed);
    for i in -48..=48 {
        let candidate = seed + i as f64 * PI / 192.0;
        let candidate_score = objective(candidate);
        if candidate_score < score {
            best = candidate;
            score = candidate_score;
        }
    }
    let mut step = PI / 192.0;
    for _ in 0..18 {
        let left = best - step;
        let right = best + step;
        if objective(left) < objective(best) {
            best = left;
        }
        if objective(right) < objective(best) {
            best = right;
        }
        step *= 0.5;
    }
    best
}

/// (-π, π] に正規化
fn norm_pm_pi(a: f64) -> f64 {
    let mut a = a % (2.0 * PI);
    if a > PI {
        a -= 2.0 * PI;
    } else if a <= -PI {
        a += 2.0 * PI;
    }
    a
}

/// raw と 2πn だけ異なる角度のうち near に最も近いものを返す（連続性の維持）
fn unwrap_near(raw: f64, near: f64) -> f64 {
    near + norm_pm_pi(raw - near)
}

/// 点がペグ内部に入らないように境界へ押し出す
pub fn clamp_outside_pegs(p: Point, pegs: &[Peg]) -> Point {
    for peg in pegs {
        let d = p.distance(peg.center);
        if peg_signed_distance(peg, p) < 0.01 {
            if d < 1e-9 {
                return on_peg(peg, 0.0) + (0.01, 0.0);
            }
            let boundary = on_peg(peg, angle_from(peg.center, p));
            let direction = boundary - peg.center;
            return boundary + direction * (0.01 / direction.hypot().max(1e-9));
        }
    }
    p
}

pub fn resolve_anchor(peg: &Peg, port: Port) -> Point {
    on_peg(peg, port.angle())
}

/// 外部点 p からペグへ、方向 s で巻き始めるときの進入接線角
fn point_tangent_entry(p: Point, peg: &Peg, s: f64) -> f64 {
    let d = p.distance(peg.center).max(peg.radius);
    let beta = (peg.radius / d).clamp(-1.0, 1.0).acos();
    let seed = angle_from(peg.center, p) + s * beta;
    if peg.shape == PegShape::Circle {
        seed
    } else {
        tangent_angle_from_seed(p, peg, seed)
    }
}

/// ペグから方向 s の巻きを解いて外部点 p へ離脱するときの接線角
fn point_tangent_exit(p: Point, peg: &Peg, s: f64) -> f64 {
    let d = p.distance(peg.center).max(peg.radius);
    let beta = (peg.radius / d).clamp(-1.0, 1.0).acos();
    let seed = angle_from(peg.center, p) - s * beta;
    if peg.shape == PegShape::Circle {
        seed
    } else {
        tangent_angle_from_seed(p, peg, seed)
    }
}

/// ペグ1（方向 s1）からペグ2（方向 s2）への共通接線。
/// 戻り値は (ペグ1上の離脱角, ペグ2上の進入角)。
/// s1 == s2 なら外接線、異なるなら内接線（交差接線）になる。
fn circle_circle_tangent(p1: &Peg, s1: f64, p2: &Peg, s2: f64) -> Option<(f64, f64)> {
    let theta = angle_from(p1.center, p2.center);
    let d = p1.center.distance(p2.center);
    if d <= 1e-9 {
        return None;
    }
    let seed = if s1 == s2 {
        let ratio = (p1.radius - p2.radius) / d;
        if ratio.abs() > 1.0 {
            return None;
        }
        let alpha = ratio.acos();
        let phi = theta - s1 * alpha;
        (phi, phi)
    } else {
        let ratio = (p1.radius + p2.radius) / d;
        if ratio.abs() > 1.0 {
            return None;
        }
        let alpha = ratio.acos();
        let phi1 = theta - s1 * alpha;
        (phi1, phi1 + PI)
    };
    if p1.shape == PegShape::Circle && p2.shape == PegShape::Circle {
        return Some(seed);
    }
    let residual = |a1: f64, a2: f64| {
        let q1 = on_peg(p1, a1);
        let q2 = on_peg(p2, a2);
        let line = q2 - q1;
        let t1 = peg_derivative(p1, a1);
        let t2 = peg_derivative(p2, a2);
        (line.cross(t1), line.cross(t2))
    };
    let (mut a1, mut a2) = seed;
    for _ in 0..14 {
        let (f1, f2) = residual(a1, a2);
        if f1.abs().max(f2.abs()) < 1e-7 {
            break;
        }
        let h = 1e-5;
        let (x1, x2) = residual(a1 + h, a2);
        let (y1, y2) = residual(a1, a2 + h);
        let j11 = (x1 - f1) / h;
        let j21 = (x2 - f2) / h;
        let j12 = (y1 - f1) / h;
        let j22 = (y2 - f2) / h;
        let det = j11 * j22 - j12 * j21;
        if det.abs() < 1e-10 {
            break;
        }
        let da1 = (-f1 * j22 + j12 * f2) / det;
        let da2 = (-j11 * f2 + f1 * j21) / det;
        a1 += da1.clamp(-0.25, 0.25);
        a2 += da2.clamp(-0.25, 0.25);
    }
    Some((a1, a2))
}

struct Hit {
    peg: usize,
    t: f64,
    side: f64,
}

fn segment_first_entry(a: Point, b: Point, peg: &Peg) -> Option<f64> {
    let d = b - a;
    let steps = ((d.hypot() / (peg.radius * 0.2).clamp(1.0, 6.0)).ceil() as usize).clamp(32, 768);
    let mut previous_t = 0.0;
    let mut previous_inside = peg_signed_distance(peg, a) < -PENETRATION_EPS;
    if previous_inside {
        return Some(0.0);
    }
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let inside = peg_signed_distance(peg, a + d * t) < -PENETRATION_EPS;
        if inside && !previous_inside {
            let mut lo = previous_t;
            let mut hi = t;
            for _ in 0..24 {
                let mid = (lo + hi) * 0.5;
                if peg_signed_distance(peg, a + d * mid) < -PENETRATION_EPS {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            return Some(hi);
        }
        previous_t = t;
        previous_inside = inside;
    }
    None
}

/// 線分 a→b が食い込む最初のペグ。接触だけ（接線）では捕捉しない
fn first_hit(
    a: Point,
    b: Point,
    pegs: &[Peg],
    exclude: Option<usize>,
    ignored: &[usize],
) -> Option<Hit> {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 < 1e-12 {
        return None;
    }
    let mut best: Option<Hit> = None;
    for (i, peg) in pegs.iter().enumerate() {
        if Some(i) == exclude || ignored.contains(&i) {
            continue;
        }
        let Some(t_in) = segment_first_entry(a, b, peg) else {
            continue;
        };
        if t_in >= 1.0 {
            continue;
        }
        // 進行方向に対して中心がどちら側か → 巻き方向。中心を正確に突っ切る
        // 縮退（cross == 0）でも符号のタイブレークで潰れない
        let cross = d.x * (peg.center.y - a.y) - d.y * (peg.center.x - a.x);
        let side = if cross >= 0.0 { 1.0 } else { -1.0 };
        if best.as_ref().map_or(true, |h| t_in < h.t) {
            best = Some(Hit {
                peg: i,
                t: t_in,
                side,
            });
        }
    }
    best
}

/// 線分を通過するペグのインデックスを、進行方向順で返す
pub fn pegs_penetrated_by_line(
    a: Point,
    b: Point,
    pegs: &[Peg],
    exclude: Option<usize>,
    ignored: &[usize],
) -> Vec<usize> {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 < 1e-12 {
        return Vec::new();
    }
    let mut out = Vec::new();
    const EPS_T: f64 = 1e-9;
    for (i, peg) in pegs.iter().enumerate() {
        if Some(i) == exclude || ignored.contains(&i) {
            continue;
        }
        let Some(t_in) = segment_first_entry(a, b, peg) else {
            continue;
        };
        if t_in > EPS_T && t_in < 1.0 - EPS_T {
            out.push((t_in, i));
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(_, idx)| idx).collect()
}

/// 完成経路の直線が有効なペグ内部を通過していないかを検証する。
/// 接線接触は `PENETRATION_EPS` により貫通扱いにならない。
pub fn segments_penetrate_pegs(segs: &[Seg], pegs: &[Peg]) -> bool {
    segs.iter().any(|segment| match *segment {
        Seg::Line { from, to } => pegs
            .iter()
            .any(|peg| segment_first_entry(from, to, peg).is_some()),
        Seg::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let sweep = end - start;
            let length = sweep.abs() * radius;
            let steps = (length / 2.0).ceil().clamp(16.0, 2048.0) as usize;
            pegs.iter().any(|peg| {
                (0..=steps).any(|i| {
                    let t = i as f64 / steps as f64;
                    let point = on_circle(center, radius, start + sweep * t);
                    peg_signed_distance(peg, point) < -PENETRATION_EPS
                })
            })
        }
    })
}

impl Thread {
    pub fn new(anchor: Point) -> Self {
        Thread {
            anchor,
            wraps: Vec::new(),
            tip: anchor,
        }
    }

    /// 自由区間の付け根（最後の巻きの離脱点、巻きが無ければアンカー）
    pub fn head(&self, pegs: &[Peg]) -> Point {
        match self.wraps.last() {
            Some(w) => on_peg(&pegs[w.peg], w.exit),
            None => self.anchor,
        }
    }
}

/// カーソル移動ごとに呼ぶコア更新。
/// (1) 末尾ラップの離脱角を再計算し、たるんだ巻きを解放（連鎖可）
/// (2) 自由区間が新しいペグに食い込んだら巻き付く（連鎖可）
pub fn update_thread_ignoring(th: &mut Thread, cursor: Point, pegs: &[Peg], ignored: &[usize]) {
    th.tip = clamp_outside_pegs(cursor, pegs);
    for _ in 0..MAX_STEPS {
        if let Some(last) = th.wraps.last_mut() {
            let peg = pegs[last.peg];
            let raw = point_tangent_exit(th.tip, &peg, last.s);
            last.exit = unwrap_near(raw, last.exit);
            if (last.exit - last.entry) * last.s <= RELEASE_EPS {
                th.wraps.pop();
                continue;
            }
        }
        let head = th.head(pegs);
        let exclude = th.wraps.last().map(|w| w.peg);
        let Some(hit) = first_hit(head, th.tip, pegs, exclude, ignored) else {
            break;
        };
        let peg = pegs[hit.peg];
        if th.wraps.is_empty() {
            let entry = point_tangent_entry(th.anchor, &peg, hit.side);
            th.wraps.push(Wrap {
                peg: hit.peg,
                s: hit.side,
                entry,
                exit: entry,
            });
        } else {
            // 摩擦モデル: 前の巻きは現在の把持点で凍結する。共通接線まで
            // 滑らせる（純タウトストリング）と、開始点が共通接線上に乗る
            // グリッド配置では前の巻きがゼロに潰れて外れてしまう
            let prev = th.wraps.last().unwrap();
            let p1 = pegs[prev.peg];
            let grip = on_peg(&p1, prev.exit);
            let entry = point_tangent_entry(grip, &peg, hit.side);
            th.wraps.push(Wrap {
                peg: hit.peg,
                s: hit.side,
                entry,
                exit: entry,
            });
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn update_thread(th: &mut Thread, cursor: Point, pegs: &[Peg]) {
    update_thread_ignoring(th, cursor, pegs, &[])
}

/// 閉じるとき、戻り線が始点円に触れない角度でも始点ペグへ接線で巻き込み、
/// アンカーに円弧で滑らかに到達させる（直線のカドを作らない）。
/// - 既に末尾が始点ペグの巻きなら何もしない
/// - 巻き込みが半周を大きく超える向きしか無い場合は不自然なので直線のまま
#[cfg_attr(not(test), allow(dead_code))]
pub fn append_closing_hug(th: &mut Thread, pegs: &[Peg], start_peg: usize, anchor: Point) {
    if th.wraps.last().map(|w| w.peg) == Some(start_peg) {
        return;
    }
    let head = th.head(pegs);
    if head.distance(anchor) < 2.0 {
        return;
    }
    let peg = pegs[start_peg];
    let anchor_angle = angle_from(peg.center, anchor);
    let mut best: Option<(f64, f64, f64)> = None; // (sweep, s, entry)
    for s in [1.0, -1.0] {
        let entry = point_tangent_entry(head, &peg, s);
        let sweep = ((anchor_angle - entry) * s).rem_euclid(TAU);
        if best.map_or(true, |(bs, _, _)| sweep < bs) {
            best = Some((sweep, s, entry));
        }
    }
    if let Some((sweep, s, entry)) = best {
        if sweep > 1e-4 && sweep <= PI + 0.2 {
            th.wraps.push(Wrap {
                peg: start_peg,
                s,
                entry,
                exit: entry + sweep * s,
            });
        }
    }
}

/// スレッドを区間列に落とす。end は先端（描画中は tip、閉じるときはアンカー）。
/// アプリ本体はパラメトリック経由（materialize_chain）に移行済みで、
/// 現在はソルバー結果との一致検証（ラウンドトリップテスト）に使う
#[cfg_attr(not(test), allow(dead_code))]
pub fn materialize(th: &Thread, pegs: &[Peg], end: Point) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut cur = th.anchor;
    for w in &th.wraps {
        let peg = pegs[w.peg];
        let pin = on_peg(&peg, w.entry);
        if cur.distance(pin) > 0.01 {
            segs.push(Seg::Line { from: cur, to: pin });
        }
        segs.push(Seg::Arc {
            center: peg.center,
            radius: peg.radius,
            start: w.entry,
            end: w.exit,
        });
        cur = on_peg(&peg, w.exit);
    }
    if cur.distance(end) > 0.01 {
        segs.push(Seg::Line { from: cur, to: end });
    }
    segs
}

pub fn materialize_face(
    face: &Face,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
) -> Option<Vec<Seg>> {
    // 完成した閉路では作図開始ポートはUI上の入口にすぎない。円だけの面は
    // 全ペグ間の共通接線を循環計算し、8方向ポートを固定アンカーとして残さない。
    if face
        .elements
        .iter()
        .all(|element| matches!(element, Element::Wrap { .. }))
    {
        if let Some(segs) = materialize_taut_wrap_cycle(face, resolve) {
            return Some(segs);
        }
    }
    let start_peg = resolve(&face.start.peg)?;
    let start = resolve_anchor(&start_peg, face.start.port);
    materialize_chain(start, &face.elements, resolve, start)
}

fn materialize_taut_wrap_cycle(
    face: &Face,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
) -> Option<Vec<Seg>> {
    let mut wraps: Vec<(PegRef, f64, u32, f64)> = face
        .elements
        .iter()
        .filter_map(|element| match *element {
            Element::Wrap {
                peg,
                s,
                turns,
                sweep_hint,
                ..
            } => Some((peg, s, turns, sweep_hint)),
            Element::Pin { .. } => None,
        })
        .collect();
    wraps.dedup_by(|next, previous| {
        if next.0 == previous.0 {
            previous.2 = previous.2.saturating_add(next.2);
            true
        } else {
            false
        }
    });
    if wraps.len() > 1 && wraps.first().map(|wrap| wrap.0) == wraps.last().map(|wrap| wrap.0) {
        let last = wraps.pop().unwrap();
        wraps[0].2 = wraps[0].2.saturating_add(last.2);
    }
    if wraps.is_empty() {
        return None;
    }

    // 古い保存データや、始点への巻き込みが省略された面にも始点ペグを補う。
    if !wraps.iter().any(|(peg, _, _, _)| *peg == face.start.peg) {
        let previous = resolve(&wraps.last()?.0)?;
        let start = resolve(&face.start.peg)?;
        let next = resolve(&wraps.first()?.0)?;
        let previous_s = wraps.last()?.1;
        let next_s = wraps.first()?.1;
        let start_s = [1.0, -1.0]
            .into_iter()
            .filter_map(|s| {
                let (_, entry) = circle_circle_tangent(&previous, previous_s, &start, s)?;
                let (exit, _) = circle_circle_tangent(&start, s, &next, next_s)?;
                Some((s, ((exit - entry) * s).rem_euclid(TAU)))
            })
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?
            .0;
        wraps.push((face.start.peg, start_s, 0, PI));
    }
    if wraps.len() < 2 {
        return None;
    }

    let pegs: Vec<Peg> = wraps
        .iter()
        .map(|(peg, _, _, _)| resolve(peg))
        .collect::<Option<_>>()?;
    let count = wraps.len();
    let mut entries = Vec::with_capacity(count);
    let mut exits = Vec::with_capacity(count);
    for i in 0..count {
        let previous = (i + count - 1) % count;
        let next = (i + 1) % count;
        let (_, entry) =
            circle_circle_tangent(&pegs[previous], wraps[previous].1, &pegs[i], wraps[i].1)?;
        let (exit, _) = circle_circle_tangent(&pegs[i], wraps[i].1, &pegs[next], wraps[next].1)?;
        entries.push(entry);
        exits.push(exit);
    }

    let mut segs = Vec::with_capacity(count * 2);
    for i in 0..count {
        let s = wraps[i].1;
        // 共通接線は s の向きを前提に選ばれているため、保存された hint に
        // 合わせて符号を反転してはいけない。反転すると線と円弧が180°の
        // カスプになる。追加周回は turns だけで明示的に表現する。
        let sweep = ((exits[i] - entries[i]) * s).rem_euclid(TAU);
        let end = entries[i] + (sweep + wraps[i].2 as f64 * TAU) * s;
        segs.push(Seg::Arc {
            center: pegs[i].center,
            radius: pegs[i].radius,
            start: entries[i],
            end,
        });
        let next = (i + 1) % count;
        let from = on_peg(&pegs[i], end);
        let to = on_peg(&pegs[next], entries[next]);
        if from.distance(to) > 1e-9 {
            segs.push(Seg::Line { from, to });
        }
    }
    Some(segs)
}

/// 保存された接触順序を意図として保ちながら、現在有効な障害円を閉路全体へ
/// 挿入して共通接線を解き直す。描画時に直線を一本ずつ迂回させないため、
/// 後から有効化されたサブ円でも円弧と直線の接線連続性を維持する。
pub fn materialize_face_with_obstacles(
    face: &Face,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
    obstacles: &[(PegRef, Peg)],
) -> Option<Vec<Seg>> {
    materialize_face_with_obstacles_derived(face, resolve, obstacles).map(|(segments, _)| segments)
}

/// Resolve a taut face and return the wrap sequence produced by automatic
/// obstacle capture. The editor uses this during peg dragging to commit a
/// newly caught peg to the document instead of forgetting it next frame.
pub fn capture_face_obstacles(
    face: &Face,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
    obstacles: &[(PegRef, Peg)],
) -> Option<Vec<Element>> {
    materialize_face_with_obstacles_derived(face, resolve, obstacles).map(|(_, elements)| elements)
}

fn materialize_face_with_obstacles_derived(
    face: &Face,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
    obstacles: &[(PegRef, Peg)],
) -> Option<(Vec<Seg>, Vec<Element>)> {
    if !face
        .elements
        .iter()
        .all(|element| matches!(element, Element::Wrap { .. }))
    {
        return materialize_face(face, resolve).map(|segments| (segments, face.elements.clone()));
    }
    if face
        .elements
        .iter()
        .filter(|element| matches!(element, Element::Wrap { .. }))
        .count()
        < 2
    {
        return materialize_face(face, resolve).map(|segments| (segments, face.elements.clone()));
    }

    let mut elements = face.elements.clone();
    if !elements
        .iter()
        .any(|element| matches!(element, Element::Wrap { peg, .. } if *peg == face.start.peg))
    {
        let base = materialize_taut_wrap_cycle(face, resolve)?;
        let start = resolve(&face.start.peg)?;
        let start_arc = base.iter().find_map(|segment| match *segment {
            Seg::Arc {
                center,
                start: entry,
                end,
                ..
            } if center.distance(start.center) < 1e-8 => Some((entry, end)),
            _ => None,
        });
        let s = start_arc.map_or(1.0, |(entry, end)| if end >= entry { 1.0 } else { -1.0 });
        elements.push(Element::Wrap {
            peg: face.start.peg,
            s,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: PI,
        });
    }

    let mut last_segs = None;
    let mut automatically_inserted = Vec::new();
    for _ in 0..MAX_STEPS {
        let probe = Face {
            start: face.start,
            elements: elements.clone(),
            visible: face.visible,
            name: face.name.clone(),
            baked: None,
            ignored_pegs: face.ignored_pegs.clone(),
        };
        let segs = materialize_taut_wrap_cycle(&probe, resolve)?;
        last_segs = Some(segs.clone());
        let mut line_index = 0usize;
        let mut insertion: Option<(usize, PegRef, f64)> = None;
        for segment in &segs {
            let Seg::Line { from, to } = *segment else {
                continue;
            };
            let d = to - from;
            let mut best: Option<(f64, PegRef, f64)> = None;
            for (peg_ref, peg) in obstacles {
                // Once an obstacle has been inserted into this derived taut cycle,
                // do not discover the same peg again from another edge. Explicit
                // repeated wraps remain intact in `face.elements`; this only
                // prevents automatic capture from multiplying while a peg moves.
                if automatically_inserted.contains(peg_ref) {
                    continue;
                }
                let Some(t) = segment_first_entry(from, to, peg) else {
                    continue;
                };
                if !(1e-9..1.0 - 1e-9).contains(&t) {
                    continue;
                }
                let cross = d.x * (peg.center.y - from.y) - d.y * (peg.center.x - from.x);
                let side = if cross >= 0.0 { 1.0 } else { -1.0 };
                if best.as_ref().map_or(true, |(best_t, _, _)| t < *best_t) {
                    best = Some((t, *peg_ref, side));
                }
            }
            if let Some((_, peg_ref, side)) = best {
                insertion = Some((line_index + 1, peg_ref, side));
                break;
            }
            line_index += 1;
        }
        let Some((index, peg, s)) = insertion else {
            return Some((segs, elements));
        };
        elements.insert(
            index.min(elements.len()),
            Element::Wrap {
                peg,
                s,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: PI,
            },
        );
        automatically_inserted.push(peg);
    }
    // 高密度配置で上限に達しても面を消さない。未解決の貫通は呼び出し側の
    // invalid表示で明示し、ユーザーが半径や配置を戻せる状態を保つ。
    last_segs.map(|segments| (segments, elements))
}

/// 開始点から要素列をたどり、end で終わる開いた経路を実体化する。
/// end == start なら閉路（materialize_face と同じ）。描画中のプレビューは
/// end = 現在の先端 で呼ぶことで、閉じ線を表示せずカーソルに追従する
pub fn materialize_chain(
    start: Point,
    elements: &[crate::model::Element],
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
    end: Point,
) -> Option<Vec<Seg>> {
    let mut segs = Vec::new();
    let mut cur = start;

    for (i, el) in elements.iter().enumerate() {
        match *el {
            Element::Pin { pos } => {
                if cur.distance(pos) > 1e-9 {
                    segs.push(Seg::Line { from: cur, to: pos });
                }
                cur = pos;
            }
            Element::Wrap {
                peg: ref peg_ref,
                s,
                turns,
                exit_offset,
                sweep_hint,
            } => {
                let peg = resolve(peg_ref)?;

                // 進入は常に「直前の点」（アンカー / Pin / 前の巻きの把持点）からの
                // 接線。ソルバーの摩擦モデルと同じ構成
                let entry = point_tangent_entry(cur, &peg, s);
                let nat_exit =
                    natural_exit_angle(&peg, s, entry, elements.get(i + 1), resolve, end)?;
                // 掃引の枝選択: 円の再配置で自然掃引が 0/2π を跨いだとき、
                // 確定時の掃引量（sweep_hint）に最も近い枝を採用する
                let mut sweep = (nat_exit - entry) * s;
                if (sweep - TAU - sweep_hint).abs() < (sweep - sweep_hint).abs() {
                    sweep -= TAU;
                } else if (sweep + TAU - sweep_hint).abs() < (sweep - sweep_hint).abs() {
                    sweep += TAU;
                }
                let exit = entry + sweep * s + turns as f64 * TAU * s + exit_offset;

                let from = on_peg(&peg, entry);
                if cur.distance(from) > 1e-9 {
                    segs.push(Seg::Line {
                        from: cur,
                        to: from,
                    });
                }
                segs.push(Seg::Arc {
                    center: peg.center,
                    radius: peg.radius,
                    start: entry,
                    end: exit,
                });
                cur = on_peg(&peg, exit);
            }
        }
    }

    if cur.distance(end) > 1e-9 {
        segs.push(Seg::Line { from: cur, to: end });
    }
    Some(segs)
}

/// 巻きの「自然な」離脱角（オフセット・周回数を除いた、(0, 2π] に正規化した
/// タウトストリングの接線角）。materialize_chain と thread_to_wrap_elements が
/// 同じ式を共有することでラウンドトリップが厳密に一致する
fn natural_exit_angle(
    peg: &Peg,
    s: f64,
    entry: f64,
    next: Option<&crate::model::Element>,
    resolve: &impl Fn(&PegRef) -> Option<Peg>,
    end: Point,
) -> Option<f64> {
    let raw = match next {
        Some(Element::Pin { pos }) => point_tangent_exit(*pos, peg, s),
        Some(Element::Wrap {
            peg: next_ref,
            s: next_s,
            ..
        }) => {
            let next_peg = resolve(next_ref)?;
            let (exit, _) = circle_circle_tangent(peg, s, &next_peg, *next_s)?;
            exit
        }
        None => point_tangent_exit(end, peg, s),
    };
    let mut exit = unwrap_near(raw, entry);
    let mut signed = (exit - entry) * s;
    if signed <= 0.0 {
        exit += TAU * s.signum();
        signed = (exit - entry) * s;
    }
    while signed > TAU + 1e-12 {
        exit -= TAU * s.signum();
        signed = (exit - entry) * s;
    }
    Some(exit)
}

/// ライブのスレッド（ソルバー結果）をパラメトリック要素列に変換する。
/// end は自由端の目標点（プレビューなら tip、閉じるならアンカー）。
/// 摩擦による把持のズレは exit_offset として保存され、materialize_chain で
/// 厳密に再現される
pub fn thread_to_wrap_elements(
    th: &Thread,
    pegs: &[Peg],
    refs: &[PegRef],
    end: Point,
) -> Option<Vec<Element>> {
    let mut out = Vec::with_capacity(th.wraps.len());
    let mut cur = th.anchor;
    let resolve_local =
        |r: &PegRef| -> Option<Peg> { refs.iter().position(|pr| pr == r).map(|idx| pegs[idx]) };
    for (i, w) in th.wraps.iter().enumerate() {
        let peg = pegs[w.peg];
        let peg_ref = *refs.get(w.peg)?;
        let entry = point_tangent_entry(cur, &peg, w.s);
        // 次の要素をチェーンと同じ形に見立てて自然離脱角を求める
        let next_el = match th.wraps.get(i + 1) {
            Some(nw) => Some(Element::Wrap {
                peg: *refs.get(nw.peg)?,
                s: nw.s,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: PI,
            }),
            None => None,
        };
        let nat_exit = natural_exit_angle(&peg, w.s, entry, next_el.as_ref(), &resolve_local, end)?;
        let actual_sweep = ((w.exit - entry) * w.s).max(0.0);
        let natural_sweep = ((nat_exit - entry) * w.s).rem_euclid(TAU);
        let extra_sweep = (actual_sweep - natural_sweep).max(0.0);
        // 完全周回は最も近い整数へ分離し、残りだけを把持オフセットにする。
        // floorだと自然接線との差がわずかに負のとき、1周がoffset側へ入り
        // 保存後の線幅・周回数が一つ少なくなる。
        let turns = (extra_sweep / TAU).round() as u32;
        let exit_offset = w.exit - (nat_exit + turns as f64 * TAU * w.s);
        let sweep_hint = (nat_exit - entry) * w.s;
        out.push(Element::Wrap {
            peg: peg_ref,
            s: w.s,
            turns,
            exit_offset,
            sweep_hint,
        });
        cur = on_peg(&peg, w.exit);
    }
    Some(out)
}

/// 区間列を折れ線に展開。弧の分割幅を弦誤差 tol（ワールド単位）から決める。
/// 分割が細かすぎると egui のフェザリングで線が細く見えるため、
/// 画面上の見た目に合わせた tol を渡すこと
#[cfg_attr(not(test), allow(dead_code))]
pub fn flatten_segs_tol(segs: &[Seg], out: &mut Vec<Point>, tol: f64) {
    for seg in segs {
        match *seg {
            Seg::Line { from, to } => {
                if out.is_empty() {
                    out.push(from);
                }
                out.push(to);
            }
            Seg::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let step = if radius > 1e-6 {
                    (8.0 * tol / radius).sqrt().clamp(0.02, 0.5)
                } else {
                    0.5
                };
                let sweep = end - start;
                let steps = ((sweep.abs() / step).ceil() as usize).max(2);
                if out.is_empty() {
                    out.push(on_circle(center, radius, start));
                }
                for i in 1..=steps {
                    let a = start + sweep * (i as f64 / steps as f64);
                    out.push(on_circle(center, radius, a));
                }
            }
        }
    }
}

/// 区間列を折れ線に展開（描画・塗り・面積計算用）
#[cfg_attr(not(test), allow(dead_code))]
pub fn flatten_segs(segs: &[Seg], out: &mut Vec<Point>) {
    for seg in segs {
        match *seg {
            Seg::Line { from, to } => {
                if out.is_empty() {
                    out.push(from);
                }
                out.push(to);
            }
            Seg::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = end - start;
                let steps = ((sweep.abs() / 0.06).ceil() as usize).max(2);
                if out.is_empty() {
                    out.push(on_circle(center, radius, start));
                }
                for i in 1..=steps {
                    let a = start + sweep * (i as f64 / steps as f64);
                    out.push(on_circle(center, radius, a));
                }
            }
        }
    }
}

pub fn signed_area(points: &[Point]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        sum += a.x * b.y - b.x * a.y;
    }
    sum * 0.5
}

/// 複数のポリゴン（各面の外周を平坦化した点列）を論理和（ユニオン）して
/// 合体した輪郭のリング列を返す。外周と穴（カウンター）を含む。
/// マージ機能の中核。
#[cfg_attr(not(test), allow(dead_code))]
pub fn union_polygons(polys: &[Vec<Point>]) -> Vec<Vec<Point>> {
    let ring_sets: Vec<Vec<Vec<Point>>> = polys.iter().map(|ring| vec![ring.clone()]).collect();
    union_polygon_ring_sets(&ring_sets)
}

/// 複数の形状（各形状は外周・穴・独立成分を含むリング群）を論理和する。
/// リングの向きには依存せず、包含の深さが偶数なら面、奇数なら穴として扱う。
pub fn union_polygon_ring_sets(ring_sets: &[Vec<Vec<Point>>]) -> Vec<Vec<Point>> {
    use geo::{BooleanOps, Coord, LineString, MultiPolygon, Polygon};

    let to_poly = |pts: &[Point]| -> Option<Polygon<f64>> {
        if pts.len() < 3 {
            return None;
        }
        let ring: Vec<Coord<f64>> = pts.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
        Some(Polygon::new(LineString(ring), vec![]))
    };

    let contains = |ring: &[Point], point: Point| {
        let mut inside = false;
        let mut j = ring.len().saturating_sub(1);
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[j]);
            if (a.y > point.y) != (b.y > point.y)
                && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
            {
                inside = !inside;
            }
            j = i;
        }
        inside
    };

    let mut acc: Option<MultiPolygon<f64>> = None;
    for rings in ring_sets {
        let mut ordered: Vec<&Vec<Point>> = rings.iter().filter(|ring| ring.len() >= 3).collect();
        ordered.sort_by(|a, b| {
            signed_area(b)
                .abs()
                .partial_cmp(&signed_area(a).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut shape: Option<MultiPolygon<f64>> = None;
        for (index, ring) in ordered.iter().enumerate() {
            let sample = ring[0];
            let depth = ordered[..index]
                .iter()
                .filter(|outer| contains(outer, sample))
                .count();
            let Some(poly) = to_poly(ring) else { continue };
            let mp = MultiPolygon(vec![poly]);
            if shape.is_none() && depth % 2 == 1 {
                continue;
            }
            shape = Some(match (shape, depth % 2) {
                (Some(current), 0) => current.union(&mp),
                (Some(current), _) => current.difference(&mp),
                (None, 0) => mp,
                (None, _) => unreachable!(),
            });
        }
        if let Some(shape) = shape {
            acc = Some(match acc {
                Some(current) => current.union(&shape),
                None => shape,
            });
        }
    }

    let mut rings = Vec::new();
    if let Some(mp) = acc {
        for poly in mp.0 {
            let ext: Vec<Point> = poly
                .exterior()
                .coords()
                .map(|c| Point::new(c.x, c.y))
                .collect();
            rings.push(dedup_closed(ext));
            for hole in poly.interiors() {
                let h: Vec<Point> = hole.coords().map(|c| Point::new(c.x, c.y)).collect();
                rings.push(dedup_closed(h));
            }
        }
    }
    rings
}

/// 画面に見えている塗りをそのまま論理和へ変換する。
/// 各シェイプを画面と同じ NonZero で三角形分割して自己交差を解消してから、
/// 全三角形を加算的に union するため、重なりが逆向き輪郭で抜け落ちない。
pub fn union_visible_ring_sets(ring_sets: &[Vec<Vec<Point>>]) -> Vec<Vec<Point>> {
    use lyon::math::point as lpoint;
    use lyon::path::Path;
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    };

    let mut triangle_shapes: Vec<Vec<Vec<Point>>> = Vec::new();
    let mut tessellate = |rings: &[Vec<Point>]| {
        let mut builder = Path::builder();
        let mut has_path = false;
        for ring in rings.iter().filter(|ring| ring.len() >= 3) {
            builder.begin(lpoint(ring[0].x as f32, ring[0].y as f32));
            for point in &ring[1..] {
                builder.line_to(lpoint(point.x as f32, point.y as f32));
            }
            builder.close();
            has_path = true;
        }
        if !has_path {
            return;
        }
        let path = builder.build();
        let mut buffers: VertexBuffers<Point, u32> = VertexBuffers::new();
        let mut tessellator = FillTessellator::new();
        let options = FillOptions::default()
            .with_fill_rule(FillRule::NonZero)
            .with_tolerance(0.05);
        if tessellator
            .tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
                    Point::new(vertex.position().x as f64, vertex.position().y as f64)
                }),
            )
            .is_err()
        {
            return;
        }
        for triangle in buffers.indices.chunks_exact(3) {
            triangle_shapes.push(vec![vec![
                buffers.vertices[triangle[0] as usize],
                buffers.vertices[triangle[1] as usize],
                buffers.vertices[triangle[2] as usize],
            ]]);
        }
    };

    for rings in ring_sets {
        let mut ordinary_rings = Vec::new();
        let mut additive_cycles = Vec::new();
        for ring in rings.iter().filter(|ring| ring.len() >= 3) {
            let cycles = split_self_intersecting_ring(ring);
            if cycles.len() > 1 {
                additive_cycles.extend(cycles);
            } else {
                ordinary_rings.push(ring.clone());
            }
        }

        // A normal multi-ring shape is kept together so its inner rings remain holes.
        // A self-overlapping contour is split into visible lobes and each lobe is added;
        // opposite winding must not erase an overlap in the authored thread.
        tessellate(&ordinary_rings);
        for cycle in additive_cycles {
            tessellate(&[cycle]);
        }
    }
    union_polygon_ring_sets(&triangle_shapes)
}

fn split_self_intersecting_ring(ring: &[Point]) -> Vec<Vec<Point>> {
    const EPS: f64 = 1e-7;
    if ring.len() < 3 {
        return Vec::new();
    }

    let mut points = ring.to_vec();
    if points.len() > 1 && points[0].distance(*points.last().unwrap()) <= EPS {
        points.pop();
    }
    let segment_count = points.len();
    if segment_count < 3 {
        return Vec::new();
    }

    let mut splits: Vec<Vec<(f64, Point)>> = (0..segment_count)
        .map(|index| {
            vec![
                (0.0, points[index]),
                (1.0, points[(index + 1) % segment_count]),
            ]
        })
        .collect();

    for first in 0..segment_count {
        for second in (first + 1)..segment_count {
            if second == first + 1 || (first == 0 && second + 1 == segment_count) {
                continue;
            }
            let a = points[first];
            let b = points[(first + 1) % segment_count];
            let c = points[second];
            let d = points[(second + 1) % segment_count];
            let rx = b.x - a.x;
            let ry = b.y - a.y;
            let sx = d.x - c.x;
            let sy = d.y - c.y;
            let denominator = rx * sy - ry * sx;
            if denominator.abs() <= EPS {
                continue;
            }
            let qx = c.x - a.x;
            let qy = c.y - a.y;
            let t = (qx * sy - qy * sx) / denominator;
            let u = (qx * ry - qy * rx) / denominator;
            if t > EPS && t < 1.0 - EPS && u > EPS && u < 1.0 - EPS {
                let intersection = Point::new(a.x + rx * t, a.y + ry * t);
                splits[first].push((t, intersection));
                splits[second].push((u, intersection));
            }
        }
    }

    let mut expanded = Vec::new();
    for segment in &mut splits {
        segment.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        segment.dedup_by(|a, b| a.1.distance(b.1) <= EPS);
        for (_, point) in segment.iter().take(segment.len().saturating_sub(1)) {
            if expanded
                .last()
                .is_none_or(|last: &Point| last.distance(*point) > EPS)
            {
                expanded.push(*point);
            }
        }
    }
    expanded.push(expanded[0]);

    fn point_key(point: Point) -> (i64, i64) {
        const SCALE: f64 = 1_000_000.0;
        (
            (point.x * SCALE).round() as i64,
            (point.y * SCALE).round() as i64,
        )
    }

    let mut cycles = Vec::new();
    let mut current = Vec::new();
    let mut positions = std::collections::HashMap::new();
    for point in expanded {
        let key = point_key(point);
        if let Some(&start) = positions.get(&key) {
            if current.len().saturating_sub(start) >= 3 {
                let cycle = current[start..].to_vec();
                if signed_area(&cycle).abs() > EPS {
                    cycles.push(cycle);
                }
            }
            current.truncate(start + 1);
            positions.clear();
            for (index, retained) in current.iter().enumerate() {
                positions.insert(point_key(*retained), index);
            }
        } else {
            positions.insert(key, current.len());
            current.push(point);
        }
    }

    if cycles.is_empty() {
        vec![points]
    } else {
        cycles
    }
}

/// 作図輪郭を一定幅の「糸」に展開し、交差箇所を加算合成した実輪郭へ変換する。
/// SVGのstroke属性ではなく閉じたポリゴンを返すため、TTFにもそのまま渡せる。
pub fn stroke_visible_ring_sets(ring_sets: &[Vec<Vec<Point>>], width: f64) -> Vec<Vec<Point>> {
    use lyon::math::point as lpoint;
    use lyon::path::{LineCap, LineJoin, Path};
    use lyon::tessellation::{
        BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
    };

    let mut triangle_shapes: Vec<Vec<Vec<Point>>> = Vec::new();
    for rings in ring_sets {
        let mut builder = Path::builder();
        let mut has_path = false;
        for ring in rings.iter().filter(|ring| ring.len() >= 2) {
            builder.begin(lpoint(ring[0].x as f32, ring[0].y as f32));
            for point in &ring[1..] {
                builder.line_to(lpoint(point.x as f32, point.y as f32));
            }
            builder.close();
            has_path = true;
        }
        if !has_path {
            continue;
        }
        let path = builder.build();
        let mut buffers: VertexBuffers<Point, u32> = VertexBuffers::new();
        let mut tessellator = StrokeTessellator::new();
        let options = StrokeOptions::default()
            .with_line_width(width.max(0.1) as f32)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round)
            .with_tolerance(0.05);
        if tessellator
            .tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |vertex: StrokeVertex| {
                    Point::new(vertex.position().x as f64, vertex.position().y as f64)
                }),
            )
            .is_err()
        {
            continue;
        }
        for triangle in buffers.indices.chunks_exact(3) {
            triangle_shapes.push(vec![vec![
                buffers.vertices[triangle[0] as usize],
                buffers.vertices[triangle[1] as usize],
                buffers.vertices[triangle[2] as usize],
            ]]);
        }
    }
    union_polygon_ring_sets(&triangle_shapes)
}

/// 閉リングの末尾が先頭と重複していれば落とす
fn dedup_closed(mut ring: Vec<Point>) -> Vec<Point> {
    if ring.len() >= 2 && ring.first().unwrap().distance(*ring.last().unwrap()) < 1e-6 {
        ring.pop();
    }
    ring
}

/// リングの各頂点へ丸いベベルをかける。
/// dist は各辺方向へ後退させる距離。隣り合う丸みが辺上で交差しないよう、
/// 隣接辺長の 49% でクランプし、後退点同士を元頂点を制御点とする
/// 二次ベジェで結ぶ。外周・穴の向きに依存せず同じ丸みになる。
pub fn bevel_ring(ring: &[Point], dist: f64) -> Vec<Point> {
    let n = ring.len();
    if n < 3 || dist <= 1e-6 {
        return ring.to_vec();
    }
    const ROUND_STEPS: usize = 6;
    let mut out = Vec::with_capacity(n * (ROUND_STEPS + 1));
    for i in 0..n {
        let prev = ring[(i + n - 1) % n];
        let cur = ring[i];
        let next = ring[(i + 1) % n];
        let to_prev = prev - cur;
        let to_next = next - cur;
        let lp = to_prev.hypot();
        let ln = to_next.hypot();
        if lp < 1e-9 || ln < 1e-9 {
            out.push(cur);
            continue;
        }
        let dp = dist.min(lp * 0.49);
        let dn = dist.min(ln * 0.49);
        let start = cur + to_prev * (dp / lp);
        let end = cur + to_next * (dn / ln);
        out.push(start);
        for step in 1..=ROUND_STEPS {
            let t = step as f64 / ROUND_STEPS as f64;
            let mt = 1.0 - t;
            out.push(Point::new(
                mt * mt * start.x + 2.0 * mt * t * cur.x + t * t * end.x,
                mt * mt * start.y + 2.0 * mt * t * cur.y + t * t * end.y,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Element, Face, PegRef, Port, StartAnchor};

    fn peg(x: f64, y: f64, r: f64) -> Peg {
        Peg {
            center: Point::new(x, y),
            radius: r,
            shape: PegShape::Circle,
            rounding: 0.0,
        }
    }

    fn square_peg(x: f64, y: f64, r: f64, rounding: f64) -> Peg {
        Peg {
            center: Point::new(x, y),
            radius: r,
            shape: PegShape::RoundedSquare,
            rounding,
        }
    }

    fn wrap_amount(w: &Wrap) -> f64 {
        (w.exit - w.entry) * w.s
    }

    #[test]
    fn straight_segment_misses_peg() {
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(0.0, 60.0));
        update_thread(&mut th, Point::new(200.0, 60.0), &pegs);
        assert!(th.wraps.is_empty());
        assert_eq!(th.tip, Point::new(200.0, 60.0));
    }

    #[test]
    fn rounded_square_boundary_uses_square_extent_and_rounding() {
        let sharp = square_peg(0.0, 0.0, 40.0, 0.0);
        let round = square_peg(0.0, 0.0, 40.0, 40.0);
        let diagonal = std::f64::consts::FRAC_PI_4;
        assert!(on_peg(&sharp, diagonal).distance(Point::ORIGIN) > 56.0);
        assert!((on_peg(&round, diagonal).distance(Point::ORIGIN) - 40.0).abs() < 0.01);
    }

    #[test]
    fn line_through_square_corner_is_captured_even_when_circle_would_miss() {
        let pegs = [square_peg(50.0, 50.0, 30.0, 0.0)];
        let mut thread = Thread::new(Point::new(55.0, 95.0));
        update_thread(&mut thread, Point::new(95.0, 55.0), &pegs);
        assert_eq!(thread.wraps.len(), 1);
    }

    #[test]
    fn segment_through_peg_wraps_on_cursor_side() {
        // アンカー(0,0) → カーソル(200,20)。ペグ(100,0) r=30 を +y 側で巻く
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(0.0, 0.0));
        update_thread(&mut th, Point::new(200.0, 20.0), &pegs);
        assert_eq!(th.wraps.len(), 1);
        let w = &th.wraps[0];
        assert_eq!(w.s, -1.0);
        // entry = π - acos(30/100), exit = atan2(20,100) + acos(30/|(100,20)|)
        assert!((w.entry - 1.87549).abs() < 1e-4, "entry = {}", w.entry);
        assert!((w.exit - 1.46956).abs() < 1e-4, "exit = {}", w.exit);
        assert!(wrap_amount(w) > 0.0);
        // 接線点は両方ともカーソル側（+y）
        assert!(on_circle(pegs[0].center, pegs[0].radius, w.entry).y > 0.0);
        assert!(on_circle(pegs[0].center, pegs[0].radius, w.exit).y > 0.0);
    }

    #[test]
    fn dead_center_crossing_still_wraps() {
        // 中心を正確に突っ切る縮退ケース。試行1で潰れたパターン
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(0.0, 0.0));
        update_thread(&mut th, Point::new(200.0, 0.0), &pegs);
        assert_eq!(th.wraps.len(), 1);
        let w = &th.wraps[0];
        assert!(wrap_amount(w) > 0.5, "wrap amount = {}", wrap_amount(w));
        // 経路の点はどれも円内部に入らない
        let segs = materialize(&th, &pegs, th.tip);
        let mut pts = Vec::new();
        flatten_segs(&segs, &mut pts);
        for p in pts {
            assert!(p.distance(pegs[0].center) > pegs[0].radius - 0.5);
        }
    }

    #[test]
    fn wrap_releases_when_slack() {
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(0.0, 0.0));
        update_thread(&mut th, Point::new(200.0, 20.0), &pegs);
        assert_eq!(th.wraps.len(), 1);
        // カーソルをアンカー側の張らない位置へ戻すと解放される
        update_thread(&mut th, Point::new(0.0, 60.0), &pegs);
        assert!(th.wraps.is_empty());
    }

    #[test]
    fn multi_turn_wrap_accumulates_beyond_tau() {
        // カーソルがペグの周りを1周半すると巻き付き量が 2π を超える
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(-60.0, 0.0));
        let orbit = 120.0;
        let mut max_amount: f64 = 0.0;
        for i in 0..=140 {
            let a = PI + i as f64 * 0.08; // アンカー側から角度増加方向へ約11.2rad
            let c = Point::new(100.0 + orbit * a.cos(), orbit * a.sin());
            update_thread(&mut th, c, &pegs);
            if let Some(w) = th.wraps.first() {
                max_amount = max_amount.max(wrap_amount(w));
            }
        }
        assert_eq!(th.wraps.len(), 1);
        assert!(
            max_amount > 2.0 * PI,
            "max wrap amount = {max_amount} (expected > 2π)"
        );
    }

    #[test]
    fn weave_between_two_pegs_has_opposite_windings() {
        let pegs = [peg(100.0, 0.0, 30.0), peg(200.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(0.0, -60.0));
        // ペグ1の上を通り、ペグ2の下へ抜けるS字。細かく動かして自然な軌跡にする
        let waypoints = [
            Point::new(60.0, 0.0),
            Point::new(150.0, 60.0),
            Point::new(220.0, 30.0),
            Point::new(300.0, -60.0),
        ];
        for wpt in waypoints {
            update_thread(&mut th, wpt, &pegs);
        }
        assert_eq!(th.wraps.len(), 2, "wraps: {:?}", th.wraps);
        assert_eq!(th.wraps[0].peg, 0);
        assert_eq!(th.wraps[1].peg, 1);
        assert!(
            th.wraps[0].s * th.wraps[1].s < 0.0,
            "windings should be opposite"
        );
        assert!(wrap_amount(&th.wraps[0]) > 0.0);
        assert!(wrap_amount(&th.wraps[1]) > 0.0);
    }

    #[test]
    fn cursor_inside_peg_is_clamped_to_boundary() {
        let pegs = [peg(100.0, 0.0, 30.0)];
        let p = clamp_outside_pegs(Point::new(110.0, 5.0), &pegs);
        assert!((p.distance(pegs[0].center) - (30.0 + 0.01)).abs() < 1e-6);
    }

    #[test]
    fn internal_tangent_matches_hand_computed_values() {
        // O1=(0,0) O2=(10,0) r=1: 内接線 φ1 = -acos(0.2), φ2 = φ1 + π
        let p1 = peg(0.0, 0.0, 1.0);
        let p2 = peg(10.0, 0.0, 1.0);
        let (phi1, phi2) = circle_circle_tangent(&p1, 1.0, &p2, -1.0).unwrap();
        let alpha = (0.2f64).acos();
        assert!((norm_pm_pi(phi1) + alpha).abs() < 1e-9);
        let t1 = on_circle(p1.center, p1.radius, phi1);
        let t2 = on_circle(p2.center, p2.radius, phi2);
        // 接線方向が両ペグの進行方向と一致する（＝張った糸として整合）
        let dir = (t2 - t1).normalize();
        let travel1 = kurbo::Vec2::new(-phi1.sin(), phi1.cos()); // s1=+1 の進行方向
        assert!((dir.x - travel1.x).abs() < 1e-9 && (dir.y - travel1.y).abs() < 1e-9);
    }

    #[test]
    fn external_tangent_is_parallel_offset_for_equal_radii() {
        let p1 = peg(0.0, 0.0, 30.0);
        let p2 = peg(200.0, 0.0, 30.0);
        let (phi1, phi2) = circle_circle_tangent(&p1, 1.0, &p2, 1.0).unwrap();
        assert!((norm_pm_pi(phi1) - (-PI / 2.0)).abs() < 1e-9);
        assert!((norm_pm_pi(phi2) - (-PI / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn closing_produces_valid_area() {
        // ペグを1つ巻いて始点へ戻る → 面積を持つ閉路になる
        let pegs = [peg(100.0, 0.0, 30.0)];
        let anchor = Point::new(0.0, 0.0);
        let mut th = Thread::new(anchor);
        update_thread(&mut th, Point::new(200.0, 20.0), &pegs);
        update_thread(&mut th, Point::new(200.0, -80.0), &pegs);
        update_thread(&mut th, Point::new(0.0, -80.0), &pegs);
        update_thread(&mut th, anchor, &pegs);
        assert_eq!(th.wraps.len(), 1);
        let segs = materialize(&th, &pegs, anchor);
        let mut pts = Vec::new();
        flatten_segs(&segs, &mut pts);
        assert!(signed_area(&pts).abs() > 100.0);
    }

    fn make_peg_resolver(pegs: &[Peg]) -> impl Fn(&PegRef) -> Option<Peg> + use<'_> {
        move |r: &PegRef| match *r {
            PegRef::Custom { id } => pegs.get(id as usize).copied(),
            _ => None,
        }
    }

    fn thread_to_face(th: &Thread) -> Face {
        let start = StartAnchor {
            peg: PegRef::Custom { id: 0 },
            port: Port::Top,
        };
        let mut elements = Vec::new();
        for w in &th.wraps {
            let amount = (w.exit - w.entry) * w.s;
            let turns = if amount <= 0.0 {
                0
            } else {
                (amount / (2.0 * PI)).floor() as u32
            };
            elements.push(Element::Wrap {
                peg: PegRef::Custom { id: w.peg as u64 },
                s: w.s,
                turns,
                exit_offset: 0.0,
                sweep_hint: std::f64::consts::PI,
            });
        }
        Face {
            start,
            elements,
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        }
    }

    fn sample_path(segs: &[Seg], samples: usize) -> Vec<Point> {
        if segs.is_empty() || samples == 0 {
            return Vec::new();
        }
        let mut parts: Vec<(f64, f64, Seg)> = Vec::new();
        let mut total = 0.0;
        for seg in segs {
            let len = match *seg {
                Seg::Line { from, to } => from.distance(to),
                Seg::Arc {
                    radius, start, end, ..
                } => (end - start).abs() * radius,
            };
            let next = total + len;
            parts.push((total, next, *seg));
            total = next;
        }
        if total == 0.0 {
            return vec![match segs[0] {
                Seg::Line { from, .. } => from,
                Seg::Arc {
                    center,
                    radius,
                    start,
                    ..
                } => on_circle(center, radius, start),
            }];
        }
        let mut out = Vec::with_capacity(samples);
        for i in 0..samples {
            let t = if samples == 1 {
                0.0
            } else {
                (i as f64) / ((samples - 1) as f64)
            };
            let mut target = t * total;
            if i == samples - 1 {
                target = total;
            }
            for (start_d, end_d, seg) in &parts {
                if target >= *start_d && target <= *end_d + 1e-12 {
                    let seg_t = if (*end_d - *start_d).abs() < 1e-12 {
                        0.0
                    } else {
                        (target - *start_d) / (*end_d - *start_d)
                    };
                    match *seg {
                        Seg::Line { from, to } => {
                            out.push(from + (to - from) * seg_t);
                        }
                        Seg::Arc {
                            center,
                            radius,
                            start,
                            end,
                            ..
                        } => {
                            out.push(on_circle(center, radius, start + (end - start) * seg_t));
                        }
                    }
                    break;
                }
            }
        }
        out
    }

    fn assert_seg_chain_connected(segs: &[Seg]) {
        let mut prev: Option<Point> = None;
        for seg in segs {
            let (from, to) = match *seg {
                Seg::Line { from, to } => (from, to),
                Seg::Arc {
                    center,
                    radius,
                    start,
                    end,
                } => (
                    on_circle(center, radius, start),
                    on_circle(center, radius, end),
                ),
            };
            if let Some(p) = prev {
                assert!(
                    (p.distance(from)).abs() < 1e-8,
                    "segment gap = {}",
                    p.distance(from)
                );
            }
            prev = Some(to);
        }
    }

    #[test]
    fn materialize_face_roundtrip_from_solver() {
        let pegs = [peg(200.0, 100.0, 30.0)];
        let anchor = resolve_anchor(&pegs[0], Port::Top);
        let mut th = Thread::new(anchor);
        update_thread(&mut th, Point::new(360.0, 100.0), &pegs);
        update_thread(&mut th, anchor, &pegs);
        let live = materialize(&th, &pegs, anchor);
        let face = thread_to_face(&th);
        let face = materialize_face(&face, &make_peg_resolver(&pegs)).unwrap();
        let live_pts = sample_path(&live, 200);
        let face_pts = sample_path(&face, 200);
        assert_eq!(live_pts.len(), face_pts.len());
        for (a, b) in live_pts.iter().zip(face_pts.iter()) {
            assert!((a.x - b.x).abs() < 1e-6, "x mismatch: {} vs {}", a.x, b.x);
            assert!((a.y - b.y).abs() < 1e-6, "y mismatch: {} vs {}", a.y, b.y);
        }
    }

    #[test]
    fn materialize_face_is_continuous_after_radius_change() {
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
        let small_r = 44.0;
        let large_r = 72.0;

        let small_resolve = |rref: &PegRef| match *rref {
            PegRef::Grid { row, col } if row == 0 => Some(Peg {
                center: Point::new(col as f64 * 200.0 + 100.0, 100.0),
                radius: small_r,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            _ => None,
        };
        let large_resolve = |rref: &PegRef| match *rref {
            PegRef::Grid { row, col } if row == 0 => Some(Peg {
                center: Point::new(col as f64 * 200.0 + 100.0, 100.0),
                radius: large_r,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            _ => None,
        };

        let small = materialize_face(&face, &small_resolve).unwrap();
        let large = materialize_face(&face, &large_resolve).unwrap();
        assert_seg_chain_connected(&small);
        assert_seg_chain_connected(&large);
    }

    #[test]
    fn completed_wrap_cycle_does_not_snap_to_construction_port() {
        let elements = vec![
            Element::Wrap {
                peg: PegRef::Grid { row: 0, col: 1 },
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: PI,
            },
            Element::Wrap {
                peg: PegRef::Grid { row: 1, col: 1 },
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: PI,
            },
            Element::Wrap {
                peg: PegRef::Grid { row: 1, col: 0 },
                s: 1.0,
                turns: 0,
                exit_offset: 0.0,
                sweep_hint: PI,
            },
        ];
        let make_face = |port| Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port,
            },
            elements: elements.clone(),
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let resolver = |peg_ref: &PegRef| match *peg_ref {
            PegRef::Grid { row, col } => Some(Peg {
                center: Point::new(col as f64 * 180.0, row as f64 * 180.0),
                radius: if row == 0 && col == 0 { 72.0 } else { 46.0 },
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            _ => None,
        };

        let top = materialize_face(&make_face(Port::Top), &resolver).unwrap();
        let right = materialize_face(&make_face(Port::Right), &resolver).unwrap();
        assert_eq!(top.len(), right.len());
        for (a, b) in top.iter().zip(&right) {
            match (*a, *b) {
                (Seg::Line { from: af, to: at }, Seg::Line { from: bf, to: bt }) => {
                    assert!(af.distance(bf) < 1e-8);
                    assert!(at.distance(bt) < 1e-8);
                }
                (
                    Seg::Arc {
                        center: ac,
                        radius: ar,
                        start: as_,
                        end: ae,
                    },
                    Seg::Arc {
                        center: bc,
                        radius: br,
                        start: bs,
                        end: be,
                    },
                ) => {
                    assert!(ac.distance(bc) < 1e-8);
                    assert!((ar - br).abs() < 1e-8);
                    assert!((as_ - bs).abs() < 1e-8);
                    assert!((ae - be).abs() < 1e-8);
                }
                _ => panic!("segment kinds differ"),
            }
        }
        assert!(top.iter().any(|segment| matches!(segment,
            Seg::Arc { center, radius, .. }
                if center.distance(Point::new(0.0, 0.0)) < 1e-8 && (*radius - 72.0).abs() < 1e-8
        )));
    }

    #[test]
    fn newly_enabled_obstacle_is_inserted_into_one_taut_cycle() {
        let a = PegRef::Grid { row: 0, col: 0 };
        let b = PegRef::Grid { row: 0, col: 1 };
        let c = PegRef::Grid { row: 1, col: 0 };
        let sub = PegRef::Sub { row: 0, col: 0 };
        let wrap = |peg| Element::Wrap {
            peg,
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: PI,
        };
        let face = Face {
            start: StartAnchor {
                peg: a,
                port: Port::Top,
            },
            elements: vec![wrap(a), wrap(b), wrap(c)],
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let peg_for = |peg_ref: &PegRef| match *peg_ref {
            x if x == a => Some(Peg {
                center: Point::new(-100.0, 0.0),
                radius: 20.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            x if x == b => Some(Peg {
                center: Point::new(100.0, 0.0),
                radius: 20.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            x if x == c => Some(Peg {
                center: Point::new(0.0, 150.0),
                radius: 20.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            x if x == sub => Some(Peg {
                center: Point::new(0.0, -20.0),
                radius: 18.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            }),
            _ => None,
        };
        let obstacles = [a, b, c, sub]
            .into_iter()
            .map(|peg_ref| (peg_ref, peg_for(&peg_ref).unwrap()))
            .collect::<Vec<_>>();

        let segs = materialize_face_with_obstacles(&face, &peg_for, &obstacles).unwrap();
        assert_eq!(
            segs.iter()
                .filter(|segment| matches!(segment,
                    Seg::Arc { center, .. } if center.distance(Point::new(0.0, -20.0)) < 1e-8
                ))
                .count(),
            1,
            "an automatically caught peg must occur only once in the taut cycle"
        );
        assert_seg_chain_connected(&segs);
        let captured = capture_face_obstacles(&face, &peg_for, &[(sub, peg_for(&sub).unwrap())])
            .expect("moving obstacle should produce a persistent wrap sequence");
        assert_eq!(
            captured
                .iter()
                .filter(|element| matches!(element, Element::Wrap { peg, .. } if *peg == sub))
                .count(),
            1,
            "the caught peg must be committed exactly once"
        );

        for pair in segs.windows(2) {
            match (pair[0], pair[1]) {
                (Seg::Arc { end, start, .. }, Seg::Line { from, to }) => {
                    let tangent = kurbo::Vec2::new(-end.sin(), end.cos()) * (end - start).signum();
                    let line = to - from;
                    assert!(tangent.cross(line).abs() / line.hypot().max(1e-9) < 1e-7);
                    assert!(tangent.dot(line) > 0.0);
                }
                (Seg::Line { from, to }, Seg::Arc { start, end, .. }) => {
                    let tangent =
                        kurbo::Vec2::new(-start.sin(), start.cos()) * (end - start).signum();
                    let line = to - from;
                    assert!(line.cross(tangent).abs() / line.hypot().max(1e-9) < 1e-7);
                    assert!(line.dot(tangent) > 0.0);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn derived_cycle_never_returns_a_line_through_an_existing_contact_peg() {
        let refs = [
            PegRef::Grid { row: 0, col: 0 },
            PegRef::Grid { row: 0, col: 1 },
            PegRef::Grid { row: 0, col: 2 },
            PegRef::Grid { row: 0, col: 3 },
        ];
        let centers = [
            Point::new(-50.0, 0.0),
            Point::new(50.0, 0.0),
            Point::new(150.0, 0.0),
            Point::new(0.0, 0.0),
        ];
        let signs = [-1.0, 1.0, -1.0, -1.0];
        let face = Face {
            start: StartAnchor {
                peg: refs[0],
                port: Port::Left,
            },
            elements: refs
                .iter()
                .zip(signs)
                .map(|(peg, s)| Element::Wrap {
                    peg: *peg,
                    s,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: PI,
                })
                .collect(),
            visible: true,
            ignored_pegs: Vec::new(),
            name: String::new(),
            baked: None,
        };
        let resolver = |peg_ref: &PegRef| {
            refs.iter()
                .position(|candidate| candidate == peg_ref)
                .map(|index| Peg {
                    center: centers[index],
                    radius: 20.0,
                    shape: PegShape::Circle,
                    rounding: 0.0,
                })
        };
        let obstacles: Vec<(PegRef, Peg)> = refs
            .iter()
            .map(|peg_ref| (*peg_ref, resolver(peg_ref).unwrap()))
            .collect();

        if let Some(segs) = materialize_face_with_obstacles(&face, &resolver, &obstacles) {
            let pegs: Vec<Peg> = obstacles.iter().map(|(_, peg)| *peg).collect();
            assert!(!segments_penetrate_pegs(&segs, &pegs));
        }
    }

    #[test]
    fn reverse_face_keeps_curve_flips_area_sign() {
        let face = Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::Right,
            },
            elements: vec![
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: 1 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: std::f64::consts::PI,
                },
                Element::Pin {
                    pos: Point::new(260.0, 220.0),
                },
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: 0 },
                    s: -1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: std::f64::consts::PI,
                },
            ],
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let resolver = |r: &PegRef| -> Option<Peg> {
            match *r {
                PegRef::Grid { row: 0, col: 0 } => Some(Peg {
                    center: Point::new(100.0, 100.0),
                    radius: 30.0,
                    shape: PegShape::Circle,
                    rounding: 0.0,
                }),
                PegRef::Grid { row: 0, col: 1 } => Some(Peg {
                    center: Point::new(280.0, 100.0),
                    radius: 30.0,
                    shape: PegShape::Circle,
                    rounding: 0.0,
                }),
                _ => None,
            }
        };
        let segs = materialize_face(&face, &resolver).unwrap();
        let rev = Face {
            start: face.start,
            elements: face
                .elements
                .iter()
                .rev()
                .map(|e| match *e {
                    Element::Wrap {
                        peg,
                        s,
                        turns,
                        exit_offset,
                        sweep_hint,
                    } => Element::Wrap {
                        peg,
                        s: -s,
                        turns,
                        exit_offset,
                        sweep_hint,
                    },
                    Element::Pin { pos } => Element::Pin { pos },
                })
                .collect(),
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let rev_segs = materialize_face(&rev, &resolver).unwrap();
        let a = {
            let mut pts = Vec::new();
            flatten_segs(&segs, &mut pts);
            signed_area(&pts)
        };
        let b = {
            let mut pts = Vec::new();
            flatten_segs(&rev_segs, &mut pts);
            signed_area(&pts)
        };
        assert!(a.abs() > 0.0);
        assert!((a + b).abs() < 1e-6);
        assert!((a.abs() - b.abs()).abs() < 1e-6);
    }

    #[test]
    fn materialize_chain_follows_tip_without_closing() {
        // 描画中プレビュー: 経路は先端で終わり、始点への閉じ線を含まない
        let pegs = [peg(200.0, 100.0, 30.0), peg(400.0, 100.0, 30.0)];
        let resolver = make_peg_resolver(&pegs);
        let start = resolve_anchor(&pegs[0], Port::Top);
        let tip = Point::new(400.0, 260.0);
        let elements = vec![Element::Wrap {
            peg: PegRef::Custom { id: 1 },
            s: 1.0,
            turns: 0,
            exit_offset: 0.0,
            sweep_hint: std::f64::consts::PI,
        }];
        let segs = materialize_chain(start, &elements, &resolver, tip).unwrap();
        let last_end = match *segs.last().unwrap() {
            Seg::Line { to, .. } => to,
            Seg::Arc {
                center,
                radius,
                end,
                ..
            } => on_circle(center, radius, end),
        };
        assert!(last_end.distance(tip) < 1e-9, "path must end at tip");
        assert!(
            last_end.distance(start) > 100.0,
            "path must not close back to start"
        );
        assert_seg_chain_connected(&segs);
    }

    /// 実マウスのように細かく補間しながらドラッグする
    fn drag(th: &mut Thread, pegs: &[Peg], waypoints: &[Point]) {
        let mut cur = th.tip;
        for &w in waypoints {
            let dist = cur.distance(w);
            let steps = (dist / 4.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f64 / steps as f64;
                let p = Point::new(cur.x + (w.x - cur.x) * t, cur.y + (w.y - cur.y) * t);
                update_thread(th, p, pegs);
            }
            cur = w;
        }
    }

    #[test]
    fn friction_holds_first_peg_when_catching_second() {
        // 実グリッド構成（spacing200/r60/catch6）。十字点開始で A→B と
        // 引っかけたとき、A の巻きが共通接線への滑りで消えないこと
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        let anchor = Point::new(110.0, 44.0); // A の上の十字点
        let mut th = Thread::new(anchor);
        drag(
            &mut th,
            &pegs,
            &[
                Point::new(250.0, 120.0),
                Point::new(420.0, 60.0),
                Point::new(480.0, 200.0),
            ],
        );
        assert!(
            th.wraps.len() >= 2,
            "expected >=2 wraps, got {:?}",
            th.wraps
        );
        assert_eq!(th.wraps[0].peg, 0);
        assert!((th.wraps[0].exit - th.wraps[0].entry) * th.wraps[0].s > 0.0);
    }

    #[test]
    fn recatch_wrapped_peg_from_above() {
        // A→B と巻いた後、A の上をまた通ると A に再度引っかかる
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        let mut th = Thread::new(Point::new(110.0, 44.0));
        drag(
            &mut th,
            &pegs,
            &[
                Point::new(180.0, 250.0),
                Point::new(310.0, 250.0),
                Point::new(420.0, 100.0),
                Point::new(310.0, 20.0),
                Point::new(110.0, 20.0),
                Point::new(40.0, 200.0),
            ],
        );
        let peg0_wraps = th.wraps.iter().filter(|w| w.peg == 0).count();
        assert!(
            th.wraps.len() >= 3 && peg0_wraps >= 2,
            "expected A..A pattern, got {:?}",
            th.wraps
        );
    }

    #[test]
    fn grip_offsets_roundtrip_through_chain() {
        // 摩擦の把持点を含む状態が exit_offset 経由で厳密に再現されること
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        let anchor = Point::new(110.0, 44.0);
        let mut th = Thread::new(anchor);
        drag(
            &mut th,
            &pegs,
            &[
                Point::new(250.0, 120.0),
                Point::new(420.0, 60.0),
                Point::new(480.0, 200.0),
            ],
        );
        assert!(th.wraps.len() >= 2);
        let refs: Vec<PegRef> = (0..pegs.len() as u64)
            .map(|id| PegRef::Custom { id })
            .collect();
        let elements = thread_to_wrap_elements(&th, &pegs, &refs, th.tip).unwrap();
        let chain =
            materialize_chain(anchor, &elements, &make_peg_resolver(&pegs), th.tip).unwrap();
        let live = materialize(&th, &pegs, th.tip);
        let live_pts = sample_path(&live, 300);
        let chain_pts = sample_path(&chain, 300);
        assert_eq!(live_pts.len(), chain_pts.len());
        for (a, b) in live_pts.iter().zip(chain_pts.iter()) {
            assert!(a.distance(*b) < 1e-6, "mismatch: {a:?} vs {b:?}");
        }
    }

    #[test]
    fn live_multi_wrap_is_saved_as_turns_and_roundtrips() {
        let pegs = [peg(0.0, 0.0, 20.0)];
        let refs = [PegRef::Custom { id: 0 }];
        let anchor = Point::new(-60.0, 0.0);
        let tip = Point::new(60.0, 0.0);
        let entry = point_tangent_entry(anchor, &pegs[0], 1.0);
        let th = Thread {
            anchor,
            wraps: vec![Wrap {
                peg: 0,
                s: 1.0,
                entry,
                exit: entry + TAU * 3.0 + PI / 2.0,
            }],
            tip,
        };
        let elements = thread_to_wrap_elements(&th, &pegs, &refs, tip).unwrap();
        assert!(matches!(elements[0], Element::Wrap { turns: 3, .. }));
        let rebuilt = materialize_chain(anchor, &elements, &make_peg_resolver(&pegs), tip).unwrap();
        let original = materialize(&th, &pegs, tip);
        let rebuilt_points = sample_path(&rebuilt, 400);
        let original_points = sample_path(&original, 400);
        for (a, b) in rebuilt_points.iter().zip(original_points.iter()) {
            assert!(a.distance(*b) < 1e-6);
        }
    }

    #[test]
    fn convert_thread_wraps_preserves_turns() {
        let pegs = [peg(100.0, 0.0, 30.0)];
        let mut th = Thread::new(Point::new(-60.0, 0.0));
        let orbit = 120.0;
        for i in 0..=140 {
            let a = PI + i as f64 * 0.08;
            let c = Point::new(100.0 + orbit * a.cos(), orbit * a.sin());
            update_thread(&mut th, c, &pegs);
        }
        let face = thread_to_face(&th);
        assert_eq!(face.elements.len(), 1);
        let wrap = &face.elements[0];
        if let Element::Wrap { turns, .. } = wrap {
            assert_eq!(*turns, 1);
        } else {
            panic!("expected wrap");
        }
    }
}

#[cfg(test)]
mod closing_repro {
    use super::*;
    use crate::model::Port;

    fn peg(x: f64, y: f64, r: f64) -> Peg {
        Peg {
            center: Point::new(x, y),
            radius: r,
            shape: PegShape::Circle,
            rounding: 0.0,
        }
    }

    fn drag(th: &mut Thread, pegs: &[Peg], waypoints: &[Point]) {
        let mut cur = th.tip;
        for &w in waypoints {
            let dist = cur.distance(w);
            let steps = (dist / 4.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f64 / steps as f64;
                let p = Point::new(cur.x + (w.x - cur.x) * t, cur.y + (w.y - cur.y) * t);
                update_thread(th, p, pegs);
            }
            cur = w;
        }
    }

    #[test]
    fn closing_hug_wraps_start_peg_when_chord_misses() {
        // 戻り線が始点円に触れない角度から戻っても、閉じ時は始点ペグへ
        // 接線で巻き込み、弧でアンカーに到達する（直線のカドを作らない）
        // 頭（最後の巻きの離脱点）が右下、アンカーが右上45度: 戻り線は
        // 始点円の右側をかすめる配置 → 短い巻き込みが存在する
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 310.0, 66.0)];
        let anchor = resolve_anchor(&pegs[0], Port::TopRight);
        let mut th = Thread::new(anchor);
        th.wraps.push(Wrap {
            peg: 1,
            s: 1.0,
            entry: -2.656,
            exit: -2.356,
        });
        th.tip = anchor;
        append_closing_hug(&mut th, &pegs, 0, anchor);
        let last = th.wraps.last().expect("hug wrap expected");
        assert_eq!(last.peg, 0, "wraps = {:?}", th.wraps);
        // 弧の終端がアンカー角に一致し、掃引は半周以内
        let anchor_angle = angle_from(pegs[0].center, anchor);
        let end_diff = (last.exit - anchor_angle)
            .rem_euclid(TAU)
            .min((anchor_angle - last.exit).rem_euclid(TAU));
        assert!(
            end_diff < 1e-9,
            "exit {} vs anchor angle {}",
            last.exit,
            anchor_angle
        );
        let sweep = (last.exit - last.entry) * last.s;
        assert!(sweep > 0.0 && sweep <= PI + 0.2, "sweep = {sweep}");
        // 実体化するとアンカーへの直線ではなく弧で終わる
        let segs = materialize(&th, &pegs, anchor);
        assert!(
            matches!(segs.last(), Some(Seg::Arc { .. })),
            "segs = {:?}",
            segs.last()
        );
    }

    #[test]
    fn closing_hugs_start_peg_instead_of_cutting_chord() {
        // 45度ポートから開始し、ぐるっと回って始点ペグの反対側から戻る
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        let anchor = resolve_anchor(&pegs[0], Port::TopRight);
        let mut th = Thread::new(anchor);
        drag(
            &mut th,
            &pegs,
            &[
                Point::new(250.0, 40.0),
                Point::new(400.0, 110.0), // Bの右へ
                Point::new(310.0, 260.0), // Bの下
                Point::new(110.0, 260.0), // Aの下
                Point::new(20.0, 110.0),  // Aの左
                anchor,                   // 始点へ戻る
            ],
        );
        eprintln!("wraps = {:?}", th.wraps);
        let segs = materialize(&th, &pegs, anchor);
        // 閉じ線が始点ペグを貫通していないこと
        for seg in &segs {
            if let Seg::Line { from, to } = seg {
                let mid = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
                let d = mid.distance(pegs[0].center);
                eprintln!(
                    "line ({:.1},{:.1})->({:.1},{:.1}) middist={:.2}",
                    from.x, from.y, to.x, to.y, d
                );
                assert!(
                    d > pegs[0].radius - 1.0,
                    "closing line cuts through start peg: mid dist = {d}"
                );
            }
        }
    }
}

#[cfg(test)]
mod stress {
    use super::*;
    use crate::model::*;

    #[test]
    fn union_of_overlapping_squares_is_single_ring() {
        // 重なる2つの正方形 → ひとつの L字外周（穴なし）
        let a = vec![
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
        ];
        let b = vec![
            Point::new(1.0, 1.0),
            Point::new(3.0, 1.0),
            Point::new(3.0, 3.0),
            Point::new(1.0, 3.0),
        ];
        let rings = union_polygons(&[a, b]);
        assert_eq!(rings.len(), 1, "expected single merged ring");
        // 合体面積は 2*4 - 1(重なり) = 7
        assert!((signed_area(&rings[0]).abs() - 7.0).abs() < 1e-6);
    }

    #[test]
    fn union_resolves_self_crossing_bow_tie_into_visible_regions() {
        let bow_tie = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
            Point::new(100.0, 0.0),
        ];
        let rings = union_visible_ring_sets(&[vec![bow_tie]]);
        assert!(!rings.is_empty());
        assert!(
            rings
                .iter()
                .map(|ring| signed_area(ring).abs())
                .sum::<f64>()
                > 4_000.0
        );
    }

    #[test]
    fn union_keeps_a_contour_retraced_in_the_opposite_direction_filled() {
        let retraced_square = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
            Point::new(0.0, 0.0),
            Point::new(0.0, 100.0),
            Point::new(100.0, 100.0),
            Point::new(100.0, 0.0),
        ];
        let rings = union_visible_ring_sets(&[vec![retraced_square]]);
        let area: f64 = rings.iter().map(|ring| signed_area(ring).abs()).sum();
        assert!(area > 9_000.0, "opposite retrace was knocked out: {area}");
    }

    #[test]
    fn stroked_self_crossing_keeps_both_crossing_branches() {
        let bow_tie = vec![
            Point::new(0.0, 0.0),
            Point::new(100.0, 100.0),
            Point::new(0.0, 100.0),
            Point::new(100.0, 0.0),
        ];
        let rings = stroke_visible_ring_sets(&[vec![bow_tie]], 6.0);
        assert!(!rings.is_empty());
        assert!(
            rings
                .iter()
                .map(|ring| signed_area(ring).abs())
                .sum::<f64>()
                > 1_500.0
        );
    }

    #[test]
    fn union_of_ring_around_hole_keeps_interior() {
        // 大きい四角から内側をまたぐ形 → 穴が残るケースの最小確認: 大四角 + 中四角は
        // 単純に大きい方に含まれるので1リング。ここでは分離した2つ→2リング
        let a = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ];
        let b = vec![
            Point::new(5.0, 5.0),
            Point::new(6.0, 5.0),
            Point::new(6.0, 6.0),
            Point::new(5.0, 6.0),
        ];
        let rings = union_polygons(&[a, b]);
        assert_eq!(rings.len(), 2, "disjoint shapes stay separate");
    }

    #[test]
    fn bevel_rounds_each_corner_with_a_curve() {
        let sq = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let beveled = bevel_ring(&sq, 2.0);
        assert_eq!(beveled.len(), 28, "each corner gets a sampled round arc");
        assert!(
            beveled[3].x + beveled[3].y < 1.8,
            "round bevel bows toward the original corner instead of using a straight chamfer"
        );
        // ベベル後も内側に収まり面積は元より小さい
        assert!(signed_area(&beveled).abs() < signed_area(&sq).abs());
        assert!(signed_area(&beveled).abs() > 90.0);
    }

    #[test]
    fn bevel_zero_is_identity() {
        let sq = vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        assert_eq!(bevel_ring(&sq, 0.0).len(), 3);
    }

    #[test]
    fn grid_scrub_stress_does_not_panic() {
        // グリッド設定を激しく変えながら全面を実体化してもパニックしないこと
        let face = Face {
            start: StartAnchor {
                peg: PegRef::Grid { row: 0, col: 0 },
                port: Port::TopRight,
            },
            elements: vec![
                Element::Wrap {
                    peg: PegRef::Grid { row: 0, col: 1 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.3,
                    sweep_hint: 1.2,
                },
                Element::Wrap {
                    peg: PegRef::Sub { row: 0, col: 0 },
                    s: -1.0,
                    turns: 1,
                    exit_offset: -0.2,
                    sweep_hint: 4.0,
                },
                Element::Pin {
                    pos: Point::new(500.0, 400.0),
                },
                Element::Wrap {
                    peg: PegRef::Grid { row: 1, col: 1 },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: 0.05,
                },
            ],
            visible: true,
            name: String::new(),
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let center = Point::new(640.0, 400.0);
        for i in 0..400 {
            let spacing = 40.0 + (i as f64 * 7.3) % 280.0;
            let radius = (i as f64 * 3.7) % 100.0;
            let catch = (i as f64 * 1.1) % 24.0;
            let sub = (i as f64 * 2.3) % 60.0;
            for anchor in [ScaleAnchor::TopLeft, ScaleAnchor::Center] {
                let grid = GridSettings {
                    spacing,
                    radius,
                    catch,
                    sub_radius: sub,
                    subs_enabled: true,
                    lines_enabled: true,
                    snap_to_lines: true,
                    pegs_enabled: true,
                    scale_link: false,
                    scale_anchor: anchor,
                };
                let resolver = |r: &PegRef| {
                    resolve_peg(
                        &grid,
                        &[],
                        center,
                        crate::model::PegAppearance::default(),
                        r,
                    )
                };
                if let Some(segs) = materialize_face(&face, &resolver) {
                    let mut pts = Vec::new();
                    flatten_segs_tol(&segs, &mut pts, 0.3);
                    let _ = signed_area(&pts);
                }
            }
        }
    }

    #[test]
    fn penetration_detects_line_that_starts_inside_peg() {
        let obstacle = Peg {
            center: Point::new(0.0, 0.0),
            radius: 10.0,
            shape: PegShape::Circle,
            rounding: 0.0,
        };
        let segs = [Seg::Line {
            from: Point::new(-1.0, 0.0),
            to: Point::new(20.0, 0.0),
        }];
        assert!(segments_penetrate_pegs(&segs, &[obstacle]));
    }

    #[test]
    fn penetration_detects_arc_through_another_peg() {
        let obstacle = Peg {
            center: Point::new(0.0, 10.0),
            radius: 3.0,
            shape: PegShape::Circle,
            rounding: 0.0,
        };
        let segs = [Seg::Arc {
            center: Point::new(0.0, 0.0),
            radius: 10.0,
            start: 0.0,
            end: PI,
        }];
        assert!(segments_penetrate_pegs(&segs, &[obstacle]));
    }

    #[test]
    fn impossible_circle_tangent_is_rejected() {
        let a = Peg {
            center: Point::new(0.0, 0.0),
            radius: 60.0,
            shape: PegShape::Circle,
            rounding: 0.0,
        };
        let b = Peg {
            center: Point::new(100.0, 0.0),
            radius: 60.0,
            shape: PegShape::Circle,
            rounding: 0.0,
        };
        assert!(circle_circle_tangent(&a, 1.0, &b, -1.0).is_none());
    }

    #[test]
    fn cycle_arc_direction_matches_outgoing_tangent() {
        let pegs = [
            Peg {
                center: Point::new(0.0, 0.0),
                radius: 10.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            },
            Peg {
                center: Point::new(0.0, 100.0),
                radius: 10.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            },
            Peg {
                center: Point::new(100.0, 0.0),
                radius: 10.0,
                shape: PegShape::Circle,
                rounding: 0.0,
            },
        ];
        let face = Face {
            start: StartAnchor {
                peg: PegRef::Custom { id: 0 },
                port: Port::Top,
            },
            elements: (0..3)
                .map(|id| Element::Wrap {
                    peg: PegRef::Custom { id },
                    s: 1.0,
                    turns: 0,
                    exit_offset: 0.0,
                    sweep_hint: 0.1,
                })
                .collect(),
            name: String::new(),
            visible: true,
            baked: None,
            ignored_pegs: Vec::new(),
        };
        let resolver = |peg_ref: &PegRef| match *peg_ref {
            PegRef::Custom { id } => pegs.get(id as usize).copied(),
            _ => None,
        };
        let segs = materialize_face(&face, &resolver).unwrap();
        for index in 0..segs.len() {
            let (Seg::Arc { end, start, .. }, Seg::Line { from, to }) =
                (segs[index], segs[(index + 1) % segs.len()])
            else {
                continue;
            };
            let direction = (end - start).signum();
            let arc_tangent = kurbo::Vec2::new(-end.sin() * direction, end.cos() * direction);
            let line = to - from;
            assert!(
                arc_tangent.dot(line) > 0.0,
                "arc and line must point the same way"
            );
        }
    }

    #[test]
    fn ring_set_union_keeps_hole_and_disconnected_component() {
        let square = |x0, y0, x1, y1| {
            vec![
                Point::new(x0, y0),
                Point::new(x1, y0),
                Point::new(x1, y1),
                Point::new(x0, y1),
            ]
        };
        let first = vec![square(0.0, 0.0, 10.0, 10.0), square(3.0, 3.0, 7.0, 7.0)];
        let second = vec![square(20.0, 0.0, 24.0, 4.0)];
        let rings = union_polygon_ring_sets(&[first, second]);
        assert_eq!(rings.len(), 3);
    }
}
