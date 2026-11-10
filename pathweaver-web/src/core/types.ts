// Mirrors pathweaver-core's serde model. The Rust core validates every edit.
export interface Vec2 {x: number; y: number}
export type RGB = [number, number, number]

export interface PathNode {
  id: number
  p: Vec2
  incoming: Vec2
  outgoing: Vec2
  smooth: boolean
  exact_incoming?: boolean
  exact_outgoing?: boolean
}
export interface Width {left: number; right: number; linked: boolean; angle: number}
export interface WidthKey {id: number; segment: number; t: number; width: Width; interpolated?: boolean}
export type Join = 'Miter' | 'Round' | 'Bevel'

/** Live text: characters + font stay editable; contours = laid-out glyphs (placed by matrix). */
export interface TextSpec {text: string; font: string; size: number; line_height: number; matrix: Matrix}
/** A placed bitmap; the pixels are `Document.assets[asset]`. */
export interface Placed {asset: string; width: number; height: number; matrix: Matrix; opacity: number; reference?: boolean}
export type Matrix = [number, number, number, number, number, number]

export interface Stroke {
  id: number
  text?: TextSpec | null
  image?: Placed | null
  name: string
  visible: boolean
  closed: boolean
  nodes: PathNode[]
  widths: WidthKey[]
  start_width: Width
  end_width: Width
  start_angle: number
  end_angle: number
  start_cap_locked?: boolean
  end_cap_locked?: boolean
  fill: RGB | null
  edge: RGB | null
  edge_width: number
  frame_angle: number
  corners: {node: number; radius: number; kind: string}[]
  join: Join
  miter_limit: number
  contours?: Vec2[][] | null
  appearance?: Graph | null
  folder?: number | null
  /** A guide made from this path: a thin line to snap to, never painted or exported. */
  guide?: boolean
  start_cap?: Cap
  end_cap?: Cap
}
export type Cap = 'Butt' | 'Round' | 'Square'
export interface CornerWidget {node: number; p: Vec2; dir: Vec2; radius: number; kind: 'Circular' | 'Clothoid' | 'Bevel'; limit: number}

export type NodeKind =
  | 'Input' | 'Rough' | 'Zigzag' | 'Transform' | 'Fill' | 'Stack' | 'Output' | 'Reroute'
  | 'Pathfinder' | 'Boolean' | 'Offset' | 'Round' | 'Band' | 'Pucker' | 'Simplify' | 'Streak'
  | 'Dissolve' | 'Halftone' | 'Mezzotint' | 'Opacity' | 'Shadow' | 'Glow' | 'Blur' | 'Value' | 'Math' | 'Measure'
  | 'Rasterize' | 'RasterBlur' | 'Threshold' | 'Noise' | 'Trace' | 'Image'
export type ParamKey =
  | 'Amount' | 'Detail' | 'Seed' | 'Angle' | 'Spread' | 'Opacity' | 'Blur'
  | 'X' | 'Y' | 'Rotation' | 'Scale' | 'Value' | 'A' | 'B'

export interface Picture {name: string; width: number; height: number; png: string}
export interface GraphNode {
  id: number
  label: string
  kind: NodeKind
  position: [number, number]
  inputs: (number | null)[]
  bypass: boolean
  bindings: [ParamKey, number][]
  color: RGB
  smooth: boolean
  inner: boolean
  both?: boolean
  keep_source?: boolean
  /** ノイズ・しきい値・画像トレース: judge by darkness on white (ink), not alpha. */
  by_darkness?: boolean
  picture?: Picture | null
  operation: 'Union' | 'Difference' | 'Intersect' | 'Xor'
  math: string
  measure_height: boolean
  effect: {join: 'Round' | 'Miter' | 'Bevel'; position: 'Outer' | 'Inner' | 'Center'; grain?: 'Dots' | 'Lines'; [k: string]: unknown}
  [k: string]: unknown
}
/** `shared`: data-block id when several paths use this appearance (edits sync). */
export interface Graph {name: string; shared?: number | null; enabled: boolean; nodes: GraphNode[]; output: number}
export interface Preset {name: string; category: string; graph: Graph}

export interface Guide {vertical: boolean; position: number}
export interface Document {
  format: string
  version: number
  next_id: number
  grid_spacing: number
  snap_grid: boolean
  snap_guides: boolean
  snap_tangent: boolean
  snap_normal: boolean
  guides: Guide[]
  angle_step: number
  appearance_presets: Preset[]
  folders?: Folder[]
  /** Placed images by key (data URLs). Left out of most core calls. */
  assets?: Record<string, string>
  strokes: Stroke[]
}
export interface Folder {id: number; name: string; visible: boolean; open: boolean; parent?: number | null}

export interface Anchor {stroke: number; node: number}
export interface RenderLayer {
  color: string; opacity: number; d: string
  image?: {href: string; x: number; y: number; w: number; h: number}
  /** A placed image: the canvas draws `Document.assets[asset]` with `matrix`. */
  placed?: {asset: string; w: number; h: number; matrix: Matrix; reference: boolean}
}
export interface Render {layers: RenderLayer[]; hit: string; center: string; error?: string; stale?: boolean}
export interface WidthHandle {
  target: 'start' | 'end' | number
  center: Vec2; dir: Vec2
  left: number; right: number; linked: boolean; angle: number; interpolated: boolean
}
export interface Frame {origin: Vec2; tangent: Vec2}
export interface SnapGuide {origin: Vec2; direction: Vec2; kind: string}
export interface ParamInfo {key: ParamKey; label: string; min: number; max: number; value: number}
export interface NodeInfo {label: string; inputs: number; number: boolean; params: ParamInfo[]}
export interface KindInfo {kind: NodeKind; label: string; inputs: number; number: boolean}
