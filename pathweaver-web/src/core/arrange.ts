// Align / distribute and smart guides: pure geometry on bounding boxes.
// The canvas measures the boxes (geometry bounds, like the selection box)
// and applies the moves; nothing here touches the document.
import type {Bounds} from '../canvas/geometry'
import type {Vec2} from './types'

export type AlignKind = 'left' | 'hcenter' | 'right' | 'top' | 'vcenter' | 'bottom'
/** Centres evenly apart, or equal gaps between neighbours. */
export type DistributeKind = 'hcenter' | 'vcenter' | 'hgap' | 'vgap'
/** What to align to (Illustrator's Align To). */
export type AlignTo = 'selection' | 'key' | 'artboard'

export const union = (list: Bounds[]): Bounds | null =>
  list.length ? list.reduce((a, b) => ({x0: Math.min(a.x0, b.x0), y0: Math.min(a.y0, b.y0), x1: Math.max(a.x1, b.x1), y1: Math.max(a.y1, b.y1)})) : null
const cx = (b: Bounds) => (b.x0 + b.x1) / 2
const cy = (b: Bounds) => (b.y0 + b.y1) / 2

/** How far each box moves so it lines up with `to`. */
export function alignMoves(boxes: Bounds[], to: Bounds, kind: AlignKind): Vec2[] {
  return boxes.map(b => {
    switch (kind) {
      case 'left': return {x: to.x0 - b.x0, y: 0}
      case 'hcenter': return {x: cx(to) - cx(b), y: 0}
      case 'right': return {x: to.x1 - b.x1, y: 0}
      case 'top': return {x: 0, y: to.y0 - b.y0}
      case 'vcenter': return {x: 0, y: cy(to) - cy(b)}
      case 'bottom': return {x: 0, y: to.y1 - b.y1}
    }
  })
}

/**
 * How far each box moves to spread them evenly along one axis.
 * `range`: the span to fill (the artboard); default = the boxes' own extremes,
 * whose outer two stay put. `gap`: a fixed spacing (with a key object), laid
 * out in order. `key`: index of the box that must not move (the rest shift
 * along with it).
 */
export function distributeMoves(boxes: Bounds[], kind: DistributeKind, opts: {range?: Bounds; gap?: number; key?: number} = {}): Vec2[] {
  const horizontal = kind === 'hcenter' || kind === 'hgap'
  const lo = (b: Bounds) => (horizontal ? b.x0 : b.y0)
  const hi = (b: Bounds) => (horizontal ? b.x1 : b.y1)
  const mid = (b: Bounds) => (lo(b) + hi(b)) / 2
  const n = boxes.length
  const moves = new Array<number>(n).fill(0)
  if (n >= 2) {
    const order = boxes.map((_, i) => i).sort((a, b) => mid(boxes[a]) - mid(boxes[b]) || a - b)
    const first = boxes[order[0]], last = boxes[order[n - 1]]
    if (kind === 'hcenter' || kind === 'vcenter') {
      // the outer centres: the extremes' own, or set into the range
      const a = opts.range ? (horizontal ? opts.range.x0 : opts.range.y0) + (hi(first) - lo(first)) / 2 : mid(first)
      const z = opts.range ? (horizontal ? opts.range.x1 : opts.range.y1) - (hi(last) - lo(last)) / 2 : mid(last)
      order.forEach((i, k) => { moves[i] = a + ((z - a) * k) / (n - 1) - mid(boxes[i]) })
    } else {
      const sizes = order.map(i => hi(boxes[i]) - lo(boxes[i]))
      const total = sizes.reduce((s, v) => s + v, 0)
      const start = opts.range ? (horizontal ? opts.range.x0 : opts.range.y0) : lo(first)
      const end = opts.range ? (horizontal ? opts.range.x1 : opts.range.y1) : hi(last)
      const gap = opts.gap ?? (end - start - total) / (n - 1)
      let at = start
      order.forEach((i, k) => { moves[i] = at - lo(boxes[i]); at += sizes[k] + gap })
    }
    if (opts.key != null && opts.key >= 0 && opts.key < n) {
      const k = moves[opts.key]
      for (let i = 0; i < n; i++) moves[i] -= k
    }
  }
  return moves.map(m => (horizontal ? {x: m, y: 0} : {x: 0, y: m}))
}

// ---------- smart guides ----------
/** A line to snap to: position `v` on one axis, drawn from `lo` to `hi` on the other. */
export interface SnapLine {v: number; lo: number; hi: number}
export interface SnapTargets {x: SnapLine[]; y: SnapLine[]}
/** A guide segment to draw (world units). */
export interface SmartGuide {a: Vec2; b: Vec2}

/** Edges and centres of the given boxes (other objects, artboards), sorted for search. */
export function snapTargets(boxes: Bounds[]): SnapTargets {
  const x: SnapLine[] = [], y: SnapLine[] = []
  for (const b of boxes) {
    for (const v of [b.x0, cx(b), b.x1]) x.push({v, lo: b.y0, hi: b.y1})
    for (const v of [b.y0, cy(b), b.y1]) y.push({v, lo: b.x0, hi: b.x1})
  }
  x.sort((a, b) => a.v - b.v)
  y.sort((a, b) => a.v - b.v)
  return {x, y}
}
/** First index with lines[i].v >= v. */
function lowerBound(lines: SnapLine[], v: number) {
  let a = 0, b = lines.length
  while (a < b) { const m = (a + b) >> 1; if (lines[m].v < v) a = m + 1; else b = m }
  return a
}
/** The smallest correction that puts one of `values` on a line within `threshold`. */
function nearest(values: number[], lines: SnapLine[], threshold: number): number | null {
  let best: number | null = null
  for (const v of values) {
    const i = lowerBound(lines, v)
    for (const j of [i - 1, i]) {
      const l = lines[j]
      if (!l) continue
      const d = l.v - v
      if (Math.abs(d) <= threshold && (best === null || Math.abs(d) < Math.abs(best))) best = d
    }
  }
  return best
}

/**
 * Snap a moving box. `xs` / `ys`: the positions that may snap (edges and
 * centre for a move, the dragged edge for a resize; empty = that axis is not
 * free). Returns the correction per axis (null: nothing in reach) and the guides to show, through every line
 * that the corrected box touches.
 */
export function smartSnap(box: Bounds, xs: number[], ys: number[], t: SnapTargets, threshold: number): {dx: number | null; dy: number | null; guides: SmartGuide[]} {
  const dx = nearest(xs, t.x, threshold)
  const dy = nearest(ys, t.y, threshold)
  const guides: SmartGuide[] = []
  const eps = 1e-6
  const b = {x0: box.x0 + (dx ?? 0), x1: box.x1 + (dx ?? 0), y0: box.y0 + (dy ?? 0), y1: box.y1 + (dy ?? 0)}
  const seen = new Set<string>()
  if (dx !== null) {
    for (const v of xs.map(v => v + dx)) {
      const hit = t.x.filter(l => Math.abs(l.v - v) < eps)
      if (!hit.length || seen.has(`x${v.toFixed(6)}`)) continue
      seen.add(`x${v.toFixed(6)}`)
      const lo = Math.min(b.y0, ...hit.map(l => l.lo)), hi = Math.max(b.y1, ...hit.map(l => l.hi))
      guides.push({a: {x: v, y: lo}, b: {x: v, y: hi}})
    }
  }
  if (dy !== null) {
    for (const v of ys.map(v => v + dy)) {
      const hit = t.y.filter(l => Math.abs(l.v - v) < eps)
      if (!hit.length || seen.has(`y${v.toFixed(6)}`)) continue
      seen.add(`y${v.toFixed(6)}`)
      const lo = Math.min(b.x0, ...hit.map(l => l.lo)), hi = Math.max(b.x1, ...hit.map(l => l.hi))
      guides.push({a: {x: lo, y: v}, b: {x: hi, y: v}})
    }
  }
  return {dx, dy, guides}
}
/** The snappable positions of a box on one axis: both edges and the centre. */
export const boxXs = (b: Bounds) => [b.x0, cx(b), b.x1]
export const boxYs = (b: Bounds) => [b.y0, cy(b), b.y1]
