import type { GlyphState } from '../state/types'

export function buildGlyphSvg(glyph: GlyphState) {
  const paths = glyph.contours
    .filter((contour) => contour.visible)
    .map(
      (contour) => `  <path d="${contour.fillPathData}" fill="black" />
  <path d="${contour.threadPathData}" fill="none" stroke="black" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />`,
    )
    .join('\n')

  return `<svg viewBox="0 0 ${glyph.metrics.width} ${glyph.metrics.height}" xmlns="http://www.w3.org/2000/svg">\n${paths}\n</svg>`
}

export function downloadTextFile(filename: string, contents: string, type: string) {
  const blob = new Blob([contents], { type })
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
  URL.revokeObjectURL(url)
}
