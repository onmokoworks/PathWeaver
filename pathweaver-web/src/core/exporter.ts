// Export: SVG, PNG or JPEG of artboards, the selection, or the whole drawing.
// The core writes the SVG (framed by the artboard); pictures are that SVG
// drawn into a canvas at the chosen scale, so both always look the same.
import {call} from './wasm'
import {exportAssets} from './knockout'
import type {Document, RGB} from './types'

export type ExportFormat = 'svg' | 'png' | 'jpeg'
export type ExportTarget = 'artboards' | 'selection' | 'all'
export interface ExportOptions {
  target: ExportTarget
  /** Which artboards (ids) when target = artboards. */
  artboards: number[]
  /** Paths in the selection (target = selection). */
  selection: number[]
  format: ExportFormat
  scale: number
  /** null: transparent (JPEG has no transparency: white). */
  background: RGB | null
  /** File name without extension. */
  base: string
}
/** One output file: text (SVG) or base64 bytes (pictures). */
export interface ExportFile {name: string; mime: string; text?: string; base64?: string}

/** The largest picture side: beyond this WebKit's canvas gives up. */
export const MAX_SIDE = 16384
const EXT: Record<ExportFormat, string> = {svg: 'svg', png: 'png', jpeg: 'jpg'}
const MIME: Record<ExportFormat, string> = {svg: 'image/svg+xml', png: 'image/png', jpeg: 'image/jpeg'}
const safe = (s: string) => s.replace(/[\\/:*?"<>|]/g, '_').trim() || 'アートボード'

type Frame = [number, number, number, number]
/** The document as exported: only the selected paths for target = selection. */
const scoped = (d: Document, o: ExportOptions): Document =>
  o.target === 'selection' ? {...d, strokes: d.strokes.filter(s => o.selection.includes(s.id))} : d
/** Selection / whole drawing: the core measures what is painted, effects
 *  included (a glow is not cut), plus a margin for the whole drawing. */
const MARGIN: Record<ExportTarget, number> = {artboards: 0, selection: 0, all: 16}
function paintedFrame(d: Document, o: ExportOptions): Frame | null {
  if (!scoped(d, o).strokes.length) return null
  const svg = call<string>('svg', {doc: {...scoped(d, o), assets: {}}, margin: MARGIN[o.target]})
  const vb = svg.match(/viewBox="([^"]+)"/)?.[1].split(/\s+/).map(Number)
  return vb && vb.length === 4 ? vb as Frame : null
}

/** The frames to export: name and x, y, w, h. */
export function frames(d: Document, o: ExportOptions): {name: string; frame: Frame}[] {
  if (o.target === 'artboards') {
    const list = (d.artboards ?? []).filter(a => o.artboards.includes(a.id))
    const single = list.length === 1
    return list.map(a => ({name: single && d.artboards!.length === 1 ? o.base : `${o.base}-${safe(a.name)}`, frame: [a.x, a.y, a.w, a.h]}))
  }
  const f = paintedFrame(d, o)
  return f ? [{name: o.base, frame: f}] : []
}

export async function exportFiles(d: Document, o: ExportOptions): Promise<ExportFile[]> {
  const doc = {...scoped(d, o), assets: await exportAssets(d)}
  const bg = o.format === 'jpeg' ? (o.background ?? [255, 255, 255]) : o.background
  const out: ExportFile[] = []
  for (const f of frames(d, o)) {
    // SVG keeps a background only when one was asked for
    const svg = call<string>('svg', {doc, frame: f.frame, background: o.format === 'svg' ? o.background : null})
    const name = `${f.name}.${EXT[o.format]}`
    if (o.format === 'svg') { out.push({name, mime: MIME.svg, text: svg}); continue }
    out.push({name, mime: MIME[o.format], base64: await rasterize(svg, f.frame[2], f.frame[3], o.scale, o.format, bg)})
  }
  return out
}

/** Pixel size of a frame at a scale (clamped to what a canvas can hold). */
export function pixelSize(w: number, h: number, scale: number) {
  let pw = Math.max(1, Math.round(w * scale)), ph = Math.max(1, Math.round(h * scale))
  const k = Math.min(1, MAX_SIDE / Math.max(pw, ph))
  if (k < 1) { pw = Math.max(1, Math.floor(pw * k)); ph = Math.max(1, Math.floor(ph * k)) }
  return {w: pw, h: ph, clamped: k < 1}
}

async function rasterize(svg: string, w: number, h: number, scale: number, format: 'png' | 'jpeg', bg: RGB | null): Promise<string> {
  const size = pixelSize(w, h, scale)
  const url = URL.createObjectURL(new Blob([svg], {type: 'image/svg+xml'}))
  try {
    const img = new Image()
    img.src = url
    await img.decode()
    const c = document.createElement('canvas')
    c.width = size.w; c.height = size.h
    const g = c.getContext('2d')!
    if (bg) { g.fillStyle = `rgb(${bg[0]}, ${bg[1]}, ${bg[2]})`; g.fillRect(0, 0, size.w, size.h) }
    g.drawImage(img, 0, 0, size.w, size.h)
    const blob = await new Promise<Blob | null>(r => c.toBlob(r, format === 'png' ? 'image/png' : 'image/jpeg', 0.92))
    if (!blob) throw new Error('画像を作れませんでした（大きすぎる可能性があります）')
    const bytes = new Uint8Array(await blob.arrayBuffer())
    let bin = ''
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    return btoa(bin)
  } finally {
    URL.revokeObjectURL(url)
  }
}
