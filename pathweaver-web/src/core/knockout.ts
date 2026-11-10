// 抜き: key the black (Unmult) or the white (paper) out of a placed image.
// The document keeps the original pixels; the keyed copy is derived here,
// cached per asset, and handed to the core only for export (`asset#black`).
import {reactive} from 'vue'
import type {Document, Knockout} from './types'

/**
 * Key one RGBA buffer in place (straight alpha, 0–255).
 * Black: coverage = brightest channel, colour divided back out — black goes
 * clear, light and glow keep their colour (After Effects' Unmult).
 * White: the same on the inverted image — white paper goes clear, ink stays.
 */
export function knockPixels(px: Uint8ClampedArray, mode: Knockout) {
  for (let i = 0; i < px.length; i += 4) {
    const a = px[i + 3] / 255
    if (!a) continue
    const r = px[i] / 255, g = px[i + 1] / 255, b = px[i + 2] / 255
    if (mode === 'black') {
      const m = Math.max(r, g, b)
      px[i + 3] = Math.round(m * a * 255)
      if (m > 0) { px[i] = Math.round((r / m) * 255); px[i + 1] = Math.round((g / m) * 255); px[i + 2] = Math.round((b / m) * 255) }
    } else {
      const m = Math.max(1 - r, 1 - g, 1 - b)
      px[i + 3] = Math.round(m * a * 255)
      if (m > 0) { px[i] = Math.round((1 - (1 - r) / m) * 255); px[i + 1] = Math.round((1 - (1 - g) / m) * 255); px[i + 2] = Math.round((1 - (1 - b) / m) * 255) }
    }
  }
}

export const keyedName = (asset: string, mode: Knockout) => `${asset}#${mode}`

// keyed data URLs by `asset#mode` (reactive: the canvas redraws when one is ready)
const keyed = reactive(new Map<string, string>())
const pending = new Map<string, Promise<string>>()

async function make(src: string, mode: Knockout): Promise<string> {
  const img = new Image()
  img.src = src
  await img.decode()
  const c = document.createElement('canvas')
  c.width = img.naturalWidth; c.height = img.naturalHeight
  const g = c.getContext('2d', {colorSpace: 'srgb', willReadFrequently: true} as CanvasRenderingContext2DSettings)!
  g.drawImage(img, 0, 0)
  const data = g.getImageData(0, 0, c.width, c.height)
  knockPixels(data.data, mode)
  g.putImageData(data, 0, 0)
  return c.toDataURL('image/png')
}

/** The keyed copy, computed once per asset and mode. */
export function ensureKeyed(asset: string, src: string, mode: Knockout): Promise<string> {
  const k = keyedName(asset, mode)
  const done = keyed.get(k)
  if (done) return Promise.resolve(done)
  let p = pending.get(k)
  if (!p) {
    p = make(src, mode).then(url => { keyed.set(k, url); return url }).finally(() => pending.delete(k))
    pending.set(k, p)
  }
  return p
}

/**
 * What to draw for a placed image: the keyed copy once it is ready. Until
 * then nothing (an unkeyed flash of black would read as a bug).
 */
export function placedHref(assets: Record<string, string> | undefined, asset: string, mode?: Knockout | null): string | undefined {
  const src = assets?.[asset]
  if (!mode || !src) return src
  const done = keyed.get(keyedName(asset, mode))
  if (done) return done
  void ensureKeyed(asset, src, mode).catch(() => {})
  return undefined
}

/** Assets for export: the originals plus every keyed copy in use. */
export async function exportAssets(d: Document): Promise<Record<string, string>> {
  const out = {...(d.assets ?? {})}
  for (const s of d.strokes) {
    const img = s.image
    if (!img?.knockout || !out[img.asset]) continue
    out[keyedName(img.asset, img.knockout)] = await ensureKeyed(img.asset, out[img.asset], img.knockout)
  }
  return out
}
