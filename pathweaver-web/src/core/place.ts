// Bringing outside material in (Illustrator's Place): bitmaps become placed
// images, SVG files become editable paths (and live text), all in one folder
// named after the file, centred where they were dropped (or in the view).
import {call} from './wasm'
import type {Editor} from './editor'
import type {Matrix, RGB, Stroke, Vec2} from './types'
import {flatten, parsePath, transformSubpath, type Affine, type Subpath} from './svgPath'
import {apply, baselineTop, DEFAULT_FONT, outlineText} from './textOutline'
import {readPaint, type GradientContext, type Paint} from './svgPaint'
import {setGradient, setOpacity, type PaintKey} from './paint'

const MAX_SIDE = 4096

/** Short content hash: the same picture placed twice is stored once. */
function hash(s: string) {
  let h1 = 0x811c9dc5, h2 = 0x01000193
  for (let i = 0; i < s.length; i += Math.max(1, Math.floor(s.length / 200000))) {
    const c = s.charCodeAt(i)
    h1 = Math.imul(h1 ^ c, 16777619)
    h2 = Math.imul(h2 + c, 2654435761)
  }
  return `img${(h1 >>> 0).toString(36)}${(h2 >>> 0).toString(36)}${s.length.toString(36)}`
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => resolve(img)
    img.onerror = () => reject(new Error('画像を読み込めませんでした（PNG・JPEG・GIF・WebP が使えます）'))
    img.src = src
  })
}
const fileUrl = (f: Blob): Promise<string> => new Promise((resolve, reject) => {
  const r = new FileReader()
  r.onload = () => resolve(String(r.result))
  r.onerror = () => reject(r.error)
  r.readAsDataURL(f)
})

/** A bitmap as a data URL no larger than MAX_SIDE, with its pixel size. */
async function normalizedImage(src: string): Promise<{url: string; w: number; h: number}> {
  const img = await loadImage(src)
  let w = img.naturalWidth, h = img.naturalHeight
  if (!w || !h) throw new Error('画像の大きさが分かりません')
  const big = Math.max(w, h) > MAX_SIDE
  if (!big && /^data:image\/(png|jpeg|gif|webp)/.test(src)) return {url: src, w, h}
  const k = big ? MAX_SIDE / Math.max(w, h) : 1
  w = Math.round(w * k); h = Math.round(h * k)
  const c = document.createElement('canvas')
  c.width = w; c.height = h
  c.getContext('2d')!.drawImage(img, 0, 0, w, h)
  const jpeg = /^data:image\/jpe?g/.test(src)
  return {url: jpeg ? c.toDataURL('image/jpeg', 0.92) : c.toDataURL('image/png'), w, h}
}

export interface PlaceTarget {
  ed: Editor
  /** Where to centre the material (world); null = the middle of the view. */
  at: Vec2 | null
  /** World units visible, to fit large material into view. */
  viewSize: {w: number; h: number}
}

const blankStroke = (id: number, name: string): Stroke => ({
  id, name, visible: true, closed: false, nodes: [], widths: [],
  start_width: {left: 0.5, right: 0.5, linked: true, angle: 0}, end_width: {left: 0.5, right: 0.5, linked: true, angle: 0},
  start_angle: 90, end_angle: 90, fill: null, edge: null, edge_width: 1, frame_angle: 0,
  corners: [], join: 'Miter', miter_limit: 10, contours: null, appearance: null,
})
const rectOf = (m: Matrix, w: number, h: number): Vec2[] =>
  [[0, 0], [w, 0], [w, h], [0, h]].map(([x, y]) => ({x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5]}))

/** Place one bitmap (data URL). Returns the new stroke id. */
export async function placeImage(t: PlaceTarget, src: string, name: string): Promise<number | undefined> {
  const {url, w, h} = await normalizedImage(src)
  // Pixel = 1 unit, shrunk to fit 70% of the view when larger.
  const k = Math.min(1, (t.viewSize.w * 0.7) / w, (t.viewSize.h * 0.7) / h)
  const c = t.at ?? {x: 0, y: 0}
  const matrix: Matrix = [k, 0, 0, k, c.x - (w * k) / 2, c.y - (h * k) / 2]
  return t.ed.edit(d => {
    const key = hash(url)
    ;(d.assets ??= {})[key] ??= url
    const s = blankStroke(t.ed.nextId(d), name)
    s.closed = true
    s.image = {asset: key, width: w, height: h, matrix, opacity: 1}
    s.contours = [rectOf(matrix, w, h)]
    d.strokes.push(s)
    t.ed.selected = [s.id]
    t.ed.anchors = []
    return s.id
  })
}

// ---------------- SVG ----------------

const ID: Affine = [1, 0, 0, 1, 0, 0]
const mul = (a: Affine, b: Affine): Affine => [
  a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1],
  a[0] * b[2] + a[2] * b[3], a[1] * b[2] + a[3] * b[3],
  a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5],
]
const fromDOM = (m: DOMMatrix | SVGMatrix | null): Affine => (m ? [m.a, m.b, m.c, m.d, m.e, m.f] : ID)

/** Put a read SVG paint on a stroke (colour, opacity, gradient). */
function givePaint(s: Stroke, key: PaintKey, p: Paint | null) {
  s[key] = p ? [...p.color] as RGB : null
  setOpacity(s, key, p?.opacity ?? 1)
  setGradient(s, key, p?.gradient ?? null)
}
/** `opacity` of the element and every group around it (a group's opacity is not inherited in CSS). */
function groupOpacity(el: Element, root: Element): number {
  let k = 1
  for (let at: Element | null = el; at && at !== root.parentElement; at = at.parentElement) {
    const o = parseFloat(getComputedStyle(at).opacity)
    if (Number.isFinite(o)) k *= o
  }
  return k
}

function shapeData(el: Element): string | null {
  const n = (k: string) => parseFloat(el.getAttribute(k) ?? '0') || 0
  switch (el.tagName.toLowerCase()) {
    case 'path': return el.getAttribute('d')
    case 'line': return `M${n('x1')} ${n('y1')}L${n('x2')} ${n('y2')}`
    case 'polyline': case 'polygon': {
      const pts = (el.getAttribute('points') ?? '').trim()
      return pts ? `M${pts}${el.tagName.toLowerCase() === 'polygon' ? 'Z' : ''}` : null
    }
    case 'rect': {
      const x = n('x'), y = n('y'), w = n('width'), h = n('height')
      if (w <= 0 || h <= 0) return null
      let rx = el.hasAttribute('rx') ? n('rx') : n('ry'), ry = el.hasAttribute('ry') ? n('ry') : rx
      rx = Math.min(rx, w / 2); ry = Math.min(ry, h / 2)
      if (!rx || !ry) return `M${x} ${y}H${x + w}V${y + h}H${x}Z`
      return `M${x + rx} ${y}H${x + w - rx}A${rx} ${ry} 0 0 1 ${x + w} ${y + ry}V${y + h - ry}A${rx} ${ry} 0 0 1 ${x + w - rx} ${y + h}H${x + rx}A${rx} ${ry} 0 0 1 ${x} ${y + h - ry}V${y + ry}A${rx} ${ry} 0 0 1 ${x + rx} ${y}Z`
    }
    case 'circle': case 'ellipse': {
      const cx = n('cx'), cy = n('cy')
      const rx = el.tagName.toLowerCase() === 'circle' ? n('r') : n('rx'), ry = el.tagName.toLowerCase() === 'circle' ? n('r') : n('ry')
      if (rx <= 0 || ry <= 0) return null
      return `M${cx + rx} ${cy}A${rx} ${ry} 0 0 1 ${cx} ${cy + ry}A${rx} ${ry} 0 0 1 ${cx - rx} ${cy}A${rx} ${ry} 0 0 1 ${cx} ${cy - ry}A${rx} ${ry} 0 0 1 ${cx + rx} ${cy}Z`
    }
  }
  return null
}

const near = (a: Vec2, b: Vec2) => Math.hypot(a.x - b.x, a.y - b.y) < 1e-6
/** A subpath as an editable PathWeaver path (anchors + handles). */
function nodesOf(ed: Editor, d: {next_id: number}, sp: Subpath, closed: boolean) {
  const segs = [...sp.segs]
  let wrap: Vec2 | null = null
  // Closed and ending on its start: that last segment's handle belongs to the first anchor.
  if (closed && segs.length > 1 && near(segs.at(-1)!.p, sp.start)) wrap = segs.pop()!.c2
  const pts = [sp.start, ...segs.map(s => s.p)]
  const nodes = pts.map(p => ({id: ed.nextId(d as never), p: {...p}, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false}))
  segs.forEach((s, i) => {
    const a = nodes[i], b = nodes[i + 1]
    a.outgoing = {x: s.c1.x - a.p.x, y: s.c1.y - a.p.y}
    b.incoming = {x: s.c2.x - b.p.x, y: s.c2.y - b.p.y}
  })
  if (wrap) {
    // The closing curve runs from the last anchor back to the first.
    const last = sp.segs.at(-1)!
    nodes.at(-1)!.outgoing = {x: last.c1.x - nodes.at(-1)!.p.x, y: last.c1.y - nodes.at(-1)!.p.y}
    nodes[0].incoming = {x: wrap.x - nodes[0].p.x, y: wrap.y - nodes[0].p.y}
  }
  // Straight segments carry no handles; collinear handles are smooth anchors.
  const tiny = (v: Vec2) => Math.hypot(v.x, v.y) < 1e-6
  segs.forEach((s, i) => {
    const a = nodes[i], b = nodes[i + 1]
    const ab = {x: b.p.x - a.p.x, y: b.p.y - a.p.y}
    const off = (v: Vec2) => Math.abs(v.x * ab.y - v.y * ab.x) / (Math.hypot(ab.x, ab.y) || 1)
    if (off(a.outgoing) < 1e-6 && off(b.incoming) < 1e-6 && Math.abs(Math.hypot(a.outgoing.x, a.outgoing.y) * 3 - Math.hypot(ab.x, ab.y)) < 1e-3) {
      a.outgoing = {x: 0, y: 0}; b.incoming = {x: 0, y: 0}
    }
  })
  for (const n of nodes) {
    if (tiny(n.incoming) || tiny(n.outgoing)) continue
    const cross = n.incoming.x * n.outgoing.y - n.incoming.y * n.outgoing.x
    const dot = n.incoming.x * n.outgoing.x + n.incoming.y * n.outgoing.y
    n.smooth = Math.abs(cross) < 1e-3 * Math.hypot(n.incoming.x, n.incoming.y) * Math.hypot(n.outgoing.x, n.outgoing.y) && dot < 0
  }
  return nodes
}

interface Imported {strokes: Stroke[]; skipped: number; texts: Stroke[]}

/** Read an SVG document into strokes (not yet in the document). */
async function readSvg(ed: Editor, d: {next_id: number; assets?: Record<string, string>}, text: string): Promise<Imported> {
  const parsed = new DOMParser().parseFromString(text, 'image/svg+xml')
  const src = parsed.documentElement
  if (parsed.querySelector('parsererror') || src.tagName.toLowerCase() !== 'svg') throw new Error('SVG として読めませんでした')
  // Nothing from the file runs: scripts, handlers and embedded HTML are dropped.
  src.querySelectorAll('script, foreignObject').forEach(e => e.remove())
  for (const e of [src, ...src.querySelectorAll('*')]) {
    for (const a of [...e.attributes]) if (/^on/i.test(a.name)) e.removeAttribute(a.name)
  }
  // Mounted (hidden) so the browser resolves CSS, inherited paint and transforms.
  const host = document.createElement('div')
  host.style.cssText = 'position:fixed;left:-100000px;top:0;opacity:0;pointer-events:none' // not visibility: it is inherited
  const svg = document.importNode(src, true) as unknown as SVGSVGElement
  host.appendChild(svg)
  document.body.appendChild(host)
  try {
    const rootInv = fromDOM(svg.getScreenCTM()?.inverse() ?? null)
    const ctm = (el: Element): Affine => mul(rootInv, fromDOM((el as SVGGraphicsElement).getScreenCTM?.() ?? null))
    const out: Imported = {strokes: [], skipped: 0, texts: []}
    const vb = svg.viewBox?.baseVal
    const viewport = vb && vb.width > 0 && vb.height > 0 ? {w: vb.width, h: vb.height} : {w: svg.width?.baseVal?.value || 100, h: svg.height?.baseVal?.value || 100}
    const context = (el: Element, m: Affine): GradientContext => ({
      byId: id => svg.querySelector(`[id="${CSS.escape(id)}"]`),
      bbox: () => { try { const b = (el as SVGGraphicsElement).getBBox(); return {x: b.x, y: b.y, width: b.width, height: b.height} } catch { return {x: 0, y: 0, width: 0, height: 0} } },
      viewport, ctm: m,
      stopStyle: stop => { const cs = getComputedStyle(stop); return {color: cs.stopColor, opacity: cs.stopOpacity} },
    })
    const hidden = (el: Element) => el.closest('defs, clipPath, mask, symbol, marker, pattern') !== null
    for (const el of svg.querySelectorAll('*')) {
      if (hidden(el)) continue
      const tag = el.tagName.toLowerCase()
      const cs = getComputedStyle(el)
      if (cs.display === 'none' || cs.visibility === 'hidden') continue
      if (tag === 'use') { out.skipped++; continue }
      const label = el.getAttribute('id') || el.getAttribute('inkscape:label') || ''
      if (tag === 'image') {
        const href = el.getAttribute('href') ?? el.getAttribute('xlink:href') ?? ''
        if (!href.startsWith('data:image/')) { out.skipped++; continue }
        const {url, w, h} = await normalizedImage(href)
        const n = (k: string) => parseFloat(el.getAttribute(k) ?? '0') || 0
        const bw = n('width') || w, bh = n('height') || h
        const m = mul(ctm(el), [bw / w, 0, 0, bh / h, n('x'), n('y')]) as Matrix
        const key = hash(url)
        ;(d.assets ??= {})[key] ??= url
        const s = blankStroke(ed.nextId(d as never), label || '画像')
        s.closed = true
        s.image = {asset: key, width: w, height: h, matrix: m, opacity: Math.max(0, Math.min(1, parseFloat(cs.opacity) || 1))}
        s.contours = [rectOf(m, w, h)]
        out.strokes.push(s)
        continue
      }
      if (tag === 'text') {
        const content = (el.textContent ?? '').replace(/\s+/g, ' ').trim()
        if (!content) continue
        const size = parseFloat(cs.fontSize) || 16
        const family = cs.fontFamily.split(',')[0].trim().replace(/^["']|["']$/g, '') || DEFAULT_FONT
        const x = parseFloat(el.getAttribute('x') ?? '0') || 0, y = parseFloat(el.getAttribute('y') ?? '0') || 0
        const anchor = cs.textAnchor
        const m = mul(ctm(el), [1, 0, 0, 1, x, y - baselineTop(family, size, 1.2)]) as Matrix
        const s = blankStroke(ed.nextId(d as never), label || content.slice(0, 20))
        s.closed = true
        givePaint(s, 'fill', readPaint(cs.fill, cs.fillOpacity, groupOpacity(el, svg), context(el, ctm(el))) ?? {color: [0, 0, 0], opacity: 1, gradient: null})
        s.contours = []
        s.text = {text: content, font: family, size, line_height: 1.2, matrix: m}
        if (anchor === 'middle' || anchor === 'end') {
          // Shift by the measured width (start-anchored layout here).
          const w = (el as SVGTextContentElement).getComputedTextLength?.() ?? 0
          s.text.matrix = mul(s.text.matrix as Affine, [1, 0, 0, 1, anchor === 'middle' ? -w / 2 : -w, 0]) as Matrix
        }
        out.strokes.push(s)
        out.texts.push(s)
        continue
      }
      const data = shapeData(el)
      if (!data) continue
      let subs: Subpath[]
      try { subs = parsePath(data) } catch { out.skipped++; continue }
      const m = ctm(el)
      subs = subs.map(sp => transformSubpath(sp, m))
      if (!subs.length) continue
      const group = groupOpacity(el, svg)
      const fill = readPaint(cs.fill, cs.fillOpacity, group, context(el, m))
      const edge = readPaint(cs.stroke, cs.strokeOpacity, group, context(el, m))
      const scale = Math.sqrt(Math.abs(m[0] * m[3] - m[1] * m[2])) || 1
      const width = Math.max(0.01, Math.min(2000, (parseFloat(cs.strokeWidth) || 1) * scale))
      const name = label || {path: 'パス', rect: '長方形', circle: '円', ellipse: '楕円', line: '直線', polyline: '折れ線', polygon: '多角形'}[tag] || tag
      if (!fill && !edge) continue
      const join = cs.strokeLinejoin === 'round' ? 'Round' : cs.strokeLinejoin === 'bevel' ? 'Bevel' : 'Miter'
      const cap = cs.strokeLinecap === 'round' ? 'Round' : cs.strokeLinecap === 'square' ? 'Square' : undefined
      if (fill && subs.length > 1) {
        // Several subpaths (holes, separate islands): one filled shape.
        const rings = subs.map(sp => flatten(sp))
        const s = blankStroke(ed.nextId(d as never), name)
        s.closed = true
        givePaint(s, 'fill', fill)
        s.contours = call<Vec2[][]>('normalizeRings', {rings, evenodd: cs.fillRule === 'evenodd'})
        if (!s.contours.length) continue
        if (edge) { givePaint(s, 'edge', edge); s.edge_width = width }
        s.join = join
        out.strokes.push(s)
        continue
      }
      for (const sp of subs) {
        const closed = sp.closed || !!fill
        const nodes = nodesOf(ed, d, sp, closed)
        if (nodes.length < 2) continue
        const s = blankStroke(ed.nextId(d as never), subs.length > 1 ? `${name} ${out.strokes.length + 1}` : name)
        s.closed = closed
        s.nodes = nodes
        givePaint(s, 'fill', fill)
        givePaint(s, 'edge', edge)
        const half = Math.min(1000, width / 2)
        s.start_width = {left: half, right: half, linked: true, angle: 0}
        s.end_width = {left: half, right: half, linked: true, angle: 0}
        s.join = join
        if (cap && !closed) { s.start_cap = cap; s.end_cap = cap }
        out.strokes.push(s)
      }
    }
    return out
  } finally {
    host.remove()
  }
}

/** Import an SVG file as paths in a new folder. Returns the new stroke ids. */
export async function importSvg(t: PlaceTarget, text: string, name: string): Promise<number[]> {
  const ed = t.ed
  // Read against a scratch copy of the id counter, then commit in one edit.
  const scratch = {next_id: ed.doc.next_id, assets: {...(ed.doc.assets ?? {})}}
  const got = await readSvg(ed, scratch, text)
  if (!got.strokes.length) throw new Error(got.skipped ? 'この SVG には読み込める形がありませんでした（<use> や外部画像は未対応）' : 'この SVG には形がありません')
  // Centre where asked, keeping the drawing's own size.
  const pts = got.strokes.flatMap(s => s.contours?.flat() ?? s.nodes.map(n => n.p))
  for (const s of got.strokes) if (s.text) pts.push({x: s.text.matrix[4], y: s.text.matrix[5]})
  const xs = pts.map(p => p.x), ys = pts.map(p => p.y)
  const c = t.at ?? {x: 0, y: 0}
  const dx = c.x - (Math.min(...xs) + Math.max(...xs)) / 2, dy = c.y - (Math.min(...ys) + Math.max(...ys)) / 2
  // Text gets its outlines from the font before going in (one undo step).
  for (const s of got.texts) {
    try { s.contours = (await outlineText(s.text!)).map(r => r.map(p => apply(s.text!.matrix, p))) } catch { /* stays empty: retyped later */ }
  }
  const ids = ed.edit(doc => {
    doc.next_id = Math.max(doc.next_id, scratch.next_id)
    doc.assets = scratch.assets
    if (!Object.keys(doc.assets).length) delete doc.assets
    doc.folders ??= []
    const folder = scratch.next_id++
    doc.next_id = scratch.next_id
    doc.folders.push({id: folder, name: name.replace(/\.svg$/i, ''), visible: true, open: true})
    for (const s of got.strokes) {
      for (const n of s.nodes) n.p = {x: n.p.x + dx, y: n.p.y + dy}
      if (s.contours) s.contours = s.contours.map(r => r.map(p => ({x: p.x + dx, y: p.y + dy})))
      for (const pl of [s.text, s.image, s.fill_gradient, s.edge_gradient]) if (pl) { pl.matrix[4] += dx; pl.matrix[5] += dy }
      s.folder = folder
      doc.strokes.push(s)
    }
    call('validate', {doc})
    ed.selected = got.strokes.map(s => s.id)
    ed.anchors = []
    return got.strokes.map(s => s.id)
  })
  if (!ids) return []
  ed.status = `「${name}」を読み込みました（${ids.length}個${got.skipped ? `・読めなかった要素 ${got.skipped}個` : ''}）`
  return ids
}

const isSvg = (f: {name: string; type: string}) => f.type === 'image/svg+xml' || /\.svg$/i.test(f.name)
/** Files dropped, pasted or chosen in the file dialog. */
export async function placeFiles(t: PlaceTarget, files: File[]) {
  for (const f of files) {
    try {
      if (isSvg(f)) await importSvg(t, await f.text(), f.name)
      else if (f.type.startsWith('image/') || /\.(png|jpe?g|gif|webp|bmp)$/i.test(f.name)) {
        await placeImage(t, await fileUrl(f), f.name)
        t.ed.status = `「${f.name}」を配置しました`
      } else t.ed.error = `「${f.name}」は読み込めない種類のファイルです（SVG・PNG・JPEG・GIF・WebP）`
    } catch (e) {
      t.ed.error = e instanceof Error ? e.message : String(e)
    }
  }
}
/** From the Mac app's file dialog: SVG text or an image data URL. */
export async function placeData(t: PlaceTarget, name: string, kind: 'svg' | 'image', data: string) {
  try {
    if (kind === 'svg') await importSvg(t, data, name)
    else { await placeImage(t, data, name); t.ed.status = `「${name}」を配置しました` }
  } catch (e) {
    t.ed.error = e instanceof Error ? e.message : String(e)
  }
}
