// Illustrator's shape tools as path generators. A shape is made from a few
// parameters (its frame, sides, radii…) into ordinary anchors and handles: the
// result is a normal path you can edit point by point. Until something else
// is done, the parameters stay adjustable (Blender's "Adjust Last Operation").
import type {PathNode, Vec2} from './types'

export type ShapeKind = 'rect' | 'rounded' | 'ellipse' | 'polygon' | 'star' | 'line' | 'arc' | 'spiral'

export const SHAPES: {kind: ShapeKind; label: string; icon: string; key?: string}[] = [
  {kind: 'rect', label: '長方形', icon: 'mdi:rectangle-outline', key: 'M'},
  {kind: 'rounded', label: '角丸長方形', icon: 'mdi:square-rounded-outline'},
  {kind: 'ellipse', label: '楕円', icon: 'mdi:ellipse-outline', key: 'L'},
  {kind: 'polygon', label: '多角形', icon: 'mdi:pentagon-outline'},
  {kind: 'star', label: 'スター', icon: 'mdi:star-outline'},
  {kind: 'line', label: '直線', icon: 'mdi:vector-line', key: '\\'},
  {kind: 'arc', label: '円弧', icon: 'mdi:vector-curve'},
  {kind: 'spiral', label: 'スパイラル', icon: 'mdi:rotate-orbit'},
]
export const shapeInfo = (k: ShapeKind) => SHAPES.find(s => s.kind === k)!

/**
 * Box shapes (rect, rounded, ellipse, arc) live in x, y, w, h (w/h may be
 * negative: the drag direction). Radial shapes (polygon, star, spiral) live in
 * a centre x, y, a radius and an angle. A line runs from x, y by w, h.
 */
export interface ShapeParams {
  kind: ShapeKind
  x: number; y: number; w: number; h: number
  radius: number          // polygon / star outer / spiral outer
  radius2: number         // star inner
  sides: number           // polygon sides, star points, spiral segments
  angle: number           // radial shapes: degrees, 0 = first point straight up
  corner: number          // rounded rect / polygon corner radius
  decay: number           // spiral: each quarter turn keeps this share (%)
  clockwise: boolean      // spiral direction
  concave: boolean        // arc bulges inwards
  closed: boolean         // arc: closed into a quarter pie
}
export function defaults(kind: ShapeKind): ShapeParams {
  return {kind, x: 0, y: 0, w: 100, h: 100, radius: 50, radius2: 25, sides: kind === 'star' ? 5 : kind === 'spiral' ? 10 : 6, angle: 0, corner: kind === 'rounded' ? 12 : 0, decay: 80, clockwise: true, concave: false, closed: false}
}

type Raw = Omit<PathNode, 'id'>
const KAPPA = 0.5522847498307936
const P = (x: number, y: number): Vec2 => ({x, y})
const corner = (p: Vec2): Raw => ({p, incoming: P(0, 0), outgoing: P(0, 0), smooth: false})

export interface BuiltShape {nodes: Raw[]; closed: boolean; corners: number}

export function buildShape(s: ShapeParams): BuiltShape {
  switch (s.kind) {
    case 'rect': case 'rounded': {
      const x0 = Math.min(s.x, s.x + s.w), y0 = Math.min(s.y, s.y + s.h), x1 = Math.max(s.x, s.x + s.w), y1 = Math.max(s.y, s.y + s.h)
      // Clockwise from top-left, like Illustrator; corners stay live (角丸).
      return {nodes: [P(x0, y0), P(x1, y0), P(x1, y1), P(x0, y1)].map(corner), closed: true, corners: s.kind === 'rounded' || s.corner > 0 ? s.corner : 0}
    }
    case 'ellipse': {
      const rx = Math.abs(s.w) / 2, ry = Math.abs(s.h) / 2
      const cx = Math.min(s.x, s.x + s.w) + rx, cy = Math.min(s.y, s.y + s.h) + ry
      const kx = rx * KAPPA, ky = ry * KAPPA
      const nodes: Raw[] = [
        {p: P(cx, cy - ry), incoming: P(-kx, 0), outgoing: P(kx, 0), smooth: true},
        {p: P(cx + rx, cy), incoming: P(0, -ky), outgoing: P(0, ky), smooth: true},
        {p: P(cx, cy + ry), incoming: P(kx, 0), outgoing: P(-kx, 0), smooth: true},
        {p: P(cx - rx, cy), incoming: P(0, ky), outgoing: P(0, -ky), smooth: true},
      ]
      return {nodes, closed: true, corners: 0}
    }
    case 'polygon': case 'star': {
      const n = Math.max(s.kind === 'star' ? 2 : 3, Math.round(s.sides))
      const count = s.kind === 'star' ? n * 2 : n
      const a0 = (s.angle - 90) * Math.PI / 180
      const nodes: Raw[] = []
      for (let i = 0; i < count; i++) {
        const r = s.kind === 'star' && i % 2 ? s.radius2 : s.radius
        const a = a0 + (i * 2 * Math.PI) / count
        nodes.push(corner(P(s.x + Math.cos(a) * r, s.y + Math.sin(a) * r)))
      }
      return {nodes, closed: true, corners: s.corner}
    }
    case 'line':
      return {nodes: [corner(P(s.x, s.y)), corner(P(s.x + s.w, s.y + s.h))], closed: false, corners: 0}
    case 'arc': {
      // A quarter ellipse from the start corner to the opposite one (Illustrator's Arc).
      const a = P(s.x, s.y), b = P(s.x + s.w, s.y + s.h)
      // Convex: bulges away from the corner (x+w, y); concave: towards it.
      const k = KAPPA
      const out = s.concave ? P(s.w * k, 0) : P(0, s.h * k)
      const inn = s.concave ? P(0, -s.h * k) : P(-s.w * k, 0)
      const nodes: Raw[] = [
        {p: a, incoming: P(0, 0), outgoing: out, smooth: false},
        {p: b, incoming: inn, outgoing: P(0, 0), smooth: false},
      ]
      // Closed: back through the ellipse's centre (a quarter pie).
      if (s.closed) nodes.push(corner(s.concave ? P(s.x, s.y + s.h) : P(s.x + s.w, s.y)))
      return {nodes, closed: s.closed, corners: 0}
    }
    case 'spiral': {
      // Quarter turns, each radius = previous × decay, around a moving centre
      // so the curve stays smooth (Illustrator's Spiral). Starts outside.
      const segs = Math.max(2, Math.min(200, Math.round(s.sides)))
      const decay = Math.max(5, Math.min(99, s.decay)) / 100
      const dir = s.clockwise ? 1 : -1
      let r = s.radius
      let a = (s.angle - 90) * Math.PI / 180
      let p = P(s.x + Math.cos(a) * r, s.y + Math.sin(a) * r)
      // centre of the current quarter
      let c = P(s.x, s.y)
      const nodes: Raw[] = [corner(p)]
      for (let i = 0; i < segs; i++) {
        const a1 = a + dir * Math.PI / 2
        const q = P(c.x + Math.cos(a1) * r, c.y + Math.sin(a1) * r)
        const t0 = P(-Math.sin(a) * dir, Math.cos(a) * dir), t1 = P(-Math.sin(a1) * dir, Math.cos(a1) * dir)
        const h = r * KAPPA
        nodes.at(-1)!.outgoing = P(t0.x * h, t0.y * h)
        const next = r * decay
        nodes.push({p: q, incoming: P(-t1.x * h, -t1.y * h), outgoing: P(0, 0), smooth: true})
        // the next quarter shrinks towards the point just reached
        c = P(q.x - Math.cos(a1) * next, q.y - Math.sin(a1) * next)
        r = next
        a = a1
      }
      nodes[0].smooth = false
      nodes.at(-1)!.smooth = false
      return {nodes, closed: false, corners: 0}
    }
  }
}

/** Parameters from a drag (start → current), with Illustrator's modifiers. */
export function fromDrag(kind: ShapeKind, base: ShapeParams, a: Vec2, b: Vec2, shift: boolean, alt: boolean, angleStep: number): ShapeParams {
  const s: ShapeParams = {...base, kind}
  let dx = b.x - a.x, dy = b.y - a.y
  switch (kind) {
    case 'rect': case 'rounded': case 'ellipse': case 'arc': {
      if (shift) { const m = Math.max(Math.abs(dx), Math.abs(dy)); dx = Math.sign(dx || 1) * m; dy = Math.sign(dy || 1) * m }
      // Option: from the centre (not for the arc, which runs corner to corner).
      if (alt && kind !== 'arc') return {...s, x: a.x - dx, y: a.y - dy, w: 2 * dx, h: 2 * dy}
      return {...s, x: a.x, y: a.y, w: dx, h: dy}
    }
    case 'line': {
      if (shift) {
        const l = Math.hypot(dx, dy), st = angleStep * Math.PI / 180
        const ang = Math.round(Math.atan2(dy, dx) / st) * st
        dx = Math.cos(ang) * l; dy = Math.sin(ang) * l
      }
      if (alt) return {...s, x: a.x - dx, y: a.y - dy, w: 2 * dx, h: 2 * dy}
      return {...s, x: a.x, y: a.y, w: dx, h: dy}
    }
    case 'polygon': case 'star': case 'spiral': {
      // From the centre; the drag sets the size and (without Shift) the turn.
      const r = Math.hypot(dx, dy)
      const angle = shift ? 0 : Math.atan2(dy, dx) * 180 / Math.PI + 90
      const ratio = base.radius > 0 ? base.radius2 / base.radius : 0.5
      return {...s, x: a.x, y: a.y, radius: r, radius2: r * ratio, angle}
    }
  }
}
