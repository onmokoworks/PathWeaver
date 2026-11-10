// Glyph outlines for live text. The text keeps its characters and font; its
// shape is the outlines laid out here, stored as the stroke's contours (so
// fill, outline and the appearance graph apply to text like to any shape).
//
// Layout matches CSS, so the on-canvas text editor's caret sits on the
// outlines: local origin = top-left of the first line box, each line box is
// size × line_height tall and its baseline is centred like CSS half-leading.
//
// Mac app: CoreText lays out the exact outlines (with font fallback for
// Japanese). Browser: the text is drawn on a canvas and traced back (close
// enough to develop and test with).
import {call} from './wasm'
import type {Editor} from './editor'
import type {Matrix, Stroke, TextSpec, Vec2} from './types'

export const DEFAULT_FONT = 'Hiragino Sans'
type Post = (m: {type: string; [k: string]: unknown}) => void

let post: Post | null = null
let nextRequest = 1
const waiting = new Map<number, (r: {contours?: number[][]; error?: string}) => void>()
let fontList: string[] | null = null
const fontWaiters: ((list: string[]) => void)[] = []

/** Called by the host bridge in the Mac app. */
export function initTextHost(p: Post | null) {
  post = p
  if (!window.pathweaver) return
  Object.assign(window.pathweaver, {
    glyphs: ((id: number, contours: number[][] | null, error?: string) => {
      waiting.get(id)?.(contours ? {contours} : {error: error ?? '文字の形を作れませんでした'})
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

const cache = new Map<string, Vec2[][]>()
/** Outlines of the text in its local space (see the layout rule above). */
export async function outlineText(t: Pick<TextSpec, 'text' | 'font' | 'size' | 'line_height'>): Promise<Vec2[][]> {
  const key = JSON.stringify([t.text, t.font, t.size, t.line_height, !!post])
  const hit = cache.get(key)
  if (hit) return hit
  let rings: Vec2[][]
  if (!t.text.trim()) rings = []
  else if (post) rings = await nativeOutline(t)
  else rings = traceOutline(t)
  // Fonts are drawn with the nonzero rule; normalize like any filled shape.
  const out = rings.length ? call<Vec2[][]>('normalizeRings', {rings, evenodd: false}) : []
  if (cache.size > 200) cache.clear()
  cache.set(key, out)
  return out
}

function nativeOutline(t: Pick<TextSpec, 'text' | 'font' | 'size' | 'line_height'>): Promise<Vec2[][]> {
  const id = nextRequest++
  return new Promise((resolve, reject) => {
    waiting.set(id, r => {
      if (r.error) return reject(new Error(r.error))
      // [x0, y0, x1, y1, …] per ring
      resolve((r.contours ?? []).map(flat => {
        const ring: Vec2[] = []
        for (let i = 0; i + 1 < flat.length; i += 2) ring.push({x: flat[i], y: flat[i + 1]})
        return ring
      }))
    })
    post!({type: 'glyphs', id, text: t.text, font: t.font, size: t.size, lineHeight: t.line_height})
  })
}

const cssFont = (family: string, px: number) => `${px}px ${/^(sans-serif|serif|monospace)$/.test(family) ? family : JSON.stringify(family)}`

function traceOutline(t: Pick<TextSpec, 'text' | 'font' | 'size' | 'line_height'>): Vec2[][] {
  const k = Math.min(8, 128 / t.size)          // canvas px per unit: ~128 px em
  const px = t.size * k
  const lines = t.text.split('\n')
  const c = document.createElement('canvas')
  const g = c.getContext('2d')!
  g.font = cssFont(t.font, px)
  const m = g.measureText('Hg')
  const asc = m.fontBoundingBoxAscent ?? px * 0.88
  const desc = m.fontBoundingBoxDescent ?? px * 0.12
  const lh = px * t.line_height
  const pad = Math.ceil(px * 0.5)
  const width = Math.max(...lines.map(l => g.measureText(l).width), 1)
  c.width = Math.min(8000, Math.ceil(width + 2 * pad))
  c.height = Math.min(8000, Math.ceil(lines.length * lh + 2 * pad))
  g.font = cssFont(t.font, px)
  g.fillStyle = '#000'
  lines.forEach((l, i) => g.fillText(l, pad, pad + i * lh + (lh - (asc + desc)) / 2 + asc))
  const rings = call<Vec2[][]>('traceText', {png: c.toDataURL('image/png'), scale: 1 / k})
  const o = pad / k
  return rings.map(r => r.map(p => ({x: p.x - o, y: p.y - o})))
}

/** Distance from the top of a line box to its baseline (CSS half-leading),
 *  from the font's own ascent / descent. */
export function baselineTop(family: string, size: number, lineHeight: number): number {
  const g = document.createElement('canvas').getContext('2d')!
  g.font = cssFont(family, 100)
  const m = g.measureText('Hg')
  const asc = (m.fontBoundingBoxAscent ?? 88) / 100 * size
  const desc = (m.fontBoundingBoxDescent ?? 12) / 100 * size
  return (size * lineHeight - (asc + desc)) / 2 + asc
}

export const apply = (m: Matrix, p: Vec2): Vec2 => ({x: m[0] * p.x + m[2] * p.y + m[4], y: m[1] * p.x + m[3] * p.y + m[5]})

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
  let local: Vec2[][]
  try {
    local = await outlineText(t)
  } catch (e) {
    ed.error = e instanceof Error ? e.message : String(e)
    return
  }
  if (latest.get(s.id) !== ticket) return
  const now = ed.findStroke(s.id)
  if (!now?.text) return
  now.contours = local.map(r => r.map(p => apply(now.text!.matrix, p)))
  ed.changed()
}
