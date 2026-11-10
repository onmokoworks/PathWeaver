// SVG path data → absolute cubic Bézier subpaths (lines, quadratics and arcs
// become cubics). Pure geometry, no DOM: unit-tested on its own.
import type {Vec2} from './types'

export interface Cubic {c1: Vec2; c2: Vec2; p: Vec2}
export interface Subpath {start: Vec2; segs: Cubic[]; closed: boolean}

const NUM = /[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/y
function tokenize(d: string): (string | number)[] {
  const out: (string | number)[] = []
  let i = 0
  let lastCmd = ''
  let argIndex = 0
  while (i < d.length) {
    const c = d[i]
    if (/[\s,]/.test(c)) { i++; continue }
    if (/[MmLlHhVvCcSsQqTtAaZz]/.test(c)) { out.push(c); lastCmd = c; argIndex = 0; i++; continue }
    // Arc flags may be written without separators ("a1 1 0 01 5 5").
    if ((lastCmd === 'a' || lastCmd === 'A') && (argIndex % 7 === 3 || argIndex % 7 === 4) && (c === '0' || c === '1')) {
      out.push(c === '1' ? 1 : 0); argIndex++; i++; continue
    }
    NUM.lastIndex = i
    const m = NUM.exec(d)
    if (!m) throw new Error(`パスデータを読めません（${d.slice(i, i + 12)}）`)
    out.push(parseFloat(m[0]))
    argIndex++
    i = NUM.lastIndex
  }
  return out
}

const line = (a: Vec2, b: Vec2): Cubic => ({c1: lerp(a, b, 1 / 3), c2: lerp(a, b, 2 / 3), p: {...b}})
const lerp = (a: Vec2, b: Vec2, t: number): Vec2 => ({x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t})
const quad = (a: Vec2, q: Vec2, b: Vec2): Cubic => ({c1: lerp(a, q, 2 / 3), c2: lerp(b, q, 2 / 3), p: {...b}})

/** Endpoint-parameterized arc → cubics (SVG 1.1 appendix F.6). */
function arc(p0: Vec2, rx: number, ry: number, phiDeg: number, large: boolean, sweep: boolean, p1: Vec2): Cubic[] {
  if (p0.x === p1.x && p0.y === p1.y) return []
  rx = Math.abs(rx); ry = Math.abs(ry)
  if (!rx || !ry) return [line(p0, p1)]
  const phi = phiDeg * Math.PI / 180
  const cos = Math.cos(phi), sin = Math.sin(phi)
  const dx = (p0.x - p1.x) / 2, dy = (p0.y - p1.y) / 2
  const x1 = cos * dx + sin * dy, y1 = -sin * dx + cos * dy
  const lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
  if (lambda > 1) { rx *= Math.sqrt(lambda); ry *= Math.sqrt(lambda) }
  const num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
  const den = rx * rx * y1 * y1 + ry * ry * x1 * x1
  const co = (large === sweep ? -1 : 1) * Math.sqrt(Math.max(0, num / den))
  const cx1 = co * (rx * y1) / ry, cy1 = co * -(ry * x1) / rx
  const cx = cos * cx1 - sin * cy1 + (p0.x + p1.x) / 2
  const cy = sin * cx1 + cos * cy1 + (p0.y + p1.y) / 2
  const ang = (ux: number, uy: number, vx: number, vy: number) => Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
  const t1 = ang(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry)
  let dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry)
  if (!sweep && dt > 0) dt -= 2 * Math.PI
  if (sweep && dt < 0) dt += 2 * Math.PI
  const n = Math.max(1, Math.ceil(Math.abs(dt) / (Math.PI / 2) - 1e-9))
  const step = dt / n
  const k = (4 / 3) * Math.tan(step / 4)
  const pt = (t: number): Vec2 => {
    const x = rx * Math.cos(t), y = ry * Math.sin(t)
    return {x: cos * x - sin * y + cx, y: sin * x + cos * y + cy}
  }
  const dir = (t: number): Vec2 => {
    const x = -rx * Math.sin(t), y = ry * Math.cos(t)
    return {x: cos * x - sin * y, y: sin * x + cos * y}
  }
  const out: Cubic[] = []
  for (let i = 0; i < n; i++) {
    const a = t1 + i * step, b = a + step
    const pa = pt(a), pb = i === n - 1 ? {...p1} : pt(b)
    const da = dir(a), db = dir(b)
    out.push({c1: {x: pa.x + k * da.x, y: pa.y + k * da.y}, c2: {x: pb.x - k * db.x, y: pb.y - k * db.y}, p: pb})
  }
  return out
}

export function parsePath(d: string): Subpath[] {
  const t = tokenize(d)
  const subs: Subpath[] = []
  let cur: Subpath | null = null
  let p: Vec2 = {x: 0, y: 0}
  let start: Vec2 = {x: 0, y: 0}
  let lastC2: Vec2 | null = null   // for S
  let lastQ: Vec2 | null = null    // for T
  let i = 0
  let cmd = ''
  const num = () => {
    const v = t[i++]
    if (typeof v !== 'number') throw new Error('パスデータの数値が足りません')
    return v
  }
  const open = () => {
    if (!cur) { cur = {start: {...p}, segs: [], closed: false}; subs.push(cur) }
    return cur
  }
  while (i < t.length) {
    if (typeof t[i] === 'string') cmd = t[i++] as string
    else if (!cmd) throw new Error('パスデータが M で始まっていません')
    const rel = cmd === cmd.toLowerCase()
    const at = (x: number, y: number): Vec2 => (rel ? {x: p.x + x, y: p.y + y} : {x, y})
    switch (cmd.toUpperCase()) {
      case 'M': {
        p = at(num(), num())
        start = {...p}
        cur = {start: {...p}, segs: [], closed: false}
        subs.push(cur)
        cmd = rel ? 'l' : 'L' // following pairs are line-tos
        lastC2 = lastQ = null
        break
      }
      case 'L': { const q = at(num(), num()); open().segs.push(line(p, q)); p = q; lastC2 = lastQ = null; break }
      case 'H': { const x = num(); const q = {x: rel ? p.x + x : x, y: p.y}; open().segs.push(line(p, q)); p = q; lastC2 = lastQ = null; break }
      case 'V': { const y = num(); const q = {x: p.x, y: rel ? p.y + y : y}; open().segs.push(line(p, q)); p = q; lastC2 = lastQ = null; break }
      case 'C': {
        const c1 = at(num(), num()), c2 = at(num(), num()), q = at(num(), num())
        open().segs.push({c1, c2, p: q}); p = q; lastC2 = c2; lastQ = null; break
      }
      case 'S': {
        const c1: Vec2 = lastC2 ? {x: 2 * p.x - lastC2.x, y: 2 * p.y - lastC2.y} : {...p}
        const c2 = at(num(), num()), q = at(num(), num())
        open().segs.push({c1, c2, p: q}); p = q; lastC2 = c2; lastQ = null; break
      }
      case 'Q': {
        const c = at(num(), num()), q = at(num(), num())
        open().segs.push(quad(p, c, q)); p = q; lastQ = c; lastC2 = null; break
      }
      case 'T': {
        const c: Vec2 = lastQ ? {x: 2 * p.x - lastQ.x, y: 2 * p.y - lastQ.y} : {...p}
        const q = at(num(), num())
        open().segs.push(quad(p, c, q)); p = q; lastQ = c; lastC2 = null; break
      }
      case 'A': {
        const rx = num(), ry = num(), phi = num(), large = num() !== 0, sweep = num() !== 0
        const q = at(num(), num())
        open().segs.push(...arc(p, rx, ry, phi, large, sweep, q)); p = q; lastC2 = lastQ = null; break
      }
      case 'Z': {
        if (cur) cur.closed = true
        p = {...start}
        cur = null
        lastC2 = lastQ = null
        cmd = ''
        break
      }
      default: throw new Error(`未対応のパス命令 ${cmd}`)
    }
  }
  return subs.filter(s => s.segs.length > 0)
}

/** Flattened ring of a subpath (for filled shapes kept as contours). */
export function flatten(s: Subpath, tolerance = 0.25): Vec2[] {
  const out: Vec2[] = [{...s.start}]
  let a: Vec2 = s.start
  for (const c of s.segs) {
    const len = Math.hypot(c.c1.x - a.x, c.c1.y - a.y) + Math.hypot(c.c2.x - c.c1.x, c.c2.y - c.c1.y) + Math.hypot(c.p.x - c.c2.x, c.p.y - c.c2.y)
    const straight = Math.hypot(c.p.x - a.x, c.p.y - a.y)
    const n = len - straight < 1e-6 ? 1 : Math.min(64, Math.max(2, Math.ceil(Math.sqrt(len / tolerance) / 2)))
    for (let i = 1; i <= n; i++) {
      const t = i / n, u = 1 - t
      out.push({
        x: u * u * u * a.x + 3 * u * u * t * c.c1.x + 3 * u * t * t * c.c2.x + t * t * t * c.p.x,
        y: u * u * u * a.y + 3 * u * u * t * c.c1.y + 3 * u * t * t * c.c2.y + t * t * t * c.p.y,
      })
    }
    a = c.p
  }
  if (out.length > 1 && Math.hypot(out[0].x - out.at(-1)!.x, out[0].y - out.at(-1)!.y) < 1e-9) out.pop()
  return out
}

export type Affine = [number, number, number, number, number, number]
const ap = (m: Affine, p: Vec2): Vec2 => ({x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]})
export function transformSubpath(s: Subpath, m: Affine): Subpath {
  return {start: ap(m, s.start), closed: s.closed, segs: s.segs.map(c => ({c1: ap(m, c.c1), c2: ap(m, c.c2), p: ap(m, c.p)}))}
}
