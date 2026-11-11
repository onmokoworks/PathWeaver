// Paint helpers: opacity per paint and gradients. The core owns the format
// (pathweaver-core/src/paint.rs); a gradient is a matrix from its own space
// (linear: (0,0)→(1,0); radial: the unit circle) into the document, so it
// moves with its path.
import type {Gradient, GradientKind, GradientStop, Matrix, RGB, Stroke, Vec2} from './types'

export type PaintKey = 'fill' | 'edge'
export type PaintKind = 'Solid' | GradientKind

const clamp01 = (v: number) => Math.min(1, Math.max(0, v))
const hex2 = (v: number) => Math.round(v).toString(16).padStart(2, '0')
export const hexOf = (c: RGB) => '#' + c.map(hex2).join('')
/** `#rrggbb` with `aa` when not opaque (Tweeq's colour input reads both). */
export const hexAlpha = (c: RGB, a: number) => hexOf(c) + (a >= 1 ? '' : hex2(clamp01(a) * 255))
export function parseHexAlpha(s: string): {color: RGB; opacity: number} {
  const h = s.replace('#', '')
  const full = h.length <= 4 ? [...h].map(c => c + c).join('') : h
  const color = [0, 2, 4].map(i => parseInt(full.slice(i, i + 2), 16) || 0) as RGB
  const opacity = full.length >= 8 ? Math.round(parseInt(full.slice(6, 8), 16) / 255 * 1000) / 1000 : 1
  return {color, opacity}
}

export const opacityOf = (s: Stroke, key: PaintKey) => (key === 'fill' ? s.fill_opacity : s.edge_opacity) ?? 1
export const gradientOf = (s: Stroke, key: PaintKey) => (key === 'fill' ? s.fill_gradient : s.edge_gradient) ?? null
export function setOpacity(s: Stroke, key: PaintKey, v: number) {
  const k = key === 'fill' ? 'fill_opacity' : 'edge_opacity'
  if (v >= 1) delete s[k]; else s[k] = clamp01(v)
}
export function setGradient(s: Stroke, key: PaintKey, g: Gradient | null) {
  const k = key === 'fill' ? 'fill_gradient' : 'edge_gradient'
  if (g) s[k] = g; else delete s[k]
}
export const paintKind = (s: Stroke, key: PaintKey): PaintKind => gradientOf(s, key)?.kind ?? 'Solid'

// ---- matrices ----
export const apply = (m: Matrix, p: Vec2): Vec2 => ({x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]})
export const gradientStart = (g: Gradient) => apply(g.matrix, {x: 0, y: 0})
export const gradientEnd = (g: Gradient) => apply(g.matrix, {x: 1, y: 0})
/** Gradient space placed so that (0,0)→a and (1,0)→b (no skew). */
export function lineMatrix(a: Vec2, b: Vec2): Matrix {
  let dx = b.x - a.x, dy = b.y - a.y
  if (Math.hypot(dx, dy) < 1e-6) { dx = 1e-3; dy = 0 }
  return [dx, dy, -dy, dx, a.x, a.y]
}
export const gradientAngle = (g: Gradient) => {
  const a = gradientStart(g), b = gradientEnd(g)
  return Math.atan2(b.y - a.y, b.x - a.x) * 180 / Math.PI
}
/** Turn a gradient's axis to `deg`, about the middle of a linear axis (the centre of a radial one). */
export function withAngle(g: Gradient, deg: number): Gradient {
  const a = gradientStart(g), b = gradientEnd(g)
  const len = Math.hypot(b.x - a.x, b.y - a.y)
  const r = deg * Math.PI / 180
  const d = {x: Math.cos(r) * len, y: Math.sin(r) * len}
  if (g.kind === 'Radial') return {...g, matrix: lineMatrix(a, {x: a.x + d.x, y: a.y + d.y})}
  const mid = {x: (a.x + b.x) / 2, y: (a.y + b.y) / 2}
  return {...g, matrix: lineMatrix({x: mid.x - d.x / 2, y: mid.y - d.y / 2}, {x: mid.x + d.x / 2, y: mid.y + d.y / 2})}
}

// ---- stops ----
export const sortedStops = (g: Gradient) => [...g.stops].sort((a, b) => a.offset - b.offset)
/** Colour and opacity at position t (0..1), as the core and SVG compute it. */
export function stopAt(g: Gradient, t: number): {color: RGB; opacity: number} {
  const s = sortedStops(g)
  if (!s.length) return {color: [0, 0, 0], opacity: 0}
  if (t <= s[0].offset) return {color: [...s[0].color] as RGB, opacity: s[0].opacity}
  for (let i = 1; i < s.length; i++) {
    const a = s[i - 1], b = s[i]
    if (t <= b.offset) {
      const k = b.offset - a.offset > 1e-12 ? (t - a.offset) / (b.offset - a.offset) : 1
      return {color: a.color.map((c, j) => Math.round(c + (b.color[j] - c) * k)) as RGB, opacity: a.opacity + (b.opacity - a.opacity) * k}
    }
  }
  const last = s[s.length - 1]
  return {color: [...last.color] as RGB, opacity: last.opacity}
}
/** CSS for a horizontal preview of the stops (the gradient bar). */
export function stopsCss(g: Gradient) {
  const parts = sortedStops(g).map(s => `rgba(${s.color.join(', ')}, ${s.opacity}) ${(s.offset * 100).toFixed(2)}%`)
  return `linear-gradient(to right, ${parts.join(', ')})`
}

// ---- making one ----
/** Bounding box of a path's anchors / rings (the gradient's first placement). */
export function strokeBounds(s: Stroke): {min: Vec2; max: Vec2} | null {
  const pts: Vec2[] = s.contours?.flat() ?? s.nodes.map(n => n.p)
  if (!pts.length) return null
  const xs = pts.map(p => p.x), ys = pts.map(p => p.y)
  return {min: {x: Math.min(...xs), y: Math.min(...ys)}, max: {x: Math.max(...xs), y: Math.max(...ys)}}
}
/**
 * A new gradient for a path: left to right across it (linear) or from its
 * centre to its farther side (radial), from its current colour to white —
 * or, switching between linear and radial, the same stops re-placed.
 */
export function makeGradient(s: Stroke, key: PaintKey, kind: GradientKind): Gradient {
  const old = gradientOf(s, key)
  const color = (key === 'fill' ? s.fill : s.edge) ?? [32, 33, 36]
  const opacity = opacityOf(s, key)
  const stops: GradientStop[] = old ? old.stops.map(t => ({...t, color: [...t.color] as RGB}))
    : [{offset: 0, color: [...color] as RGB, opacity}, {offset: 1, color: [255, 255, 255], opacity}]
  const b = strokeBounds(s) ?? {min: {x: 0, y: 0}, max: {x: 100, y: 100}}
  const c = {x: (b.min.x + b.max.x) / 2, y: (b.min.y + b.max.y) / 2}
  const w = Math.max(b.max.x - b.min.x, 1), h = Math.max(b.max.y - b.min.y, 1)
  if (kind === 'Radial') return {kind, stops, matrix: lineMatrix(c, {x: c.x + Math.max(w, h) / 2, y: c.y})}
  if (old) {
    // keep the axis: a radial's centre→rim becomes the line through it
    const a = gradientStart(old), e = gradientEnd(old)
    return {kind, stops, matrix: lineMatrix({x: 2 * a.x - e.x, y: 2 * a.y - e.y}, e)}
  }
  return {kind, stops, matrix: lineMatrix({x: b.min.x, y: c.y}, {x: b.max.x, y: c.y})}
}
/**
 * Switch a paint between solid and a gradient. Going to a gradient folds the
 * paint's opacity into its stops; back to solid takes the first stop's colour
 * and opacity (nothing stays hidden).
 */
export function setPaintKind(s: Stroke, key: PaintKey, kind: PaintKind) {
  const old = gradientOf(s, key)
  if (kind === 'Solid') {
    if (!old) return
    const first = sortedStops(old)[0]
    if (first) {
      if (key === 'fill') s.fill = [...first.color] as RGB; else s.edge = [...first.color] as RGB
      setOpacity(s, key, first.opacity * opacityOf(s, key))
    }
    setGradient(s, key, null)
    return
  }
  if (old?.kind === kind) return
  const g = makeGradient(s, key, kind)
  if (!old) setOpacity(s, key, 1)
  setGradient(s, key, g)
  syncFallback(s, key)
}
/** The solid colour kept beside a gradient follows its first stop (older readers, blends). */
export function syncFallback(s: Stroke, key: PaintKey) {
  const g = gradientOf(s, key)
  const first = g && sortedStops(g)[0]
  if (!first) return
  if (key === 'fill' && s.fill) s.fill = [...first.color] as RGB
  if (key === 'edge' && s.edge) s.edge = [...first.color] as RGB
}

// ---- blends ----
const lerp = (a: number, b: number, t: number) => a + (b - a) * t
/**
 * Between two gradients: matched when both have the same kind and number of
 * stops (stops, positions and placement interpolate); otherwise the nearer one.
 */
export function mixGradient(a: Gradient | null, b: Gradient | null, t: number): Gradient | null {
  if (a && b && a.kind === b.kind && a.stops.length === b.stops.length) {
    const sa = sortedStops(a), sb = sortedStops(b)
    return {
      kind: a.kind,
      matrix: a.matrix.map((v, i) => lerp(v, b.matrix[i], t)) as Matrix,
      stops: sa.map((x, i) => ({
        offset: lerp(x.offset, sb[i].offset, t),
        color: x.color.map((c, j) => Math.round(lerp(c, sb[i].color[j], t))) as RGB,
        opacity: lerp(x.opacity, sb[i].opacity, t),
      })),
    }
  }
  const near = t < 0.5 ? a : b
  return near ? JSON.parse(JSON.stringify(near)) as Gradient : null
}
