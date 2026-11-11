// Type layout: where each glyph goes. Pure and synchronous — the font's
// glyphs come in already shaped (textOutline.ts asks CoreText or the browser),
// and what comes out is one matrix per glyph. Turning the placed glyphs into
// geometry (contours) is a separate step (`placedRings`).
//
// Point text (Illustrator-style): the text origin stays put. Local origin =
// the first line box's top at the alignment point: left text grows to the
// right of x = 0, centred text around it, right text to its left. Each line
// box is size × line_height tall with its baseline centred like CSS
// half-leading (so the on-canvas editor's caret sits on the glyphs).
//
// Text on a path: the same lines, bent along the path's centerline. Each glyph
// sits on the path at its middle, turned to the tangent there. The anchor is
// `offset` (fraction of the path length); alignment works around it like the
// origin of point text. `flip` runs the text the other way, on the other side.
import type {Matrix, PathNode, TextSpec, Vec2} from './types'

/** One glyph from the font: its outline relative to its pen position on the
 *  baseline (y down), its pen x on its line (kerning included, no tracking)
 *  and its advance. */
export interface ShapedGlyph {line: number; x: number; advance: number; rings: Vec2[][]}
export interface Shaped {glyphs: ShapedGlyph[]; lines: number; ascent: number; descent: number}
/** A glyph and where it goes: `m` maps its local outline to text space
 *  (point text) or to the page (text on a path). */
export interface PlacedGlyph {glyph: ShapedGlyph; m: Matrix}

type Spec = Pick<TextSpec, 'size' | 'line_height' | 'align' | 'tracking' | 'baseline_shift'>

/** Distance from the top of a line box to its baseline (CSS half-leading). */
export const baselineIn = (sh: Pick<Shaped, 'ascent' | 'descent'>, size: number, lineHeight: number) =>
  (size * lineHeight - (sh.ascent + sh.descent)) / 2 + sh.ascent

interface Line {glyphs: ShapedGlyph[]; width: number}
/** Lines with their width after tracking (added after every glyph, like CSS letter-spacing). */
function lines(sh: Shaped, t: Spec): Line[] {
  const track = ((t.tracking ?? 0) / 1000) * t.size
  const out: Line[] = Array.from({length: Math.max(1, sh.lines)}, () => ({glyphs: [], width: 0}))
  for (const g of sh.glyphs) out[g.line]?.glyphs.push(g)
  for (const l of out) {
    const last = l.glyphs.at(-1)
    l.width = last ? last.x + last.advance + track * l.glyphs.length : 0
  }
  return out
}

/** Pen x of each glyph on its line, relative to the line start (tracking and justify spread included). */
function penX(l: Line, track: number, spread: number): number[] {
  return l.glyphs.map((g, k) => g.x + k * (track + spread))
}

/** Point text: glyph matrices in text space (see the layout rule above). */
export function layoutPoint(sh: Shaped, t: Spec): PlacedGlyph[] {
  const track = ((t.tracking ?? 0) / 1000) * t.size
  const ls = lines(sh, t)
  const widest = Math.max(0, ...ls.map(l => l.width))
  const lineBox = t.size * t.line_height
  const top = baselineIn(sh, t.size, t.line_height) - (t.baseline_shift ?? 0)
  const out: PlacedGlyph[] = []
  ls.forEach((l, i) => {
    const align = t.align ?? 'left'
    let start = 0, spread = 0
    if (align === 'center') start = -l.width / 2
    else if (align === 'right') start = -l.width
    else if (align === 'justify' && i < ls.length - 1 && l.glyphs.length > 1) spread = (widest - l.width) / (l.glyphs.length - 1)
    const xs = penX(l, track, spread)
    l.glyphs.forEach((g, k) => out.push({glyph: g, m: [1, 0, 0, 1, start + xs[k], i * lineBox + top]}))
  })
  return out
}

// ---- paths ----
/** A path's centerline as a polyline with arc lengths, for placing along it. */
export interface PathMeasure {pts: Vec2[]; len: number[]; total: number; closed: boolean}

const add = (a: Vec2, b: Vec2) => ({x: a.x + b.x, y: a.y + b.y})
const dist = (a: Vec2, b: Vec2) => Math.hypot(a.x - b.x, a.y - b.y)
function bez(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: number): Vec2 {
  const u = 1 - t
  const a = u * u * u, b = 3 * u * u * t, c = 3 * u * t * t, d = t * t * t
  return {x: a * p0.x + b * p1.x + c * p2.x + d * p3.x, y: a * p0.y + b * p1.y + c * p2.y + d * p3.y}
}

/** The centerline of a path (its anchors and handles), or null when it has no length. */
export function measurePath(nodes: PathNode[], closed: boolean): PathMeasure | null {
  if (nodes.length < 2) return null
  const pts: Vec2[] = [nodes[0].p]
  const segs = closed ? nodes.length : nodes.length - 1
  for (let i = 0; i < segs; i++) {
    const a = nodes[i], b = nodes[(i + 1) % nodes.length]
    const p0 = a.p, p1 = add(a.p, a.outgoing), p2 = add(b.p, b.incoming), p3 = b.p
    const rough = dist(p0, p1) + dist(p1, p2) + dist(p2, p3)
    const n = Math.max(1, Math.min(256, Math.ceil(rough / 2)))
    for (let k = 1; k <= n; k++) pts.push(bez(p0, p1, p2, p3, k / n))
  }
  const len = [0]
  for (let i = 1; i < pts.length; i++) len.push(len[i - 1] + dist(pts[i - 1], pts[i]))
  const total = len.at(-1)!
  return total > 1e-9 ? {pts, len, total, closed} : null
}

/** The same path run the other way. */
export function reversePath(m: PathMeasure): PathMeasure {
  const pts = [...m.pts].reverse()
  return {pts, len: m.len.map(v => m.total - v).reverse(), total: m.total, closed: m.closed}
}

/** Point and unit tangent at arc length s. Closed paths wrap around; open
 *  paths continue straight past their ends (nothing typed disappears). */
export function pointAt(m: PathMeasure, s: number): {p: Vec2; t: Vec2} {
  if (m.closed) s = ((s % m.total) + m.total) % m.total
  const {pts, len} = m
  let lo = 0, hi = len.length - 1
  if (s <= 0) hi = 1
  else if (s >= m.total) lo = len.length - 2
  else {
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1
      if (len[mid] <= s) lo = mid
      else hi = mid
    }
  }
  // skip zero-length steps for the tangent
  let a = lo, b = Math.max(hi, lo + 1)
  while (b < pts.length - 1 && dist(pts[a], pts[b]) < 1e-9) b++
  while (a > 0 && dist(pts[a], pts[b]) < 1e-9) a--
  const d = dist(pts[a], pts[b]) || 1
  const t = {x: (pts[b].x - pts[a].x) / d, y: (pts[b].y - pts[a].y) / d}
  const p = {x: pts[lo].x + t.x * (s - len[lo]), y: pts[lo].y + t.y * (s - len[lo])}
  return {p, t}
}

/** The arc length (as a fraction of the path) nearest to q, and how far q is from the path. */
export function nearestOnPath(m: PathMeasure, q: Vec2): {offset: number; distance: number} {
  let best = Infinity, at = 0
  for (let i = 1; i < m.pts.length; i++) {
    const a = m.pts[i - 1], b = m.pts[i]
    const dx = b.x - a.x, dy = b.y - a.y
    const l2 = dx * dx + dy * dy
    const u = l2 > 0 ? Math.max(0, Math.min(1, ((q.x - a.x) * dx + (q.y - a.y) * dy) / l2)) : 0
    const d = Math.hypot(a.x + dx * u - q.x, a.y + dy * u - q.y)
    if (d < best) { best = d; at = m.len[i - 1] + u * (m.len[i] - m.len[i - 1]) }
  }
  return {offset: at / m.total, distance: best}
}

/** Where the first line starts along the path (arc length on the path as
 *  given, before any flip), with the line widths, for the editor and tests. */
function pathStart(l: Line, align: TextSpec['align'], anchor: number) {
  if (align === 'center') return anchor - l.width / 2
  if (align === 'right') return anchor - l.width
  return anchor
}

/** Text on a path: glyph matrices in page space. */
export function layoutOnPath(sh: Shaped, t: Spec & Pick<TextSpec, 'on_path'>, path: PathMeasure): PlacedGlyph[] {
  const track = ((t.tracking ?? 0) / 1000) * t.size
  const flip = !!t.on_path?.flip
  const m = flip ? reversePath(path) : path
  const offset = t.on_path?.offset ?? 0
  const anchor = (flip ? 1 - offset : offset) * path.total
  const lineBox = t.size * t.line_height
  const shift = t.baseline_shift ?? 0
  const out: PlacedGlyph[] = []
  lines(sh, t).forEach((l, i) => {
    let spread = 0
    if (t.align === 'justify' && l.glyphs.length > 1) {
      const room = m.closed ? m.total : m.total - anchor
      if (room > l.width) spread = (room - l.width) / (l.glyphs.length - 1)
    }
    const start = pathStart(l, t.align, anchor)
    const xs = penX(l, track, spread)
    // Later lines sit below the first one (away from the path's left side).
    const off = i * lineBox - shift
    l.glyphs.forEach((g, k) => {
      const mid = start + xs[k] + g.advance / 2
      const {p, t: d} = pointAt(m, mid)
      const n = {x: -d.y, y: d.x}
      out.push({glyph: g, m: [d.x, d.y, n.x, n.y, p.x - d.x * (g.advance / 2) + n.x * off, p.y - d.y * (g.advance / 2) + n.y * off]})
    })
  })
  return out
}

/** Where typing starts on a path: the first line's start point and direction. */
export function pathTextStart(sh: Shaped, t: Spec & Pick<TextSpec, 'on_path'>, path: PathMeasure): {p: Vec2; t: Vec2} {
  const flip = !!t.on_path?.flip
  const m = flip ? reversePath(path) : path
  const offset = t.on_path?.offset ?? 0
  const anchor = (flip ? 1 - offset : offset) * path.total
  const first = lines(sh, t)[0]
  return pointAt(m, t.align === 'justify' ? anchor : pathStart(first, t.align, anchor))
}

export const applyM = (m: Matrix, p: Vec2): Vec2 => ({x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]})

/** Placed glyphs → rings (outer: text space → page, for point text). */
export function placedRings(glyphs: PlacedGlyph[], outer?: Matrix): Vec2[][] {
  const out: Vec2[][] = []
  for (const {glyph, m} of glyphs) {
    for (const r of glyph.rings) out.push(r.map(p => (outer ? applyM(outer, applyM(m, p)) : applyM(m, p))))
  }
  return out
}
