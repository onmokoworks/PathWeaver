// Glyph outlines for live text. The text keeps its characters, font and type
// settings; its shape is the glyphs laid out from them, stored as the
// stroke's contours (so fill, outline and the appearance graph apply to text
// like to any shape).
//
// Three steps, kept apart:
//  1. shapeText  — the font's glyphs for the characters (outline, pen x with
//     kerning, advance). Mac app: CoreText (with font fallback for Japanese).
//     Browser: each character drawn on a canvas and traced back (close
//     enough to develop and test with).
//  2. textLayout — where every glyph goes (alignment, tracking, baseline
//     shift, text on a path). Pure and synchronous.
//  3. contours  — placed glyph outlines, normalized like any filled shape.
//
// The text stays live: `syncTexts` (run on every document change) lays a
// text out again when its settings, or the path it sits on, changed.
import {call} from './wasm'
import type {Editor} from './editor'
import type {Document, Matrix, Stroke, TextSpec, Vec2} from './types'
import {nodesOf, parsePath, transformSubpath} from './svgPath'
import {pathsFromRings, type Ring} from './compound'
import {baselineIn, layoutOnPath, layoutPoint, measurePath, pathTextStart, placedRings, type PathMeasure, type Shaped, type ShapedGlyph} from './textLayout'

export const DEFAULT_FONT = 'Hiragino Sans'
type Post = (m: {type: string; [k: string]: unknown}) => void

let post: Post | null = null
let nextRequest = 1
interface HostShape {ascent: number; descent: number; glyphs: {l: number; x: number; a: number; r: number[][]}[]}
const waiting = new Map<number, (r: {shape?: HostShape; contours?: number[][]; curves?: string[]; error?: string}) => void>()
let fontList: string[] | null = null
const fontWaiters: ((list: string[]) => void)[] = []

/** Called by the host bridge in the Mac app. */
export function initTextHost(p: Post | null) {
  post = p
  if (!window.pathweaver) return
  Object.assign(window.pathweaver, {
    shaped: ((id: number, shape: HostShape | null, error?: string) => {
      waiting.get(id)?.(shape ? {shape} : {error: error ?? '文字の形を作れませんでした'})
      waiting.delete(id)
    }) as never,
    // Glyph outlines as path data (curves kept), for Create Outlines.
    glyphCurves: ((id: number, curves: string[] | null, error?: string) => {
      waiting.get(id)?.(curves ? {curves} : {error: error ?? '文字の形を作れませんでした'})
      waiting.delete(id)
    }) as never,
    fonts: ((list: string[]) => {
      fontList = list
      for (const w of fontWaiters.splice(0)) w(list)
    }) as never,
  })
}

const BROWSER_FONTS = ['Hiragino Sans', 'Hiragino Mincho ProN', 'Helvetica Neue', 'Georgia', 'Menlo', 'sans-serif', 'serif', 'monospace']
/** Installed font families (the Mac app asks the system). */
export function fontFamilies(): Promise<string[]> {
  if (fontList) return Promise.resolve(fontList)
  if (!post) return Promise.resolve(BROWSER_FONTS)
  return new Promise(resolve => {
    fontWaiters.push(resolve)
    if (fontWaiters.length === 1) post!({type: 'fonts'})
  })
}

// ---------- 1. shaping ----------
type ShapeKey = Pick<TextSpec, 'text' | 'font' | 'size'> & {kerning?: boolean}
const shapeCache = new Map<string, Shaped>()
const keyOf = (t: ShapeKey) => JSON.stringify([t.text, t.font, t.size, t.kerning !== false, !!post])
/** The shaped glyphs if already known (synchronous re-layout while dragging). */
export const shapedNow = (t: ShapeKey): Shaped | undefined => shapeCache.get(keyOf(t))
/** The font's glyphs for the text, one line per '\n'. */
export async function shapeText(t: ShapeKey): Promise<Shaped> {
  const key = keyOf(t)
  const hit = shapeCache.get(key)
  if (hit) return hit
  const lines = t.text.split('\n').length
  let sh: Shaped
  if (!t.text.trim()) sh = {glyphs: [], lines, ...metrics(t.font, t.size)}
  else if (post) sh = await nativeShape(t)
  else sh = browserShape(t)
  if (shapeCache.size > 200) shapeCache.clear()
  shapeCache.set(key, sh)
  return sh
}

function nativeShape(t: ShapeKey): Promise<Shaped> {
  const id = nextRequest++
  return new Promise((resolve, reject) => {
    waiting.set(id, r => {
      if (r.error || !r.shape) return reject(new Error(r.error ?? '文字の形を作れませんでした'))
      const glyphs: ShapedGlyph[] = r.shape.glyphs.map(g => ({
        line: g.l, x: g.x, advance: g.a,
        // [x0, y0, x1, y1, …] per ring
        rings: g.r.map(flat => {
          const ring: Vec2[] = []
          for (let i = 0; i + 1 < flat.length; i += 2) ring.push({x: flat[i], y: flat[i + 1]})
          return ring
        }),
      }))
      resolve({glyphs, lines: t.text.split('\n').length, ascent: r.shape.ascent, descent: r.shape.descent})
    })
    post!({type: 'shape', id, text: t.text, font: t.font, size: t.size, kerning: t.kerning !== false})
  })
}

const cssFont = (family: string, px: number) => `${px}px ${/^(sans-serif|serif|monospace)$/.test(family) ? family : JSON.stringify(family)}`

/** The font's ascent / descent (browser measure; the Mac app sends its own). */
function metrics(family: string, size: number): {ascent: number; descent: number} {
  const g = document.createElement('canvas').getContext('2d')!
  g.font = cssFont(family, 100)
  const m = g.measureText('Hg')
  return {ascent: (m.fontBoundingBoxAscent ?? 88) / 100 * size, descent: (m.fontBoundingBoxDescent ?? 12) / 100 * size}
}

const glyphCache = new Map<string, Vec2[][]>()
/** Browser: each character traced on its own (its pen origin on the baseline). */
function browserShape(t: ShapeKey): Shaped {
  const k = Math.min(8, 128 / t.size)          // canvas px per unit: ~128 px em
  const px = t.size * k
  const g = document.createElement('canvas').getContext('2d')!
  g.font = cssFont(t.font, px)
  // Kerning shows in the width of a run, so pen positions are measured as prefixes.
  ;(g as unknown as {fontKerning: string}).fontKerning = t.kerning === false ? 'none' : 'normal'
  const {ascent, descent} = metrics(t.font, t.size)
  const glyphs: ShapedGlyph[] = []
  t.text.split('\n').forEach((line, li) => {
    const chars = Array.from(line)
    const long = chars.length > 400   // prefixes get slow: plain advances instead
    let x = 0
    chars.forEach((ch, i) => {
      const next = long ? x + g.measureText(ch).width / k : g.measureText(chars.slice(0, i + 1).join('')).width / k
      glyphs.push({line: li, x, advance: next - x, rings: traceGlyph(ch, t.font, t.size)})
      x = next
    })
  })
  return {glyphs, lines: t.text.split('\n').length, ascent, descent}
}
function traceGlyph(ch: string, font: string, size: number): Vec2[][] {
  if (!ch.trim()) return []
  const key = JSON.stringify([ch, font, size])
  const hit = glyphCache.get(key)
  if (hit) return hit
  const k = Math.min(8, 128 / size)
  const px = size * k
  const c = document.createElement('canvas')
  const g = c.getContext('2d')!
  g.font = cssFont(font, px)
  const m = g.measureText(ch)
  const asc = m.fontBoundingBoxAscent ?? px * 0.88
  const desc = m.fontBoundingBoxDescent ?? px * 0.12
  const pad = Math.ceil(px * 0.5)
  c.width = Math.min(4000, Math.ceil(m.width + 2 * pad))
  c.height = Math.min(4000, Math.ceil(asc + desc + 2 * pad))
  g.font = cssFont(font, px)
  g.fillStyle = '#000'
  g.fillText(ch, pad, pad + asc)
  const rings = call<Vec2[][]>('traceText', {png: c.toDataURL('image/png'), scale: 1 / k})
  const ox = pad / k, oy = (pad + asc) / k
  const out = rings.map(r => r.map(p => ({x: p.x - ox, y: p.y - oy})))
  if (glyphCache.size > 2000) glyphCache.clear()
  glyphCache.set(key, out)
  return out
}

// ---------- 2 + 3. layout → contours ----------
/** Fonts are drawn with the nonzero rule; normalize like any filled shape. */
const normalize = (rings: Vec2[][]) => (rings.length ? call<Vec2[][]>('normalizeRings', {rings, evenodd: false}) : [])

/** Outlines of point text in its local space (no matrix, no path). */
export async function outlineText(t: Pick<TextSpec, 'text' | 'font' | 'size' | 'line_height'> & Partial<TextSpec>): Promise<Vec2[][]> {
  return normalize(placedRings(layoutPoint(await shapeText(t), t)))
}

/** Distance from the top of a line box to its baseline (CSS half-leading),
 *  from the font's own ascent / descent. */
export function baselineTop(family: string, size: number, lineHeight: number): number {
  return baselineIn(metrics(family, size), size, lineHeight)
}

export const apply = (m: Matrix, p: Vec2): Vec2 => ({x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]})

/** The path a text sits on, measured — or null (gone, or not a path with length). */
export function guidePath(d: Document, t: TextSpec): PathMeasure | null {
  const id = t.on_path?.stroke
  if (id == null) return null
  const p = d.strokes.find(s => s.id === id)
  if (!p || p.text || p.image || p.contours || p.blend_of != null) return null
  return measurePath(p.nodes, p.closed)
}

/** Can this stroke carry text? (an ordinary path with at least two anchors) */
export const canCarryText = (s: Stroke | undefined): s is Stroke =>
  !!s && !s.text && !s.image && !s.contours && s.blend_of == null && s.nodes.length >= 2

/** Lay the text out now with its shaped glyphs (false: not shaped yet). */
function place(d: Document, s: Stroke): boolean {
  const t = s.text
  if (!t) return true
  const sh = shapedNow(t)
  if (!sh) return false
  const path = t.on_path ? guidePath(d, t) : null
  s.contours = path
    ? normalize(placedRings(layoutOnPath(sh, t, path)))
    : normalize(placedRings(layoutPoint(sh, t), t.matrix))
  return true
}

/**
 * Re-lay out a text stroke's outlines after its text or font changed. Results
 * that arrive after a newer request are dropped. Resolves when applied.
 */
const latest = new Map<number, number>()
export async function relayout(ed: Editor, s: Stroke): Promise<void> {
  const t = s.text
  if (!t) return
  const ticket = (latest.get(s.id) ?? 0) + 1
  latest.set(s.id, ticket)
  try {
    await shapeText(t)
  } catch (e) {
    ed.error = e instanceof Error ? e.message : String(e)
    return
  }
  if (latest.get(s.id) !== ticket) return
  const now = ed.findStroke(s.id)
  if (!now?.text) return
  if (!shapedNow(now.text)) return relayout(ed, now) // the text changed meanwhile
  place(ed.doc, now)
  seen.set(now.id, signature(ed.doc, now))
  ed.changed()
}

// ---------- live ----------
// What a text's shape depends on. Point text: its own settings (moving it
// transforms the outlines with the matrix, no new layout). On a path: the
// path's anchors as well.
const seen = new Map<number, string>()
function signature(d: Document, s: Stroke): string {
  const {matrix: _m, ...rest} = s.text!
  const p = s.text!.on_path ? d.strokes.find(v => v.id === s.text!.on_path!.stroke) : undefined
  return JSON.stringify([rest, p ? [p.nodes, p.closed] : null])
}
/** A document just loaded: its saved outlines stand (nothing re-laid out). */
export function primeTexts(d: Document) {
  seen.clear()
  for (const s of d.strokes) if (s.text) seen.set(s.id, signature(d, s))
}

/** Point text placed where text on a path showed (its outlines' box stays put). */
export function detachText(d: Document, s: Stroke) {
  const t = s.text!
  t.on_path = null
  delete t.on_path
  const world = s.contours?.flat() ?? []
  const sh = shapedNow(t)
  if (!world.length || !sh) return
  const local = placedRings(layoutPoint(sh, t)).flat()
  if (!local.length) return
  const minX = (ps: Vec2[]) => Math.min(...ps.map(p => p.x)), minY = (ps: Vec2[]) => Math.min(...ps.map(p => p.y))
  t.matrix = [1, 0, 0, 1, minX(world) - minX(local), minY(world) - minY(local)]
}

/**
 * Keep every text's outlines in step with its settings and its path. Runs on
 * each document change, inside the same gesture (so one undo step holds the
 * edit and the new outlines). Text whose path is gone falls back to point
 * text. Glyphs not shaped yet are fetched and applied when they arrive.
 */
export function syncTexts(ed: Editor) {
  const d = ed.doc
  for (const s of d.strokes) {
    const t = s.text
    if (!t) continue
    if (t.on_path && !guidePath(d, t)) detachText(d, s)
    const sig = signature(d, s)
    if (seen.get(s.id) === sig) continue
    seen.set(s.id, sig)
    if (!place(d, s)) void relayout(ed, s)
  }
}

/** Start point and direction for typing text on a path (the editor box). */
export function onPathStart(d: Document, t: TextSpec): {p: Vec2; angle: number} | null {
  const path = guidePath(d, t)
  const sh = shapedNow(t) ?? {glyphs: [], lines: 1, ...metrics(t.font, t.size)}
  if (!path) return null
  const {p, t: dir} = pathTextStart(sh, t, path)
  return {p, angle: Math.atan2(dir.y, dir.x)}
}

/** The glyphs' own curves in local text space (Mac app: CoreText path data). */
function nativeCurves(t: Pick<TextSpec, 'text' | 'font' | 'size' | 'line_height'>): Promise<string[]> {
  const id = nextRequest++
  return new Promise((resolve, reject) => {
    waiting.set(id, r => (r.error ? reject(new Error(r.error)) : resolve(r.curves ?? [])))
    post!({type: 'glyphCurves', id, text: t.text, font: t.font, size: t.size, lineHeight: t.line_height})
  })
}

/** The text's outlines as editable rings (world space, anchors + handles). */
async function outlineRings(s: Stroke): Promise<Ring[]> {
  const t = s.text!
  if (!t.text.trim()) return []
  const m = t.matrix
  // Plain point text (no alignment, tracking, baseline shift or path): the
  // font's own curves. Laid-out text: its current outlines, fitted.
  const plain = (t.align ?? 'left') === 'left' && !t.tracking && !t.baseline_shift && !t.on_path && t.kerning !== false
  if (post && plain) {
    // The font's quadratic / cubic outlines, kept as they are.
    const subs = (await nativeCurves(t)).flatMap(d => parsePath(d)).map(sp => transformSubpath(sp, m))
    return subs.map(sp => nodesOf(sp, true, () => 0).map(({id: _, ...n}) => n))
  }
  // Browser: the traced outlines, fitted with curves (a bit looser than the
  // trace's pixel steps).
  const rings = s.contours ?? []
  const k = Math.hypot(m[0], m[1]) || 1
  return call<Ring[]>('fitRings', {rings, accuracy: Math.max(0.05, t.size * k * 0.006)})
}

/**
 * Type → Create Outlines: each text becomes a compound path of its glyph
 * outlines — real anchors and handles, counters (o, a, 8, 国) as holes, the
 * text's look kept. One undo step. Returns how many texts were outlined.
 */
export async function outlineToPaths(ed: Editor, texts: Stroke[]): Promise<number> {
  const shapes: {id: number; rings: Ring[]}[] = []
  for (const s of texts) {
    if (!s.text) continue
    try { shapes.push({id: s.id, rings: await outlineRings(s)}) } catch (e) {
      ed.error = e instanceof Error ? e.message : String(e)
      return 0
    }
  }
  const done = ed.edit(d => {
    const ids: number[] = []
    for (const {id, rings} of shapes) {
      const at = d.strokes.findIndex(s => s.id === id)
      if (at < 0) continue
      const s = d.strokes[at]
      const template = {...s, name: `${s.name}（アウトライン）`}
      const made = pathsFromRings(template, rings, 'nonzero', () => ed.nextId(d), s.id)
      d.strokes.splice(at, 1, ...made)
      ids.push(...made.map(m => m.id))
    }
    ed.selected = ids
    ed.anchors = []
    return shapes.length
  })
  return done ?? 0
}
