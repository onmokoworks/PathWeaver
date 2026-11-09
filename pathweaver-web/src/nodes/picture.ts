// Pictures for the 画像 node: any image the browser can decode is drawn to a
// canvas (at most 2048 px on the long side and 4M pixels) and embedded in the
// document as PNG, so the file travels with the document.
import type {Picture} from '../core/types'

const MAX_SIDE = 2048
const MAX_PIXELS = 4_000_000

export async function loadPicture(file: Blob & {name?: string}): Promise<Picture> {
  const url = URL.createObjectURL(file)
  try {
    const img = new Image()
    img.src = url
    await img.decode().catch(() => { throw new Error('この画像は読み込めません') })
    let w = img.naturalWidth, h = img.naturalHeight
    if (!w || !h) throw new Error('この画像は読み込めません')
    const k = Math.min(1, MAX_SIDE / Math.max(w, h), Math.sqrt(MAX_PIXELS / (w * h)))
    w = Math.max(1, Math.round(w * k))
    h = Math.max(1, Math.round(h * k))
    const canvas = document.createElement('canvas')
    canvas.width = w
    canvas.height = h
    const ctx = canvas.getContext('2d')!
    ctx.imageSmoothingQuality = 'high'
    ctx.drawImage(img, 0, 0, w, h)
    const png = canvas.toDataURL('image/png').replace(/^data:image\/png;base64,/, '')
    return {name: file.name ?? '画像', width: w, height: h, png}
  } finally {
    URL.revokeObjectURL(url)
  }
}

/** Ask for an image file (a hidden file input; the Mac host shows its open panel). */
export function pickImageFile(): Promise<File | null> {
  return new Promise(resolve => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = 'image/*'
    input.style.display = 'none'
    document.body.append(input)
    input.addEventListener('change', () => { resolve(input.files?.[0] ?? null); input.remove() }, {once: true})
    input.addEventListener('cancel', () => { resolve(null); input.remove() }, {once: true})
    input.click()
  })
}

export const pictureUrl = (p: Picture) => `data:image/png;base64,${p.png}`
