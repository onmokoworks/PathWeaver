// Live blends (Illustrator's Object → Blend). The steps between two source
// paths are ordinary generated paths (`blend_of`), rebuilt whenever a source
// changes — move, reshape or recolour either end and the steps follow.
//
// Matching two paths: the one with fewer anchors gets extra anchors on its
// longest segments (split exactly, so its shape does not change); closed paths
// are turned to start at the best-matching anchor and run the same way.
import type {Blend, Document, Gradient, PathNode, RGB, Stroke, Vec2, Width} from './types'
import {gradientOf, mixGradient, opacityOf, setGradient, setOpacity, type PaintKey} from './paint'

type N = Omit<PathNode, 'id'> & {src: number | null}
const sub = (a: Vec2, b: Vec2) => ({x: a.x - b.x, y: a.y - b.y})
const add = (a: Vec2, b: Vec2) => ({x: a.x + b.x, y: a.y + b.y})
const lerpV = (a: Vec2, b: Vec2, t: number) => ({x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t})
const lerp = (a: number, b: number, t: number) => a + (b - a) * t
const dist = (a: Vec2, b: Vec2) => Math.hypot(a.x - b.x, a.y - b.y)

function segLength(a: N, b: N) {
  const c1 = add(a.p, a.outgoing), c2 = add(b.p, b.incoming)
  return (dist(a.p, c1) + dist(c1, c2) + dist(c2, b.p) + dist(a.p, b.p)) / 2
}
/** Split the segment after index i at its middle (de Casteljau), shape kept. */
function split(nodes: N[], i: number) {
  const a = nodes[i], b = nodes[(i + 1) % nodes.length]
  const p0 = a.p, p1 = add(a.p, a.outgoing), p2 = add(b.p, b.incoming), p3 = b.p
  const q0 = lerpV(p0, p1, 0.5), q1 = lerpV(p1, p2, 0.5), q2 = lerpV(p2, p3, 0.5)
  const r0 = lerpV(q0, q1, 0.5), r1 = lerpV(q1, q2, 0.5), m = lerpV(r0, r1, 0.5)
  a.outgoing = sub(q0, p0)
  b.incoming = sub(q2, p3)
  const straight = dist(p0, p1) < 1e-9 && dist(p2, p3) < 1e-9
  const mid: N = {p: m, incoming: straight ? {x: 0, y: 0} : sub(r0, m), outgoing: straight ? {x: 0, y: 0} : sub(r1, m), smooth: !straight, src: null}
  nodes.splice(i + 1, 0, mid)
}
function equalize(nodes: N[], closed: boolean, n: number) {
  while (nodes.length < n) {
    const segs = closed ? nodes.length : nodes.length - 1
    let best = 0, bestLen = -1
    for (let i = 0; i < segs; i++) {
      const l = segLength(nodes[i], nodes[(i + 1) % nodes.length])
      if (l > bestLen) { bestLen = l; best = i }
    }
    split(nodes, best)
  }
}
function reversed(nodes: N[]): N[] {
  return [...nodes].reverse().map(n => ({...n, incoming: n.outgoing, outgoing: n.incoming}))
}
const centroid = (ps: Vec2[]) => ({x: ps.reduce((s, p) => s + p.x, 0) / ps.length, y: ps.reduce((s, p) => s + p.y, 0) / ps.length})
/** Order b's anchors to follow a's (closed: best start and direction; open: best direction). */
function align(a: N[], b: N[], closed: boolean): N[] {
  const ca = centroid(a.map(n => n.p)), cb = centroid(b.map(n => n.p))
  const cost = (bb: N[]) => bb.reduce((s, n, i) => s + dist(sub(a[i].p, ca), sub(n.p, cb)), 0)
  const options = [b, reversed(b)]
  let best = b, bestCost = Infinity
  for (const o of options) {
    const shifts = closed ? o.length : 1
    for (let k = 0; k < shifts; k++) {
      const r = closed ? [...o.slice(k), ...o.slice(0, k)] : o
      const c = cost(r)
      if (c < bestCost) { bestCost = c; best = r }
    }
  }
  return best
}

/** Ring resampled to `count` points by arc length (for contour shapes). */
function resample(ring: Vec2[], count: number): Vec2[] {
  const pts = [...ring, ring[0]]
  const lens = [0]
  for (let i = 1; i < pts.length; i++) lens.push(lens[i - 1] + dist(pts[i - 1], pts[i]))
  const total = lens.at(-1)! || 1
  const out: Vec2[] = []
  let j = 1
  for (let k = 0; k < count; k++) {
    const d = (k / count) * total
    while (j < lens.length - 1 && lens[j] < d) j++
    const t = (d - lens[j - 1]) / Math.max(lens[j] - lens[j - 1], 1e-12)
    out.push(lerpV(pts[j - 1], pts[j], t))
  }
  return out
}
function alignRing(a: Vec2[], b: Vec2[]): Vec2[] {
  let best = b, bestCost = Infinity
  for (const o of [b, [...b].reverse()]) for (let k = 0; k < o.length; k += Math.max(1, Math.floor(o.length / 64))) {
    const r = [...o.slice(k), ...o.slice(0, k)]
    const c = r.reduce((s, p, i) => s + dist(p, a[i]), 0)
    if (c < bestCost) { bestCost = c; best = r }
  }
  return best
}

const mixColor = (a: RGB | null, b: RGB | null, t: number): RGB | null =>
  a && b ? [0, 1, 2].map(i => Math.round(lerp(a[i], b[i], t))) as RGB : (t < 0.5 ? a : b)
/** A solid colour as a gradient shaped like `like` (so it can blend with it). */
const flat = (like: Gradient, color: RGB, opacity: number): Gradient =>
  ({kind: like.kind, matrix: [...like.matrix], stops: like.stops.map(s => ({offset: s.offset, color: [...color] as RGB, opacity}))})
/**
 * One paint (fill or line) between a and b: colour and opacity mix; a side
 * without the paint fades it in from transparent; a gradient on one side
 * meets the other's solid colour stop by stop.
 */
export function mixPaint(out: Omit<Stroke, 'id'>, a: Stroke, b: Stroke, key: PaintKey, t: number) {
  const ca = a[key], cb = b[key]
  let oa = opacityOf(a, key), ob = opacityOf(b, key)
  let ga = ca ? gradientOf(a, key) : null, gb = cb ? gradientOf(b, key) : null
  if (!ca && !cb) { out[key] = null; setOpacity(out as Stroke, key, 1); setGradient(out as Stroke, key, null); return }
  if (!ca) oa = 0
  if (!cb) ob = 0
  if (ga && !gb && cb) gb = flat(ga, cb, 1)
  if (gb && !ga && ca) ga = flat(gb, ca, 1)
  out[key] = mixColor(ca ?? cb, cb ?? ca, t)
  setOpacity(out as Stroke, key, lerp(oa, ob, t))
  setGradient(out as Stroke, key, mixGradient(ga ?? gb, gb ?? ga, t))
}
const mixWidth = (a: Width, b: Width, t: number): Width => ({left: lerp(a.left, b.left, t), right: lerp(a.right, b.right, t), linked: a.linked, angle: lerp(a.angle, b.angle, t)})

/** Why two paths cannot blend, or null. */
export function blendProblem(a: Stroke, b: Stroke): string | null {
  if (a.image || b.image) return '配置画像はブレンドできません'
  if (a.compound != null || b.compound != null) return '複合パスはブレンドできません（解除すると個々のパスでブレンドできます）'
  if (!!a.contours !== !!b.contours) return '線のパスと塗りの形（文字・複合パス）はブレンドできません'
  if (a.contours && b.contours && a.contours.length !== b.contours.length) return `輪郭の数が違う形はブレンドできません（${a.contours.length}と${b.contours.length}）`
  if (!a.contours && a.closed !== b.closed) return '開いたパスと閉じたパスはブレンドできません'
  if (!a.contours && (a.nodes.length < 2 || b.nodes.length < 2)) return 'ブレンドするパスには2つ以上のアンカーが必要です'
  return null
}

/** The `steps` paths between a and b (ids and names left to the caller). */
export function blendSteps(a: Stroke, b: Stroke, steps: number): Omit<Stroke, 'id'>[] {
  const out: Omit<Stroke, 'id'>[] = []
  let na: N[] = [], nb: N[] = []
  let rings: [Vec2[], Vec2[]][] = []
  if (a.contours && b.contours) {
    rings = a.contours.map((ra, i) => {
      const n = Math.max(ra.length, b.contours![i].length, 8)
      const A = resample(ra, n)
      return [A, alignRing(A, resample(b.contours![i], n))]
    })
  } else {
    na = a.nodes.map(n => ({...n, src: n.id}))
    nb = b.nodes.map(n => ({...n, src: n.id}))
    const n = Math.max(na.length, nb.length)
    equalize(na, a.closed, n)
    equalize(nb, b.closed, n)
    nb = align(na, nb, a.closed)
  }
  const cornerOf = (s: Stroke, src: number | null) => (src == null ? 0 : s.corners.find(c => c.node === src)?.radius ?? 0)
  for (let i = 1; i <= steps; i++) {
    const t = i / (steps + 1)
    const near = t < 0.5 ? a : b
    const s: Omit<Stroke, 'id'> = JSON.parse(JSON.stringify({...near, id: undefined}))
    mixPaint(s, a, b, 'fill', t)
    mixPaint(s, a, b, 'edge', t)
    s.edge_width = lerp(a.edge_width, b.edge_width, t)
    s.start_width = mixWidth(a.start_width, b.start_width, t)
    s.end_width = mixWidth(a.end_width, b.end_width, t)
    s.widths = []
    s.text = null
    s.guide = undefined
    if (s.appearance) s.appearance.shared = null
    if (rings.length) {
      s.contours = rings.map(([A, B]) => A.map((p, k) => lerpV(p, B[k], t)))
      s.nodes = []
      s.corners = []
    } else {
      s.contours = null
      s.closed = a.closed
      s.nodes = na.map((p, k) => {
        const q = nb[k]
        return {id: 0, p: lerpV(p.p, q.p, t), incoming: lerpV(p.incoming, q.incoming, t), outgoing: lerpV(p.outgoing, q.outgoing, t), smooth: p.smooth && q.smooth}
      })
      // corners by matched anchor: radius blends (0 where a side had none)
      s.corners = na.map((p, k) => ({k, r: lerp(cornerOf(a, p.src), cornerOf(b, nb[k].src), t), kind: a.corners.find(c => c.node === p.src)?.kind ?? b.corners.find(c => c.node === nb[k].src)?.kind ?? 'Circular'}))
        .filter(c => c.r > 1e-6).map(c => ({node: c.k, radius: c.r, kind: c.kind}))  // node = index for now
    }
    out.push(s)
  }
  return out
}

/**
 * Bring every blend's steps up to date (called on each document change).
 * A blend whose source is gone is released. Step ids are reused, so
 * selection and render caches stay stable while dragging a source.
 */
export function syncBlends(d: Document, nextId: () => number, seen: Map<number, string>) {
  const blends = d.blends ?? []
  if (!blends.length && !d.strokes.some(s => s.blend_of != null)) return
  const live: Blend[] = []
  for (const bl of blends) {
    const a = d.strokes.find(s => s.id === bl.a), b = d.strokes.find(s => s.id === bl.b)
    if (!a || !b || a.blend_of != null || b.blend_of != null || blendProblem(a, b)) continue
    live.push(bl)
  }
  if (live.length !== blends.length) d.blends = live
  const liveIds = new Set(live.map(b => b.id))
  if (d.strokes.some(s => s.blend_of != null && !liveIds.has(s.blend_of))) d.strokes = d.strokes.filter(s => s.blend_of == null || liveIds.has(s.blend_of))
  for (const bl of live) {
    const ia = d.strokes.findIndex(s => s.id === bl.a), ib = d.strokes.findIndex(s => s.id === bl.b)
    const a = d.strokes[ia], b = d.strokes[ib]
    const old = d.strokes.filter(s => s.blend_of === bl.id)
    const sig = JSON.stringify([a, b, bl.steps, ia, ib])
    if (seen.get(bl.id) === sig && old.length === bl.steps) continue
    seen.set(bl.id, sig)
    // back source first, steps in between, front source last
    const [back, front] = ia < ib ? [a, b] : [b, a]
    const steps = blendSteps(back, front, bl.steps)
    const made: Stroke[] = steps.map((s, i) => {
      const id = old[i]?.id ?? nextId()
      const nodes = s.nodes.map(n => ({...n, id: nextId()}))
      const corners = s.corners.map(c => ({...c, node: nodes[c.node as number]?.id ?? 0})).filter(c => c.node)
      return {...s, id, nodes, corners, name: `ブレンド ${i + 1}/${bl.steps}`, blend_of: bl.id, folder: back.folder ?? null} as Stroke
    })
    const rest = d.strokes.filter(s => s.blend_of !== bl.id)
    const at = rest.findIndex(s => s.id === back.id)
    rest.splice(at + 1, 0, ...made)
    d.strokes = rest
  }
}
