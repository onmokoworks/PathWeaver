// SVG paint → PathWeaver paint: a colour with its opacity, or a gradient
// (linearGradient / radialGradient with href inheritance, gradientUnits and
// gradientTransform) placed in document coordinates. `place.ts` hands in the
// mounted elements; the reading itself needs no layout, so it is unit-tested
// on parsed SVG.
import type {Gradient, GradientStop, Matrix, RGB} from './types'
import type {Affine} from './svgPath'

export interface Paint {color: RGB; opacity: number; gradient: Gradient | null}
export interface Box {x: number; y: number; width: number; height: number}

const mul = (a: Affine, b: Affine): Affine => [
  a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1],
  a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
  a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5],
]
const clamp01 = (v: number) => Math.min(1, Math.max(0, v))

/** A CSS colour: #rgb(a), #rrggbb(aa), rgb()/rgba() (commas or spaces, % alpha), a few keywords. */
export function parseColor(css: string): {color: RGB; alpha: number} | null {
  const s = css.trim().toLowerCase()
  if (!s || s === 'none' || s === 'transparent') return null
  const named: Record<string, RGB> = {black: [0, 0, 0], white: [255, 255, 255], red: [255, 0, 0], lime: [0, 255, 0], green: [0, 128, 0], blue: [0, 0, 255], gray: [128, 128, 128], grey: [128, 128, 128], yellow: [255, 255, 0]}
  if (named[s]) return {color: [...named[s]] as RGB, alpha: 1}
  const hex = /^#([0-9a-f]{3,8})$/.exec(s)
  if (hex) {
    let h = hex[1]
    if (h.length === 3 || h.length === 4) h = [...h].map(c => c + c).join('')
    if (h.length !== 6 && h.length !== 8) return null
    const v = (i: number) => parseInt(h.slice(i, i + 2), 16)
    return {color: [v(0), v(2), v(4)], alpha: h.length === 8 ? v(6) / 255 : 1}
  }
  const m = /rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:[,\s/]+([\d.]+)(%?))?/.exec(s)
  if (!m) return null
  const alpha = m[4] === undefined ? 1 : clamp01(parseFloat(m[4]) / (m[5] ? 100 : 1))
  return {color: [Math.round(+m[1]), Math.round(+m[2]), Math.round(+m[3])], alpha}
}

/** A length or percentage: `ref` is what 100% means (1 in bounding-box units). */
function length(v: string | null, ref: number, fallback: number): number {
  if (v == null || v.trim() === '') return fallback
  const n = parseFloat(v)
  if (!Number.isFinite(n)) return fallback
  return v.trim().endsWith('%') ? (n / 100) * ref : n
}
/** SVG transform list → affine (matrix, translate, scale, rotate, skewX/Y). */
export function parseTransform(t: string | null): Affine {
  let m: Affine = [1, 0, 0, 1, 0, 0]
  if (!t) return m
  for (const [, fn, args] of t.matchAll(/(\w+)\s*\(([^)]*)\)/g)) {
    const a = args.split(/[\s,]+/).filter(Boolean).map(Number)
    let n: Affine = [1, 0, 0, 1, 0, 0]
    const r = (deg: number) => deg * Math.PI / 180
    switch (fn) {
      case 'matrix': if (a.length === 6) n = a as Affine; break
      case 'translate': n = [1, 0, 0, 1, a[0] || 0, a[1] || 0]; break
      case 'scale': n = [a[0] ?? 1, 0, 0, a[1] ?? a[0] ?? 1, 0, 0]; break
      case 'rotate': {
        const c = Math.cos(r(a[0] || 0)), s = Math.sin(r(a[0] || 0))
        n = [c, s, -s, c, 0, 0]
        if (a.length >= 3) n = mul(mul([1, 0, 0, 1, a[1], a[2]], n), [1, 0, 0, 1, -a[1], -a[2]])
        break
      }
      case 'skewX': n = [1, 0, Math.tan(r(a[0] || 0)), 1, 0, 0]; break
      case 'skewY': n = [1, Math.tan(r(a[0] || 0)), 0, 1, 0, 0]; break
    }
    m = mul(m, n)
  }
  return m
}

export interface GradientContext {
  /** Element by id (for `url(#id)` and `href="#id"`). */
  byId: (id: string) => Element | null
  /** The shape's bounding box in its own user space (objectBoundingBox units). */
  bbox: () => Box
  /** Viewport size for percentages in userSpaceOnUse. */
  viewport: {w: number; h: number}
  /** The shape's user space → document. */
  ctm: Affine
  /** Stop colour and opacity as CSS resolves them (style attribute / class). */
  stopStyle?: (stop: Element) => {color: string; opacity: string}
}

const hrefOf = (el: Element) => (el.getAttribute('href') ?? el.getAttribute('xlink:href') ?? el.getAttributeNS('http://www.w3.org/1999/xlink', 'href') ?? '').replace(/^#/, '')

/** A gradient element (following `href`) as a PathWeaver gradient, or a solid paint when it has one stop. */
export function readGradient(el: Element, ctx: GradientContext): Paint | null {
  const chain: Element[] = []
  for (let at: Element | null = el; at && chain.length < 16 && !chain.includes(at); at = hrefOf(at) ? ctx.byId(hrefOf(at)) : null) {
    if (/^(linear|radial)gradient$/i.test(at.tagName)) chain.push(at)
  }
  if (!chain.length) return null
  const attr = (k: string) => chain.find(e => e.hasAttribute(k))?.getAttribute(k) ?? null
  const stopsFrom = chain.find(e => e.querySelector('stop'))
  let last = 0
  const stops: GradientStop[] = [...(stopsFrom?.querySelectorAll(':scope > stop') ?? [])].map(stop => {
    const style = ctx.stopStyle?.(stop)
    const inline = (k: string) => new RegExp(`${k}\\s*:\\s*([^;]+)`).exec(stop.getAttribute('style') ?? '')?.[1]
    const colorCss = style?.color || inline('stop-color') || stop.getAttribute('stop-color') || 'black'
    const opacityCss = style?.opacity || inline('stop-opacity') || stop.getAttribute('stop-opacity') || '1'
    const c = parseColor(colorCss) ?? {color: [0, 0, 0] as RGB, alpha: 0}
    const off = stop.getAttribute('offset') ?? '0'
    // offsets never go back (SVG rule)
    last = Math.max(last, clamp01(off.trim().endsWith('%') ? parseFloat(off) / 100 : parseFloat(off) || 0))
    const o = parseFloat(opacityCss)
    return {offset: last, color: c.color, opacity: clamp01(c.alpha * (Number.isFinite(o) ? o : 1))}
  })
  if (!stops.length) return null
  if (stops.length === 1) return {color: stops[0].color, opacity: stops[0].opacity, gradient: null}
  const radial = /^radialgradient$/i.test(chain[0].tagName)
  const bboxUnits = attr('gradientUnits') !== 'userSpaceOnUse'
  const W = bboxUnits ? 1 : ctx.viewport.w, H = bboxUnits ? 1 : ctx.viewport.h
  const D = bboxUnits ? 1 : Math.hypot(ctx.viewport.w, ctx.viewport.h) / Math.SQRT2
  let local: Affine
  if (radial) {
    const cx = length(attr('cx'), W, W / 2), cy = length(attr('cy'), H, H / 2), r = length(attr('r'), D, D / 2)
    if (!(r > 0)) return {color: stops.at(-1)!.color, opacity: stops.at(-1)!.opacity, gradient: null}
    local = [r, 0, 0, r, cx, cy]
  } else {
    const x1 = length(attr('x1'), W, 0), y1 = length(attr('y1'), H, 0), x2 = length(attr('x2'), W, W), y2 = length(attr('y2'), H, 0)
    const dx = x2 - x1, dy = y2 - y1
    if (Math.hypot(dx, dy) < 1e-12) return {color: stops.at(-1)!.color, opacity: stops.at(-1)!.opacity, gradient: null}
    local = [dx, dy, -dy, dx, x1, y1]
  }
  let m = mul(parseTransform(attr('gradientTransform')), local)
  if (bboxUnits) {
    const b = ctx.bbox()
    if (!(b.width > 0) && !(b.height > 0)) return {color: stops[0].color, opacity: stops[0].opacity, gradient: null}
    m = mul([b.width || 1, 0, 0, b.height || 1, b.x, b.y], m)
  }
  m = mul(ctx.ctm, m)
  const det = m[0] * m[3] - m[1] * m[2]
  if (!Number.isFinite(det) || Math.abs(det) < 1e-12) return {color: stops[0].color, opacity: stops[0].opacity, gradient: null}
  return {color: [...stops[0].color] as RGB, opacity: 1, gradient: {kind: radial ? 'Radial' : 'Linear', stops, matrix: m as Matrix}}
}

/**
 * One SVG paint (`fill` or `stroke` as computed) with its `*-opacity` and the
 * element's own and inherited group opacity. Null: no paint.
 */
export function readPaint(css: string, paintOpacity: string, groupOpacity: number, ctx: GradientContext): Paint | null {
  const po = parseFloat(paintOpacity)
  const k = clamp01((Number.isFinite(po) ? po : 1) * groupOpacity)
  if (k <= 0) return null
  const url = /url\(\s*["']?[^"')#]*#([^"')]+)["']?\s*\)/.exec(css)
  let p: Paint | null
  if (url) {
    const el = ctx.byId(url[1])
    p = el ? readGradient(el, ctx) : null
    // a missing server falls back (SVG: `url(#x) red`); a pattern: grey
    if (!p) {
      const rest = parseColor(css.replace(url[0], ''))
      p = rest ? {color: rest.color, opacity: rest.alpha, gradient: null} : el ? {color: [128, 128, 128], opacity: 1, gradient: null} : null
    }
  } else {
    const c = parseColor(css)
    p = c ? {color: c.color, opacity: c.alpha, gradient: null} : null
  }
  if (!p || (!p.gradient && p.opacity <= 0)) return null
  // A gradient keeps every opacity in its stops (what the inspector shows).
  if (p.gradient) return {...p, opacity: 1, gradient: {...p.gradient, stops: p.gradient.stops.map(s => ({...s, opacity: clamp01(s.opacity * k)}))}}
  return {...p, opacity: clamp01(p.opacity * k)}
}
