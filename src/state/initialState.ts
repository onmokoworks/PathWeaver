import { createGridDots } from '../geometry/grid'
import type { GlyphMetrics, GlyphState, GridSettings, ProjectState } from './types'

export const defaultMetrics: GlyphMetrics = {
  unitsPerEm: 1000,
  width: 1000,
  height: 1000,
  baseline: 820,
  capHeight: 180,
  xHeight: 520,
  ascender: 120,
  descender: 940,
}

export const defaultGridSettings: GridSettings = {
  spacing: 180,
  radius: 52,
  originX: 500,
  originY: 500,
  visibleRows: 7,
  visibleCols: 7,
  visible: true,
}

export function createEmptyGlyph(char = 'A'): GlyphState {
  return {
    char,
    advanceWidth: 760,
    metrics: defaultMetrics,
    gridSettings: defaultGridSettings,
    gridDots: createGridDots(defaultGridSettings),
    contours: [],
  }
}

export function createInitialProject(): ProjectState {
  return {
    version: 4,
    fontName: 'ThreadType Hooked Study',
    currentGlyph: 'A',
    glyphs: {
      A: createEmptyGlyph('A'),
    },
  }
}
