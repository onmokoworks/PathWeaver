export type GlyphMetrics = {
  unitsPerEm: number
  width: number
  height: number
  baseline: number
  capHeight: number
  xHeight: number
  ascender: number
  descender: number
}

export type GridSettings = {
  spacing: number
  radius: number
  originX: number
  originY: number
  visibleRows: number
  visibleCols: number
  visible: boolean
}

export type GridDot = {
  id: string
  row: number
  col: number
  x: number
  y: number
  radius: number
}

export type Point = {
  x: number
  y: number
}

export type StartPort = 'top' | 'right' | 'bottom' | 'left'

export type StartAnchor = Point & {
  pinId: string
  row: number
  col: number
  port: StartPort
  angle: number
}

export type ThreadContact = {
  pinId: string
  row: number
  col: number
  entryAngle: number
  exitAngle: number
  direction?: 'cw' | 'ccw'
}

export type ThreadSegment =
  | {
      type: 'line'
      from: Point
      to: Point
    }
  | {
      type: 'arc'
      pinId: string
      cx: number
      cy: number
      radius: number
      from: Point
      to: Point
      startAngle: number
      endAngle: number
      direction: 'cw' | 'ccw'
      largeArc: boolean
    }

export type RenderedThread = {
  segments: ThreadSegment[]
  threadPathData: string
  previewPathData?: string
  fillPathData: string
  invalid: boolean
}

export type ActiveThread = RenderedThread & {
  startAnchor: StartAnchor
  contacts: ThreadContact[]
  cursor: Point
  canClose: boolean
  activePinId?: string
}

export type GlyphContour = RenderedThread & {
  id: string
  startAnchor: StartAnchor
  contacts: ThreadContact[]
  visible: boolean
  selected?: boolean
}

export type EditorMode = 'idle' | 'waiting-start' | 'drawing' | 'closed' | 'invalid'

export type DebugState = {
  mode: EditorMode
  activePinId?: string
  hoverStartPinId?: string
  hoverStartPort?: StartPort
  contactCount?: number
  direction?: 'cw' | 'ccw'
  canClose?: boolean
  invalid?: boolean
}

export type GlyphState = {
  char: string
  advanceWidth: number
  metrics: GlyphMetrics
  gridSettings: GridSettings
  gridDots: GridDot[]
  contours: GlyphContour[]
  activeThread?: ActiveThread
}

export type ProjectState = {
  version: number
  fontName: string
  currentGlyph: string
  glyphs: Record<string, GlyphState>
}

export type HistoryState = {
  past: ProjectState[]
  present: ProjectState
  future: ProjectState[]
}
