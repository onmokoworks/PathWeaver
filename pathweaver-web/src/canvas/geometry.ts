import type {PathNode, Render, Stroke, Vec2} from '../core/types'

export const add = (a: Vec2, b: Vec2): Vec2 => ({x: a.x + b.x, y: a.y + b.y})
export const sub = (a: Vec2, b: Vec2): Vec2 => ({x: a.x - b.x, y: a.y - b.y})
export const scale = (a: Vec2, k: number): Vec2 => ({x: a.x * k, y: a.y * k})
export const len = (a: Vec2) => Math.hypot(a.x, a.y)
export const dist = (a: Vec2, b: Vec2) => Math.hypot(a.x - b.x, a.y - b.y)
export const ZERO: Vec2 = {x: 0, y: 0}

/** Parse the "M x y L x y … Z" strings produced by the core into rings. */
const ringCache = new WeakMap<Render, Vec2[][]>()
export function rings(r: Render): Vec2[][] {
  let cached = ringCache.get(r)
  if (cached) return cached
  cached = []
  let ring: Vec2[] = []
  const re = /([MLZ])\s*(-?[\d.e+-]+)?\s*(-?[\d.e+-]+)?/g
  for (const m of r.hit.matchAll(re)) {
    if (m[1] === 'M') {
      if (ring.length) cached.push(ring)
      ring = [{x: +m[2], y: +m[3]}]
    } else if (m[1] === 'L') ring.push({x: +m[2], y: +m[3]})
    else {
      if (ring.length) cached.push(ring)
      ring = []
    }
  }
  if (ring.length) cached.push(ring)
  ringCache.set(r, cached)
  return cached
}

/** Non-zero winding test against all rings. */
export function inside(p: Vec2, rs: Vec2[][]): boolean {
  let winding = 0
  for (const ring of rs) {
    for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
      const a = ring[j], b = ring[i]
      if (a.y <= p.y) {
        if (b.y > p.y && cross(a, b, p) > 0) winding++
      } else if (b.y <= p.y && cross(a, b, p) < 0) winding--
    }
  }
  return winding !== 0
}
const cross = (a: Vec2, b: Vec2, p: Vec2) => (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)

export interface Bounds {x0: number; y0: number; x1: number; y1: number}
export function bounds(points: Iterable<Vec2>): Bounds | null {
  let b: Bounds | null = null
  for (const p of points) {
    if (!b) b = {x0: p.x, y0: p.y, x1: p.x, y1: p.y}
    else {
      b.x0 = Math.min(b.x0, p.x); b.y0 = Math.min(b.y0, p.y)
      b.x1 = Math.max(b.x1, p.x); b.y1 = Math.max(b.y1, p.y)
    }
  }
  return b
}
export function renderBounds(r: Render, s: Stroke): Bounds | null {
  const pts = rings(r).flat()
  return bounds(pts.length ? pts : s.nodes.map(n => n.p))
}
export function rectHitsRings(b: Bounds, rs: Vec2[][]): boolean {
  const inRect = (p: Vec2) => p.x >= b.x0 && p.x <= b.x1 && p.y >= b.y0 && p.y <= b.y1
  if (rs.some(r => r.some(inRect))) return true
  if (inside({x: (b.x0 + b.x1) / 2, y: (b.y0 + b.y1) / 2}, rs)) return true
  const edges: [Vec2, Vec2][] = [
    [{x: b.x0, y: b.y0}, {x: b.x1, y: b.y0}], [{x: b.x1, y: b.y0}, {x: b.x1, y: b.y1}],
    [{x: b.x1, y: b.y1}, {x: b.x0, y: b.y1}], [{x: b.x0, y: b.y1}, {x: b.x0, y: b.y0}],
  ]
  for (const r of rs) {
    for (let i = 0; i < r.length; i++) {
      const a = r[i], c = r[(i + 1) % r.length]
      if (edges.some(([e0, e1]) => segmentsCross(a, c, e0, e1))) return true
    }
  }
  return false
}
function segmentsCross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) {
  const d1 = cross(c, d, a), d2 = cross(c, d, b), d3 = cross(a, b, c), d4 = cross(a, b, d)
  return d1 * d2 < 0 && d3 * d4 < 0
}

/** SVG path for the editable centerline handles (used for previews). */
export function cubicD(a: PathNode, b: PathNode) {
  const p1 = add(a.p, a.outgoing), p2 = add(b.p, b.incoming)
  return `M${a.p.x} ${a.p.y}C${p1.x} ${p1.y} ${p2.x} ${p2.y} ${b.p.x} ${b.p.y}`
}

export function affine(m: [number, number, number, number, number, number], p: Vec2): Vec2 {
  return {x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]}
}
