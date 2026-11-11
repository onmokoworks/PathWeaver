// Pointer and keyboard behaviour of the artwork canvas.
// Follows Illustrator conventions where PathWeaver has the same concept:
//   Pen: click = corner, drag = smooth, Option-drag breaks the handle,
//        hovering a segment shows "+" and clicking adds an anchor,
//        clicking an open endpoint continues that path, first point closes.
//   Direct (A): anchors/handles/segments; Option-click or double-click a
//        segment adds an anchor; Option-drag a handle breaks the pair.
import {reactive, watch} from 'vue'
import {call} from '../core/wasm'
import {baselineTop, canCarryText, DEFAULT_FONT} from '../core/textOutline'
import {measurePath, nearestOnPath} from '../core/textLayout'
import {buildShapes, defaults, fromDrag, isGroupShape, shapeInfo, type ShapeKind, type ShapeParams} from '../core/shapes'
import {readout} from '../core/units'
import {settings, wheelZoom, type Paper} from '../core/settings'
import type {Editor} from '../core/editor'
import type {Anchor, Artboard, CornerWidget, Document, Frame, PathNode, SnapGuide, Stroke, Vec2, WidthHandle} from '../core/types'
import {add, affine, Bounds, dist, inside, len, rectHitsRings, renderBounds, rings, scale, sub, ZERO} from './geometry'
import {alignMoves, boxXs, boxYs, distributeMoves, smartSnap, snapTargets, union, type AlignKind, type AlignTo, type DistributeKind, type SmartGuide, type SnapTargets} from '../core/arrange'

export interface View {panX: number; panY: number; zoom: number; width: number; height: number}
export const RULER = 20

type Matrix = [number, number, number, number, number, number]
type Drag =
  | {kind: 'pan'; last: Vec2}
  | {kind: 'marquee'; start: Vec2; current: Vec2; add: boolean; anchors: boolean}
  | {kind: 'move'; start: Vec2; origin: Vec2; base: string; moved: boolean; duplicate: boolean; box: Bounds | null; targets: SnapTargets | null}
  | {kind: 'scale'; base: string; box: Bounds; corner: Vec2; fixed: Vec2; targets: SnapTargets | null}
  | {kind: 'rotate'; base: string; center: Vec2; startAngle: number}
  | {kind: 'pivot'; box: Bounds}
  | {kind: 'artboard'; mode: 'create' | 'move' | 'resize'; id: number; start: Vec2; base: Artboard; handle: number}
  | {kind: 'anchors'; start: Vec2; origin: Vec2; base: Map<string, Vec2>; moved: boolean; targets: SnapTargets | null}
  | {kind: 'handle'; at: Anchor; side: 'in' | 'out'}
  | {kind: 'pen'; at: Anchor; start: Vec2; moved: boolean; mode: 'new' | 'out' | 'close'}
  | {kind: 'convert'; at: Anchor; start: Vec2; moved: boolean}
  | {kind: 'width'; stroke: number; target: WidthHandle['target']; side: 'left' | 'right'; base: {left: number; right: number}}
  | {kind: 'widthSlide'; stroke: number; key: number}
  | {kind: 'cap'; stroke: number; end: 'start' | 'end'; anchor: Vec2}
  | {kind: 'corner'; stroke: number; node: number}
  | {kind: 'guide'; index: number}
  | {kind: 'shape'; start: Vec2; strokes: number[]; moved: boolean; params: ShapeParams}

export interface Hover {
  cursor: string
  segment: {stroke: number; point: Vec2} | null
  anchor: Anchor | null
  pointer: Vec2 | null
}

export type {Paper} from '../core/settings'
function distToSegment(p: Vec2, a: Vec2, b: Vec2) {
  const v = sub(b, a)
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * v.x + (p.y - a.y) * v.y) / Math.max(v.x * v.x + v.y * v.y, 1e-12)))
  return dist(p, add(a, scale(v, t)))
}
const akey = (a: Anchor) => `${a.stroke}:${a.node}`

/** The reference point (基準点): where it sits in the selection's box, 0–1 on each axis. */
export interface Pivot {u: number; v: number}
function loadPivot(): Pivot {
  try { const v = JSON.parse(localStorage.getItem('pw.pivot') ?? 'null'); if (Number.isFinite(v?.u) && Number.isFinite(v?.v)) return {u: v.u, v: v.v} } catch { /* default */ }
  return {u: 0.5, v: 0.5}
}
/** The nine points of the locator (corners, edge middles, centre). */
export const PIVOT_POINTS: Pivot[] = [0, 0.5, 1].flatMap(v => [0, 0.5, 1].map(u => ({u, v})))
export const pivotIn = (b: Bounds, q: Pivot): Vec2 => ({x: b.x0 + q.u * (b.x1 - b.x0), y: b.y0 + q.v * (b.y1 - b.y0)})

function loadAlignTo(): AlignTo {
  try { const v = localStorage.getItem('pw.alignTo'); if (v === 'selection' || v === 'key' || v === 'artboard') return v } catch { /* default */ }
  return 'selection'
}

function loadShapeKind(): ShapeKind {
  try { const v = localStorage.getItem('pw.shape') as ShapeKind | null; if (v && shapeInfo(v)) return v } catch { /* default */ }
  return 'rect'
}

export function createCanvas(ed: Editor) {
  const view = reactive<View>({panX: 200, panY: 120, zoom: 1, width: 800, height: 600})
  const state = reactive({
    drag: null as Drag | null,
    hover: {cursor: 'default', segment: null, anchor: null, pointer: null} as Hover,
    snap: null as SnapGuide | null,
    penPath: null as number | null,      // id of the open path being drawn
    widthTarget: null as {stroke: number; target: WidthHandle['target']} | null,
    space: false,
    meta: false,
    alt: false,           // Option held: a width drag changes only the grabbed side
    hoverGrip: null as {stroke: number; target: WidthHandle['target']; side: 'left' | 'right'} | null,          // ⌘ held: show width bands, add/slide width points, rotate caps
    // per-viewer settings (settings.ts), read through the state for templates
    get showWidth() { return settings.showWidth },
    // Artwork preview background: independent of the UI theme, not exported.
    get paper() { return settings.paper },
    // Pen point placed ON another path (Shift+click): the next point snaps to
    // that path's normal there, and the normal is drawn as a guide.
    pathFrame: null as {origin: Vec2; tangent: Vec2; node: number} | null,
    // Where the pen would land right now (snapped), shown while hovering.
    snapAt: null as Vec2 | null,
    // Text being typed on the canvas (the overlay editor); `created` = made by
    // this edit, so an empty result removes it without an undo step.
    textEdit: null as {stroke: number; created: boolean} | null,
    // Shape tool: the kind drawn next, and the shape just made (its
    // parameters stay adjustable until the document changes otherwise).
    shapeKind: loadShapeKind(),
    lastShape: null as {strokes: number[]; params: ShapeParams; revision: number} | null,
    // Reference point for position, size and rotation (a view setting: one
    // for every selection, like Blender's pivot point). Not undoable.
    pivot: loadPivot(),
    // Artboard tool: the artboard being edited (its panel shows on the right).
    artboard: null as number | null,
    // Align / distribute: what to line up with (a view setting, like the pivot).
    alignTo: loadAlignTo(),
    // Spacing used by 間隔を揃える with a key object (Illustrator's 分布間隔).
    alignGap: 0,
    // Smart guides shown during a move / resize (world segments).
    smart: [] as SmartGuide[],
  })
  watch(() => state.alignTo, v => { try { localStorage.setItem('pw.alignTo', v) } catch { /* preference only */ } })
  watch(() => [state.pivot.u, state.pivot.v], () => { try { localStorage.setItem('pw.pivot', JSON.stringify(state.pivot)) } catch { /* preference only */ } })
  function setPivot(q: Pivot) { state.pivot = {u: q.u, v: q.v} }
  let frames: Frame[] = []
  let framesKey = ''  // what `frames` was captured for while hovering
  let lastClick = {t: 0, p: ZERO}

  // A replaced document (new / open / example) owns none of the old
  // interaction state: a half-drawn path must not continue into a path that
  // happens to share its id, and a drag must not apply to the new document.
  watch(() => ed.generation, () => {
    state.drag = null
    state.snap = null
    state.smart = []
    state.penPath = null
    state.pathFrame = null
    state.widthTarget = null
    state.hoverGrip = null
    state.hover = {cursor: 'default', segment: null, anchor: null, pointer: null}
    frames = []
  })

  const toWorld = (p: Vec2): Vec2 => ({x: (p.x - view.panX) / view.zoom, y: (p.y - view.panY) / view.zoom})
  const toScreen = (p: Vec2): Vec2 => ({x: p.x * view.zoom + view.panX, y: p.y * view.zoom + view.panY})
  const tol = (px = 7) => px / view.zoom
  /** A 下絵 (tracing reference) stays put: only the layer list selects it. */
  const pickable = (s: Stroke) => ed.shown(s) && !s.image?.reference && s.blend_of == null && !ed.locked(s)

  /** Paths whose anchors the tools may touch (locked ones are left alone). */
  const editable = () => ed.doc.strokes.filter(s => ed.shown(s) && !s.contours && !s.guide && s.blend_of == null && !ed.locked(s))
  const pathGuides = () => ed.doc.strokes.filter(s => ed.shown(s) && s.guide && s.nodes.length > 1)
  const stroke = (id: number) => ed.findStroke(id)
  const nodeOf = (at: Anchor): PathNode | undefined => stroke(at.stroke)?.nodes.find(n => n.id === at.node)

  // ---------- hit tests ----------
  function anchorAt(p: Vec2, only?: (s: Stroke) => boolean): Anchor | null {
    let best: Anchor | null = null
    let bestD = tol(7)
    for (const s of [...editable()].reverse()) {
      if (only && !only(s)) continue
      for (const n of s.nodes) {
        const d = dist(n.p, p)
        if (d < bestD) { bestD = d; best = {stroke: s.id, node: n.id} }
      }
    }
    return best
  }
  function handleAt(p: Vec2): {at: Anchor; side: 'in' | 'out'} | null {
    for (const s of [...editable()].reverse()) {
      if (!ed.selected.includes(s.id)) continue
      for (const n of s.nodes) {
        for (const side of ['in', 'out'] as const) {
          const v = side === 'in' ? n.incoming : n.outgoing
          if (len(v) > 0.01 && dist(add(n.p, v), p) < tol(6)) return {at: {stroke: s.id, node: n.id}, side}
        }
      }
    }
    return null
  }
  function segmentAt(p: Vec2, px = 6, exclude?: number | null) {
    let best: {stroke: number; segment: number; t: number; distance: number; point: Vec2; tangent: Vec2} | null = null
    for (const s of editable()) {
      if (s.nodes.length < 2 || s.id === exclude) continue
      const r = call<{segment: number; t: number; distance: number; point: Vec2; tangent: Vec2} | null>('nearest', {stroke: s, p})
      if (r && r.distance < tol(px) && (!best || r.distance < best.distance)) best = {stroke: s.id, ...r}
    }
    return best
  }
  function pathAt(p: Vec2): number | null {
    for (const s of [...ed.doc.strokes].reverse()) {
      // A blend step stands for its blend: picking it picks the first source.
      if (s.blend_of != null && ed.shown(s)) {
        const r = ed.renders.get(s.id)
        const src = ed.blendOf(s.id)?.a
        const a = src != null ? stroke(src) : undefined
        if (r && a && !ed.locked(a) && inside(p, rings(r))) return a.id
        continue
      }
      if (!pickable(s)) continue
      const r = ed.renders.get(s.id)
      if (r && inside(p, rings(r))) return s.id
      if (s.nodes.some(n => dist(n.p, p) < tol(6))) return s.id
      if (s.guide && s.nodes.length > 1) {
        const r = call<{distance: number} | null>('nearest', {stroke: s, p})
        if (r && r.distance < tol(5)) return s.id
      }
    }
    const seg = segmentAt(p, 6)
    return seg ? seg.stroke : null
  }
  function guideAt(p: Vec2): number | null {
    let best: number | null = null
    let bestD = tol(5)
    ed.doc.guides.forEach((g, i) => {
      const d = Math.abs((g.vertical ? p.x : p.y) - g.position)
      if (d < bestD) { bestD = d; best = i }
    })
    return best
  }
  function widthHandles(s: Stroke): WidthHandle[] {
    try { return call<WidthHandle[]>('widthHandles', {stroke: s}) } catch { return [] }
  }
  /** Width bands are part of the A/P/⇧C tools (no separate width tool). */
  function widthVisible() {
    return (ed.tool === 'direct' || ed.tool === 'pen' || ed.tool === 'convert') && (state.showWidth || state.meta)
  }
  /** Grips sit 10 screen px outside the band, where they are drawn and grabbed. */
  const GRIP = 10
  function widthGrips(h: WidthHandle) {
    const o = GRIP / view.zoom
    return {left: add(h.center, scale(h.dir, h.left + o)), right: sub(h.center, scale(h.dir, h.right + o))}
  }
  function widthHandleAt(p: Vec2) {
    if (!widthVisible()) return null
    for (const id of [...ed.selected].reverse()) {
      const s = stroke(id)
      if (!s || s.contours) continue
      for (const h of widthHandles(s)) {
        const g = widthGrips(h)
        // Grab anywhere from just outside the band edge out to the grip, so
        // the handle is easy to hit (with or without Option).
        for (const side of ['left', 'right'] as const) {
          const sign = side === 'left' ? 1 : -1
          const edge = add(h.center, scale(h.dir, sign * ((side === 'left' ? h.left : h.right) + 3 / view.zoom)))
          if (dist(g[side], p) < tol(8) || distToSegment(p, edge, g[side]) < tol(6)) return {stroke: s.id, target: h.target, side}
        }
        if (dist(h.center, p) < tol(6)) return {stroke: s.id, target: h.target, side: null}
      }
    }
    return null
  }

  // ---------- selection bounds (select tool) ----------
  /** A path's own geometry bounds (what the selection box measures). */
  function strokeBounds(s: Stroke): Bounds | null {
    const r = ed.geometryRender(s)
    return r ? renderBounds(r, s) : null
  }
  function selectionBounds(): Bounds | null {
    return union(ed.selected.map(id => stroke(id)).filter((s): s is Stroke => !!s).map(strokeBounds).filter((b): b is Bounds => !!b))
  }
  function boxHandles(b: Bounds) {
    return [
      {x: b.x0, y: b.y0}, {x: (b.x0 + b.x1) / 2, y: b.y0}, {x: b.x1, y: b.y0}, {x: b.x1, y: (b.y0 + b.y1) / 2},
      {x: b.x1, y: b.y1}, {x: (b.x0 + b.x1) / 2, y: b.y1}, {x: b.x0, y: b.y1}, {x: b.x0, y: (b.y0 + b.y1) / 2},
    ]
  }

  // ---------- snapping ----------
  /** `p` seen from `origin`, its direction snapped (shown as a guide line). */
  function angleFrom(origin: Vec2, p: Vec2): Vec2 {
    const v = angleStep(sub(p, origin))
    const l = len(v)
    state.snap = l > 1e-9 ? {origin, direction: scale(v, 1 / l), kind: 'Angle'} : null
    state.snapAt = l > 1e-9 ? add(origin, v) : null
    return add(origin, v)
  }
  /** Shift: the direction snaps to the angle step (15° by default), length kept. */
  function angleStep(v: Vec2): Vec2 {
    const l = len(v)
    if (l < 1e-9) return v
    const step = (ed.doc.angle_step || 15) * Math.PI / 180
    const a = Math.round(Math.atan2(v.y, v.x) / step) * step
    return {x: Math.cos(a) * l, y: Math.sin(a) * l}
  }
  const snapping = () => {
    const d = ed.doc
    return d.snap_grid || d.snap_guides || d.snap_tangent || d.snap_normal
  }
  function captureFrames(primary: Anchor | null, exclude: (a: Anchor) => boolean) {
    const list: Anchor[] = primary ? [primary] : []
    for (const s of editable()) {
      if (ed.tool === 'select' && ed.selected.includes(s.id)) continue
      for (const n of s.nodes) {
        const a = {stroke: s.id, node: n.id}
        if (!exclude(a) && !(primary && akey(primary) === akey(a))) list.push(a)
      }
    }
    frames = list.length && snapping() ? call<Frame[]>('frames', {doc: ed.doc, anchors: list}) : []
    framesKey = ''
  }
  /** Snap while hovering with the pen, so the guide (normal, tangent, grid)
   *  is visible BEFORE clicking, at the same spot the click will use. */
  function hoverSnap(p: Vec2, bypass: boolean): Vec2 {
    const s = state.penPath !== null ? stroke(state.penPath) : null
    const last = s?.nodes.at(-1)
    const primary = s && last ? {stroke: s.id, node: last.id} : null
    const key = `${ed.revision}:${primary ? akey(primary) : '-'}`
    if (snapping() && key !== framesKey) { captureFrames(primary, () => false); framesKey = key }
    const q = snap(p, bypass, !snapping())
    state.snapAt = state.snap ? q : null
    return q
  }
  function snap(p: Vec2, bypass: boolean, pathFrameOnly = false): Vec2 {
    if (bypass) { state.snap = null; return p }
    let result = p
    let guide: SnapGuide | null = null
    if (snapping() && !pathFrameOnly) {
      const r = call<{p: Vec2; guide: SnapGuide | null}>('snap', {doc: ed.doc, p, frames, zoom: view.zoom})
      result = r.p
      guide = r.guide
    }
    // Guides made from paths: the nearest point on the line (beats grid/tangent).
    if (ed.doc.snap_guides && !pathFrameOnly) {
      let best = guide && guide.kind !== 'Grid' ? dist(result, p) : tol(8)
      for (const s of pathGuides()) {
        const r = call<{distance: number; point: Vec2; tangent: Vec2} | null>('nearest', {stroke: s, p})
        if (r && r.distance < best) { best = r.distance; result = r.point; guide = {origin: r.point, direction: r.tangent, kind: 'PathGuide'} }
      }
    }
    // An explicit "start on a path" frame always offers its normal (and its
    // tangent when tangent snapping is on), whatever the toggles say.
    const f = activePathFrame()
    if (f) {
      const dirs: [Vec2, string][] = [[{x: -f.tangent.y, y: f.tangent.x}, 'Normal']]
      if (ed.doc.snap_tangent) dirs.push([f.tangent, 'Tangent'])
      let best = guide ? dist(result, p) : tol(10)
      for (const [d, kind] of dirs) {
        const q = add(f.origin, scale(d, (p.x - f.origin.x) * d.x + (p.y - f.origin.y) * d.y))
        if (dist(q, p) < best) { best = dist(q, p); result = q; guide = {origin: f.origin, direction: d, kind} }
      }
    }
    state.snap = guide
    return result
  }
  /** The path frame applies while it belongs to the pen path's last point. */
  function activePathFrame() {
    const f = state.pathFrame
    if (!f || state.penPath === null) return null
    const s = stroke(state.penPath)
    return s && s.nodes.at(-1)?.id === f.node ? f : null
  }

  // ---------- smart guides ----------
  // While moving or resizing, the selection's edges and centre snap to the
  // edges and centres of everything else on show and of the artboards. The
  // lines are gathered once when the drag starts.
  const SMART_PX = 5
  const smartOn = () => ed.doc.snap_objects !== false
  function captureTargets(moving: Set<number>): SnapTargets | null {
    if (!smartOn()) return null
    const boxes: Bounds[] = []
    for (const s of ed.doc.strokes) {
      if (moving.has(s.id) || !ed.shown(s) || s.guide) continue
      if (s.blend_of != null) { const b = ed.blendOf(s.id); if (b && (moving.has(b.a) || moving.has(b.b))) continue }
      const b = strokeBounds(s)
      if (b) boxes.push(b)
    }
    for (const a of boards()) boxes.push(boardBounds(a))
    return boxes.length ? snapTargets(boxes) : null
  }
  /** The selected paths with their blend partners (they move together). */
  function movingSet(ids: number[]) {
    const out = new Set(ids)
    for (const id of ids) { const b = ed.blendOf(id); if (b) { out.add(b.a); out.add(b.b) } }
    return out
  }

  // ---------- align / distribute ----------
  type Unit = {ids: number[]; box: Bounds}
  /**
   * What lines up as one: a folder whose shown paths are all selected (the
   * outermost such folder), a blend with both sources selected, else each path.
   */
  function arrangeUnits(): Unit[] {
    const sel = ed.selected.map(id => stroke(id)).filter((s): s is Stroke => !!s && ed.shown(s) && !ed.locked(s))
    const chosen = new Set(sel.map(s => s.id))
    const whole = new Map<number, boolean>()
    const folderWhole = (id: number) => {
      if (!whole.has(id)) whole.set(id, ed.doc.strokes.filter(v => v.blend_of == null && ed.shown(v) && ed.inFolder(v, id)).every(v => chosen.has(v.id)))
      return whole.get(id)!
    }
    const units = new Map<string, number[]>()
    for (const s of sel) {
      let key = `p${s.id}`
      let f = ed.folderOf(s.folder)
      for (let depth = 0; f && depth < 64; depth++) {
        if (folderWhole(f.id)) key = `f${f.id}`
        f = ed.folderOf(f.parent)
      }
      const b = key.startsWith('p') ? ed.blendOf(s.id) : undefined
      if (b && chosen.has(b.a) && chosen.has(b.b)) key = `b${b.id}`
      // a compound path moves as one shape
      if (key.startsWith('p') && s.compound != null) key = `c${s.compound}`
      ;(units.get(key) ?? units.set(key, []).get(key)!).push(s.id)
    }
    const out: Unit[] = []
    for (const [key, ids] of units) {
      // a blend's box includes its steps
      const parts = key.startsWith('b') ? [...ids, ...ed.doc.strokes.filter(v => v.blend_of === +key.slice(1)).map(v => v.id)] : ids
      const box = union(parts.map(id => stroke(id)).filter((v): v is Stroke => !!v).map(strokeBounds).filter((v): v is Bounds => !!v))
      if (box) out.push({ids, box})
    }
    return out
  }
  /** The key object (Illustrator): the unit of the active path, the last one clicked. */
  function keyUnit(units: Unit[] = arrangeUnits()): Unit | undefined {
    const id = ed.active?.id
    return id == null ? undefined : units.find(u => u.ids.includes(id))
  }
  /** The artboard to align with: the one under the selection's centre, else the edited one, else the first. */
  function alignBoard(box: Bounds | null): Artboard | null {
    const list = boards()
    if (!list.length) return null
    const c = box ? {x: (box.x0 + box.x1) / 2, y: (box.y0 + box.y1) / 2} : null
    return (c && list.find(a => inBox(boardBounds(a), c))) || list.find(a => a.id === state.artboard) || list[0]
  }
  /** One item always lines up with the artboard. */
  const alignMode = (units: Unit[]): AlignTo => (units.length < 2 ? 'artboard' : state.alignTo)
  function moveUnits(units: Unit[], moves: Vec2[], label: string) {
    const still = (m: Vec2) => Math.abs(m.x) < 1e-9 && Math.abs(m.y) < 1e-9
    if (moves.every(still)) { ed.status = `${label}：すでに揃っています`; return }
    ed.edit(d => {
      units.forEach((u, i) => {
        if (still(moves[i])) return
        const r = call<{doc: Document}>('edit', {doc: d, kind: 'transform', ids: u.ids, matrix: [1, 0, 0, 1, moves[i].x, moves[i].y]})
        d.strokes = r.doc.strokes
      })
    })
    ed.status = label
  }
  const ALIGN_LABEL: Record<AlignKind, string> = {left: '左揃え', hcenter: '左右中央揃え', right: '右揃え', top: '上揃え', vcenter: '上下中央揃え', bottom: '下揃え'}
  const DIST_LABEL: Record<DistributeKind, string> = {hcenter: '左右中央で分布', vcenter: '上下中央で分布', hgap: '横の間隔を揃える', vgap: '縦の間隔を揃える'}
  function align(kind: AlignKind) {
    const units = arrangeUnits()
    if (!units.length) return
    const mode = alignMode(units)
    let to: Bounds | null = null
    if (mode === 'artboard') { const a = alignBoard(union(units.map(u => u.box))); to = a ? boardBounds(a) : null }
    else if (mode === 'key') to = keyUnit(units)?.box ?? null
    else to = union(units.map(u => u.box))
    if (!to) { ed.status = mode === 'artboard' ? 'アートボードがありません' : 'キーオブジェクトがありません'; return }
    moveUnits(units, alignMoves(units.map(u => u.box), to, kind), ALIGN_LABEL[kind])
  }
  function distribute(kind: DistributeKind) {
    const units = arrangeUnits()
    if (units.length < 2) return
    const mode = alignMode(units)
    const boxes = units.map(u => u.box)
    const board = mode === 'artboard' ? alignBoard(union(boxes)) : null
    const k = mode === 'key' ? keyUnit(units) : undefined
    const key = k ? units.indexOf(k) : -1
    const opts = {
      range: board ? boardBounds(board) : undefined,
      key: key >= 0 ? key : undefined,
      gap: key >= 0 && (kind === 'hgap' || kind === 'vgap') ? state.alignGap : undefined,
    }
    moveUnits(units, distributeMoves(boxes, kind, opts), DIST_LABEL[kind])
  }

  // ---------- helpers ----------
  /** Back to the paths as they were when the drag began (`base` = strokes JSON). */
  function restoreBase(base: string) {
    ed.doc.strokes = JSON.parse(base) as Stroke[]
  }
  function transformSelected(m: Matrix) {
    const r = call<{doc: Document}>('edit', {doc: ed.doc, kind: 'transform', ids: ed.selected, matrix: m})
    ed.doc.strokes = r.doc.strokes
  }
  function finishPen() {
    if (state.penPath !== null) {
      const s = stroke(state.penPath)
      if (s && s.nodes.length < 2) ed.edit(d => { d.strokes = d.strokes.filter(v => v.id !== s.id) })
    }
    state.penPath = null
    state.pathFrame = null
  }

  // ---------- pointer ----------
  function down(e: PointerEvent, local: Vec2) {
    const p = toWorld(local)
    const now = performance.now()
    const double = now - lastClick.t < 350 && dist(local, lastClick.p) < 5
    lastClick = {t: now, p: local}
    if (e.button === 1 || (e.button === 0 && state.space)) {
      state.drag = {kind: 'pan', last: local}
      return
    }
    if (e.button !== 0) return
    ed.error = ''

    // Rulers create guides.
    if (local.x < RULER || local.y < RULER) {
      if (local.x < RULER && local.y < RULER) return
      const vertical = local.x < RULER
      ed.begin()
      ed.doc.guides.push({vertical, position: vertical ? p.x : p.y})
      ed.changed()
      state.drag = {kind: 'guide', index: ed.doc.guides.length - 1}
      return
    }

    if (state.textEdit) finishText()
    switch (ed.tool) {
      case 'text': return textDown(p)
      case 'shape': return shapeDown(e, p)
      case 'artboard': return artboardDown(e, p)
      case 'pen': return penDown(e, p)
      case 'direct': return directDown(e, p, double)
      case 'convert': return convertDown(e, p)
      default: return selectDown(e, p, double)
    }
  }

  // ---------- artboards ----------
  // Artboard tool (⇧O): drag on empty canvas = a new artboard, drag one =
  // move it, its handles = resize. The panel on the right edits the numbers.
  const boards = () => ed.doc.artboards ?? []
  const boardBounds = (a: Artboard): Bounds => ({x0: a.x, y0: a.y, x1: a.x + a.w, y1: a.y + a.h})
  function artboardAt(p: Vec2): Artboard | null {
    const list = boards()
    for (let i = list.length - 1; i >= 0; i--) { const a = list[i]; if (inBox(boardBounds(a), p)) return a }
    return null
  }
  /** The selected artboard's handle under p (index into boxHandles), or -1. */
  function artboardHandleAt(p: Vec2): number {
    const a = boards().find(b => b.id === state.artboard)
    if (!a) return -1
    return boxHandles(boardBounds(a)).findIndex(h => dist(h, p) < tol(6))
  }
  function artboardDown(e: PointerEvent, p: Vec2) {
    const hi = artboardHandleAt(p)
    const sel = boards().find(b => b.id === state.artboard)
    if (sel && hi >= 0) {
      ed.begin()
      state.drag = {kind: 'artboard', mode: 'resize', id: sel.id, start: p, base: {...sel}, handle: hi}
      return
    }
    const hit = artboardAt(p)
    if (hit) {
      state.artboard = hit.id
      ed.begin()
      state.drag = {kind: 'artboard', mode: 'move', id: hit.id, start: p, base: {...hit}, handle: -1}
      return
    }
    ed.begin()
    const id = ed.nextId()
    const list = ed.doc.artboards ?? (ed.doc.artboards = [])
    const q = snap(p, e.ctrlKey)
    const a: Artboard = {id, name: `アートボード ${list.length + 1}`, x: q.x, y: q.y, w: 1, h: 1}
    list.push(a)
    state.artboard = id
    state.drag = {kind: 'artboard', mode: 'create', id, start: q, base: {...a}, handle: -1}
  }
  function artboardMove(d: Extract<Drag, {kind: 'artboard'}>, raw: Vec2, e: PointerEvent) {
    const a = boards().find(b => b.id === d.id)
    if (!a) return
    const b = d.base
    if (d.mode === 'create') {
      let q = snap(raw, e.ctrlKey)
      if (e.shiftKey) { const m = Math.max(Math.abs(q.x - d.start.x), Math.abs(q.y - d.start.y)); q = {x: d.start.x + Math.sign(q.x - d.start.x || 1) * m, y: d.start.y + Math.sign(q.y - d.start.y || 1) * m} }
      Object.assign(a, {x: Math.min(d.start.x, q.x), y: Math.min(d.start.y, q.y), w: Math.max(Math.abs(q.x - d.start.x), 1), h: Math.max(Math.abs(q.y - d.start.y), 1)})
    } else if (d.mode === 'move') {
      let delta = sub(raw, d.start)
      if (e.shiftKey) delta = Math.abs(delta.x) > Math.abs(delta.y) ? {x: delta.x, y: 0} : {x: 0, y: delta.y}
      const q = snap({x: b.x + delta.x, y: b.y + delta.y}, e.ctrlKey)
      Object.assign(a, {x: q.x, y: q.y})
    } else {
      // handles: 0 tl, 1 t, 2 tr, 3 r, 4 br, 5 b, 6 bl, 7 l
      const q = snap(raw, e.ctrlKey)
      let x0 = b.x, y0 = b.y, x1 = b.x + b.w, y1 = b.y + b.h
      if ([0, 6, 7].includes(d.handle)) x0 = q.x
      if ([2, 3, 4].includes(d.handle)) x1 = q.x
      if ([0, 1, 2].includes(d.handle)) y0 = q.y
      if ([4, 5, 6].includes(d.handle)) y1 = q.y
      Object.assign(a, {x: Math.min(x0, x1), y: Math.min(y0, y1), w: Math.max(Math.abs(x1 - x0), 1), h: Math.max(Math.abs(y1 - y0), 1)})
    }
    ed.changed()
  }
  function artboardUp(d: Extract<Drag, {kind: 'artboard'}>) {
    const a = boards().find(b => b.id === d.id)
    // A click on empty canvas (no real drag) makes no artboard: it deselects.
    if (d.mode === 'create' && a && (a.w < tol(4) || a.h < tol(4))) {
      ed.doc.artboards = boards().filter(b => b.id !== d.id)
      state.artboard = null
      ed.changed()
    }
    ed.commit()
  }
  function deleteArtboard(id: number) {
    ed.edit(() => { ed.doc.artboards = boards().filter(b => b.id !== id) })
    if (state.artboard === id) state.artboard = null
  }

  // ---------- shapes ----------
  /** Write one generated path into a stroke (fresh anchor ids, live corners re-applied). */
  function fillPath(s: Stroke, b: ReturnType<typeof buildShapes>[number]) {
    s.nodes = b.nodes.map(n => ({...n, id: ed.nextId()}))
    s.closed = b.closed
    s.widths = []
    s.corners = []
    if (b.corners > 0 && s.nodes.length > 2) {
      try {
        const mini = {...ed.doc, strokes: [s], folders: [], assets: {}, blends: []}
        const r = call<{doc: Document}>('edit', {doc: mini, kind: 'setCorner', stroke: s.id, nodes: s.nodes.map(n => n.id), radius: b.corners, cornerKind: 'Circular'})
        s.corners = r.doc.strokes[0].corners
      } catch { /* too small for a corner: stays sharp */ }
    }
  }
  /**
   * Make the shape's paths from its parameters into the given strokes; grids
   * add or remove strokes as their divider counts change. Returns the ids.
   */
  function applyShape(ids: number[], params: ShapeParams): number[] {
    const built = buildShapes(params)
    const first = stroke(ids[0])
    if (!first) return ids
    const out: number[] = []
    built.forEach((b, i) => {
      let s = stroke(ids[i])
      if (!s) {
        s = {...JSON.parse(JSON.stringify(first)), id: ed.nextId()} as Stroke
        const at = ed.doc.strokes.findIndex(v => v.id === out[out.length - 1])
        ed.doc.strokes.splice(at + 1, 0, s)
        s.name = `${first.name.replace(/ \d+$/, '')} ${i + 1}`
      }
      // A grid is lines and rings: no fills stacked on each other.
      if (isGroupShape(params.kind)) s.fill = null
      fillPath(s, b)
      out.push(s.id)
    })
    const extra = new Set(ids.slice(built.length))
    if (extra.size) ed.doc.strokes = ed.doc.strokes.filter(v => !extra.has(v.id))
    return out
  }
  function shapeDown(e: PointerEvent, raw: Vec2) {
    const kind = state.shapeKind
    const info = shapeInfo(kind)
    captureFrames(null, () => false)
    const p = snap(raw, e.ctrlKey)
    ed.begin()
    const s = ed.newPath(p)
    if (isGroupShape(kind)) {
      // A grid's paths live together in a folder (Illustrator groups them).
      const d = ed.doc
      d.folders ??= []
      const folder = ed.nextId(d)
      d.folders.push({id: folder, name: `${info.label} ${d.folders.filter(f => f.name.startsWith(info.label)).length + 1}`, visible: true, open: false})
      s.folder = folder
      s.name = `${info.label} 1`
    } else {
      const count = ed.doc.strokes.filter(v => v.name.startsWith(info.label)).length
      s.name = `${info.label} ${count + 1}`
    }
    const params = {...(state.lastShape?.params.kind === kind ? state.lastShape.params : defaults(kind)), kind}
    ed.anchors = []
    state.drag = {kind: 'shape', start: p, strokes: [s.id], moved: false, params}
  }
  function shapeMove(d: Extract<Drag, {kind: 'shape'}>, e: PointerEvent, raw: Vec2) {
    if (!d.moved && dist(raw, d.start) < tol(3)) return
    d.moved = true
    const b = snap(raw, e.ctrlKey)
    d.params = fromDrag(d.params.kind, d.params, d.start, b, e.shiftKey, e.altKey, ed.doc.angle_step || 15)
    d.strokes = applyShape(d.strokes, d.params)
    ed.changed()
  }
  function shapeUp(d: Extract<Drag, {kind: 'shape'}>) {
    if (!stroke(d.strokes[0])) return ed.commit()
    if (!d.moved) {
      // A click makes a default-size shape there (Illustrator asks for the
      // size in a dialog; here the adjust panel takes it afterwards).
      const k = d.params.kind
      const radial = k === 'polygon' || k === 'star' || k === 'spiral'
      d.params = radial
        ? {...d.params, x: d.start.x, y: d.start.y, radius: d.params.radius || 50, radius2: d.params.radius2 || 25}
        : {...d.params, x: d.start.x, y: d.start.y, w: Math.abs(d.params.w) || 100, h: k === 'line' ? 0 : Math.abs(d.params.h) || 100}
      d.strokes = applyShape(d.strokes, d.params)
      ed.changed()
    }
    const s = stroke(d.strokes[0])!
    const tiny = d.moved && s.nodes.every(n => dist(n.p, s.nodes[0].p) < 1e-6)
    if (tiny) { ed.cancel(); return }
    ed.commit()
    ed.selected = [...d.strokes]
    state.lastShape = {strokes: d.strokes, params: d.params, revision: ed.revision}
  }
  /** Adjust Last Operation: rebuild the shape just made, still one undo step. */
  function adjustShape(patch: Partial<ShapeParams>) {
    const last = state.lastShape
    if (!last || !stroke(last.strokes[0]) || ed.revision !== last.revision) return
    last.params = {...last.params, ...patch}
    last.strokes = applyShape(last.strokes, last.params)
    ed.selected = [...last.strokes]
    ed.changed()
    last.revision = ed.revision
  }
  /** The adjust panel shows while nothing else changed the document. */
  const shapeAdjustable = () => !!state.lastShape && state.lastShape.revision === ed.revision && !!stroke(state.lastShape.strokes[0])
  function setShapeKind(k: ShapeKind) {
    state.shapeKind = k
    try { localStorage.setItem('pw.shape', k) } catch { /* preference only */ }
    setTool('shape')
  }

  // ---------- text ----------
  /** Click with the text tool: edit the text under the pointer, or start a new
   *  one there — on a path's line, a text that runs along the path from there. */
  function textDown(p: Vec2) {
    const hit = pathAt(p)
    if (hit !== null && stroke(hit)?.text) return editText(hit)
    const seg = segmentAt(p, 6)
    const carrier = seg ? stroke(seg.stroke) : undefined
    const line = canCarryText(carrier) ? measurePath(carrier.nodes, carrier.closed) : null
    const onPath = line && carrier ? {stroke: carrier.id, offset: nearestOnPath(line, p).offset} : null
    const d = ed.doc
    const size = 48, lh = 1.2
    const color = ed.drawStyle.fill ?? ed.drawStyle.edge ?? [32, 33, 36]
    ed.begin()
    const id = ed.nextId(d)
    // The click is the first line's baseline, near its left end (as in Illustrator).
    const top = p.y - baselineTop(DEFAULT_FONT, size, lh)
    d.strokes.push({
      id, name: 'テキスト', visible: true, closed: true, nodes: [], widths: [],
      start_width: {left: 0.5, right: 0.5, linked: true, angle: 0}, end_width: {left: 0.5, right: 0.5, linked: true, angle: 0},
      start_angle: 90, end_angle: 90, fill: [...color] as Stroke['fill'], edge: null, edge_width: 1, frame_angle: 0,
      corners: [], join: 'Miter', miter_limit: 10, contours: [], appearance: null,
      text: {text: '', font: DEFAULT_FONT, size, line_height: lh, matrix: [1, 0, 0, 1, p.x, top], ...(onPath ? {on_path: onPath} : {})},
    })
    if (onPath) ed.status = 'パスに沿って文字を打ちます。位置・反対側は右のパネル'
    ed.selected = [id]
    ed.anchors = []
    ed.changed()
    state.textEdit = {stroke: id, created: true}
  }
  function editText(id: number) {
    finishPen()
    ed.selected = [id]
    ed.anchors = []
    ed.begin()
    state.textEdit = {stroke: id, created: false}
  }
  /** Leave the text editor: one undo step; an empty text is removed. */
  function finishText() {
    const t = state.textEdit
    if (!t) return
    state.textEdit = null
    const s = stroke(t.stroke)
    if (s?.text && !s.text.text.trim()) {
      if (t.created) { ed.cancel(); ed.selected = []; return }
      ed.doc.strokes = ed.doc.strokes.filter(v => v.id !== s.id)
      ed.selected = []
      ed.changed()
    }
    ed.commit()
  }

  function selectDown(e: PointerEvent, p: Vec2, double = false) {
    // Double-click text: type into it (Illustrator switches to the type tool).
    if (double && !e.shiftKey) {
      const hit = pathAt(p)
      if (hit !== null && stroke(hit)?.text) { editText(hit); return }
    }
    const box = ed.selected.length ? selectionBounds() : null
    if (box) {
      const hs = boxHandles(box)
      // The reference point: drag it (it snaps to the nine points); double-click: back to the centre.
      // A handle or a path under it wins (grabbing a shape by its middle still moves it);
      // with ⌘ the point is taken anyway.
      const onPivot = dist(pivotIn(box, state.pivot), p) < tol(7)
      const grabPivot = () => {
        if (double) { setPivot({u: 0.5, v: 0.5}); return }
        state.drag = {kind: 'pivot', box}
      }
      if (onPivot && e.metaKey) return grabPivot()
      for (let i = 0; i < hs.length; i++) {
        if (dist(hs[i], p) < tol(6)) {
          const opposite = hs[(i + 4) % 8]
          ed.begin()
          state.drag = {kind: 'scale', base: JSON.stringify(ed.doc.strokes), box, corner: hs[i], fixed: opposite, targets: captureTargets(movingSet(ed.selected))}
          return
        }
      }
      if (onPivot && pathAt(p) === null) return grabPivot()
      // Just outside a corner: rotate around the reference point.
      for (const i of [0, 2, 4, 6]) {
        const d = dist(hs[i], p)
        if (d >= tol(6) && d < tol(20) && !inBox(box, p)) {
          const center = pivotIn(box, state.pivot)
          ed.begin()
          state.drag = {kind: 'rotate', base: JSON.stringify(ed.doc.strokes), center, startAngle: Math.atan2(p.y - center.y, p.x - center.x)}
          return
        }
      }
    }
    const hit = pathAt(p)
    if (hit !== null) {
      ed.selectPath(hit, e.shiftKey)
      // The selection tool takes a blend as one object (both sources).
      const bl = ed.blendOf(hit)
      if (bl && ed.selected.includes(hit)) {
        const partner = bl.a === hit ? bl.b : bl.a
        ed.selected = [...ed.selected.filter(v => v !== partner && v !== hit), partner, hit]
      }
      if (!ed.selected.includes(hit)) return
      ed.begin()
      if (e.altKey) ed.duplicateSelection()
      const origin = nearestSelectedAnchor(p) ?? p
      captureFrames(null, () => false)
      state.drag = {kind: 'move', start: p, origin, base: JSON.stringify(ed.doc.strokes), moved: false, duplicate: e.altKey, box: selectionBounds(), targets: captureTargets(movingSet(ed.selected))}
      return
    }
    const g = guideAt(p)
    if (g !== null) {
      ed.begin()
      state.drag = {kind: 'guide', index: g}
      return
    }
    if (!e.shiftKey) ed.clearSelection()
    state.drag = {kind: 'marquee', start: p, current: p, add: e.shiftKey, anchors: false}
  }
  const inBox = (b: Bounds, p: Vec2) => p.x >= b.x0 && p.x <= b.x1 && p.y >= b.y0 && p.y <= b.y1
  function nearestSelectedAnchor(p: Vec2): Vec2 | null {
    let best: Vec2 | null = null
    for (const id of ed.selected) for (const n of stroke(id)?.nodes ?? []) if (!best || dist(n.p, p) < dist(best, p)) best = n.p
    return best && {...best}
  }

  function penDown(e: PointerEvent, raw: Vec2) {
    if (widthDown(e, raw)) return
    const drawing = state.penPath !== null ? stroke(state.penPath) : undefined
    if (drawing && !drawing.closed) {
      const last = drawing.nodes[drawing.nodes.length - 1]
      const first = drawing.nodes[0]
      ed.begin()
      if (drawing.nodes.length > 1 && dist(first.p, raw) < tol(8)) {
        drawing.closed = true
        ed.changed()
        state.drag = {kind: 'pen', at: {stroke: drawing.id, node: first.id}, start: first.p, moved: false, mode: 'close'}
        return
      }
      if (dist(last.p, raw) < tol(8)) {
        // Clicking the last anchor removes its outgoing handle: the next
        // segment starts as a corner while the previous curve is kept.
        last.outgoing = {x: 0, y: 0}
        last.smooth = false
        ed.changed()
        state.drag = {kind: 'pen', at: {stroke: drawing.id, node: last.id}, start: last.p, moved: false, mode: 'out'}
        return
      }
      captureFrames({stroke: drawing.id, node: last.id}, () => false)
      const onPath = e.shiftKey ? segmentAt(raw, 8, drawing.id) : null
      // Shift off a path: the new segment's direction snaps to the angle step.
      const p = onPath ? onPath.point : e.shiftKey ? angleFrom(last.p, raw) : snap(raw, e.ctrlKey)
      const node: PathNode = {id: ed.nextId(), p, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false}
      drawing.nodes.push(node)
      state.pathFrame = onPath ? {origin: p, tangent: onPath.tangent, node: node.id} : null
      ed.selected = [drawing.id]
      ed.anchors = [{stroke: drawing.id, node: node.id}]
      ed.changed()
      state.drag = {kind: 'pen', at: {stroke: drawing.id, node: node.id}, start: p, moved: false, mode: 'new'}
      return
    }
    state.penPath = null
    // Continue an open path from either endpoint.
    const end = anchorAt(raw, s => !s.closed && s.nodes.length > 0)
    if (end) {
      const s = stroke(end.stroke)!
      const isFirst = s.nodes[0].id === end.node && s.nodes.length > 1
      const isLast = s.nodes[s.nodes.length - 1].id === end.node
      if (isFirst || isLast) {
        ed.begin()
        if (isFirst) {
          const r = call<{doc: Document}>('edit', {doc: ed.doc, kind: 'reverse', stroke: s.id})
          ed.doc.strokes = r.doc.strokes
          ed.changed()
        }
        state.penPath = s.id
        ed.selected = [s.id]
        ed.anchors = [end]
        const node = stroke(s.id)!.nodes.at(-1)!
        state.drag = {kind: 'pen', at: {stroke: s.id, node: node.id}, start: node.p, moved: false, mode: 'out'}
        ed.status = 'パスの続きを描きます'
        return
      }
    }
    // Shift+click on a path: start a new path ON it. The next point snaps to
    // the path's normal at that spot (lines perpendicular to a stroke).
    const base = e.shiftKey ? segmentAt(raw, 8) : null
    if (base) {
      ed.begin()
      const s = ed.newPath(base.point)
      ed.changed()
      state.penPath = s.id
      ed.anchors = [{stroke: s.id, node: s.nodes[0].id}]
      state.pathFrame = {origin: base.point, tangent: base.tangent, node: s.nodes[0].id}
      state.drag = {kind: 'pen', at: {stroke: s.id, node: s.nodes[0].id}, start: base.point, moved: false, mode: 'new'}
      ed.status = 'パスの上から描き始めました。次の点は法線方向（点線）に吸着します'
      return
    }
    // Hovering a segment: add an anchor without changing the shape.
    const seg = segmentAt(raw, 5)
    if (seg) {
      const at = ed.coreEdit<Anchor>('insertAnchor', {stroke: seg.stroke, segment: seg.segment, t: seg.t})
      if (at) {
        ed.selected = [at.stroke]
        ed.anchors = [at]
        ed.status = '形を保ったままアンカーを追加しました'
      }
      return
    }
    ed.begin()
    captureFrames(null, () => false)
    const p = snap(raw, e.ctrlKey)
    const s = ed.newPath(p)
    ed.changed()
    state.penPath = s.id
    ed.anchors = [{stroke: s.id, node: s.nodes[0].id}]
    state.drag = {kind: 'pen', at: {stroke: s.id, node: s.nodes[0].id}, start: p, moved: false, mode: 'new'}
  }

  /** Live corner widgets: drag = radius, Option+click = next corner kind. */
  function cornerWidgets(s: Stroke): CornerWidget[] {
    try { return call<CornerWidget[]>('cornerWidgets', {stroke: s, zoom: view.zoom}) } catch { return [] }
  }
  function cornerDown(e: PointerEvent, p: Vec2): boolean {
    for (const id of [...ed.selected].reverse()) {
      const s = stroke(id)
      if (!s || s.contours) continue
      const w = cornerWidgets(s).find(v => dist(v.p, p) < tol(6))
      if (!w) continue
      if (e.altKey) {
        const next = {Circular: 'Clothoid', Clothoid: 'Bevel', Bevel: 'Circular'}[w.kind]
        if (ed.coreEdit('setCorner', {stroke: s.id, nodes: [w.node], radius: w.radius, cornerKind: next}) !== undefined)
          ed.status = `角の種類：${{Circular: '円弧', Clothoid: 'クロソイド', Bevel: '面取り'}[next as CornerWidget['kind']]}`
        return true
      }
      ed.begin()
      state.drag = {kind: 'corner', stroke: s.id, node: w.node}
      return true
    }
    return false
  }
  function directDown(e: PointerEvent, p: Vec2, double: boolean) {
    if (widthDown(e, p)) return
    if (cornerDown(e, p)) return
    const h = handleAt(p)
    const a = anchorAt(p)
    if (h && !(a && ed.isAnchorSelected(a) === false && dist(nodeOf(a)!.p, p) < dist(add(nodeOf(h.at)!.p, nodeOf(h.at)![h.side === 'in' ? 'incoming' : 'outgoing']), p))) {
      ed.begin()
      state.drag = {kind: 'handle', at: h.at, side: h.side}
      return
    }
    if (a) {
      ed.selectAnchor(a, e.shiftKey)
      if (!ed.isAnchorSelected(a)) return
      startAnchorDrag(a, p)
      return
    }
    const seg = segmentAt(p)
    if (seg) {
      if (e.altKey || double) {
        const at = ed.coreEdit<Anchor>('insertAnchor', {stroke: seg.stroke, segment: seg.segment, t: seg.t})
        if (at) {
          ed.selected = [at.stroke]
          ed.anchors = [at]
          ed.status = '形を保ったままアンカーを追加しました'
        }
        return
      }
      // Grab the segment: select its two anchors and drag them together.
      const s = stroke(seg.stroke)!
      const a0 = {stroke: s.id, node: s.nodes[seg.segment].id}
      const a1 = {stroke: s.id, node: s.nodes[(seg.segment + 1) % s.nodes.length].id}
      if (!e.shiftKey) ed.anchors = []
      for (const at of [a0, a1]) if (!ed.isAnchorSelected(at)) ed.selectAnchor(at, true)
      startAnchorDrag(a0, p)
      return
    }
    if (!e.shiftKey) ed.clearSelection()
    state.drag = {kind: 'marquee', start: p, current: p, add: e.shiftKey, anchors: true}
  }
  function startAnchorDrag(grab: Anchor, p: Vec2) {
    ed.begin()
    const base = new Map<string, Vec2>()
    for (const a of ed.anchors) { const n = nodeOf(a); if (n) base.set(akey(a), {...n.p}) }
    captureFrames(grab, a => ed.isAnchorSelected(a))
    state.drag = {kind: 'anchors', start: p, origin: {...nodeOf(grab)!.p}, base, moved: false, targets: captureTargets(new Set(ed.anchors.map(a => a.stroke)))}
  }

  function convertDown(e: PointerEvent, p: Vec2) {
    if (widthDown(e, p)) return
    const h = handleAt(p)
    if (h) {
      const n = nodeOf(h.at)!
      ed.begin()
      n.smooth = false
      state.drag = {kind: 'handle', at: h.at, side: h.side}
      return
    }
    const a = anchorAt(p)
    if (a) {
      ed.selectAnchor(a, false)
      ed.begin()
      state.drag = {kind: 'convert', at: a, start: p, moved: false}
      return
    }
    directDown(e, p, false)
  }

  /**
   * Width editing inside A / P / ⇧C (original PathWeaver behaviour):
   *   drag a band end = both sides, Option+drag = that side only,
   *   ⌘+click the centerline = add a width point, ⌘+drag a point's centre =
   *   slide it along the path, ⌘+drag an endpoint = rotate that end's cut.
   * Returns true when the press was consumed.
   */
  function widthDown(e: PointerEvent, p: Vec2): boolean {
    const meta = e.metaKey || state.meta
    const h = widthHandleAt(p)
    // ⌘ + any part of a width point's handle (grips or centre): slide it
    // along the path.
    if (meta && h && typeof h.target === 'number') {
      state.widthTarget = {stroke: h.stroke, target: h.target}
      ed.begin()
      state.drag = {kind: 'widthSlide', stroke: h.stroke, key: h.target}
      return true
    }
    if (h?.side) {
      state.widthTarget = {stroke: h.stroke, target: h.target}
      const w = widthHandles(stroke(h.stroke)!).find(v => v.target === h.target)
      ed.begin()
      state.drag = {kind: 'width', stroke: h.stroke, target: h.target, side: h.side, base: {left: w?.left ?? 0, right: w?.right ?? 0}}
      return true
    }
    if (!meta) {
      if (h && typeof h.target === 'number') { state.widthTarget = {stroke: h.stroke, target: h.target}; return true }
      state.widthTarget = null
      return false
    }
    if (h && typeof h.target === 'number') {
      state.widthTarget = {stroke: h.stroke, target: h.target}
      ed.begin()
      state.drag = {kind: 'widthSlide', stroke: h.stroke, key: h.target}
      return true
    }
    // ⌘ + an open path's endpoint: rotate its cut.
    const end = anchorAt(p, s => !s.closed && s.nodes.length > 1 && ed.selected.includes(s.id))
    if (end) {
      const s = stroke(end.stroke)!
      const which = s.nodes[0].id === end.node ? 'start' : s.nodes.at(-1)!.id === end.node ? 'end' : null
      if (which) {
        ed.begin()
        state.drag = {kind: 'cap', stroke: s.id, end: which, anchor: {...nodeOf(end)!.p}}
        return true
      }
    }
    // ⌘ + centerline: add a width point (it follows the current profile).
    // A compound path's members have no width profile (their line is one width).
    const seg = segmentAt(p, 8)
    if (seg && stroke(seg.stroke)?.compound == null) {
      if (!ed.selected.includes(seg.stroke)) ed.selectPath(seg.stroke)
      const id = ed.coreEdit<number>('insertWidth', {stroke: seg.stroke, segment: seg.segment, t: seg.t})
      if (id !== undefined) {
        state.widthTarget = {stroke: seg.stroke, target: id}
        ed.status = '幅点を追加しました。端をドラッグで太さ、Optionで片側だけ'
      }
      return true
    }
    return false
  }

  function move(e: PointerEvent, local: Vec2) {
    state.meta = e.metaKey
    state.alt = e.altKey
    const raw = toWorld(local)
    state.hover.pointer = raw
    const d = state.drag
    if (!d) return hover(e, local, raw)
    switch (d.kind) {
      case 'pan':
        view.panX += local.x - d.last.x
        view.panY += local.y - d.last.y
        d.last = local
        return
      case 'marquee':
        d.current = raw
        return
      case 'guide': {
        const g = ed.doc.guides[d.index]
        if (g) g.position = g.vertical ? raw.x : raw.y
        ed.changed()
        return
      }
      case 'shape': return shapeMove(d, e, raw)
      case 'move': {
        if (!d.moved && dist(raw, d.start) < tol(2)) return
        d.moved = true
        let target = add(d.origin, sub(raw, d.start))
        const v = sub(raw, d.start)
        const along = e.shiftKey ? (Math.abs(v.x) > Math.abs(v.y) ? 'x' : 'y') : null
        if (along) {
          target = along === 'x' ? add(d.origin, {x: v.x, y: 0}) : add(d.origin, {x: 0, y: v.y})
        } else if (!e.ctrlKey && snapping()) {
          const r = call<{p: Vec2; guide: SnapGuide | null}>('snapTranslation', {doc: ed.doc, p: raw, start: d.start, origin: d.origin, frames, zoom: view.zoom})
          target = r.p
          state.snap = r.guide
        }
        const delta = sub(target, d.origin)
        // Smart guides: the moved box's edges / centre onto other objects'.
        state.smart = []
        if (!e.ctrlKey && d.targets && d.box) {
          const free = along === 'x' ? {x: v.x, y: 0} : along === 'y' ? {x: 0, y: v.y} : v
          const b = {x0: d.box.x0 + free.x, y0: d.box.y0 + free.y, x1: d.box.x1 + free.x, y1: d.box.y1 + free.y}
          const r = smartSnap(b, along === 'y' ? [] : boxXs(b), along === 'x' ? [] : boxYs(b), d.targets, tol(SMART_PX))
          if (r.dx !== null) delta.x = free.x + r.dx
          if (r.dy !== null) delta.y = free.y + r.dy
          state.smart = r.guides
          if (r.guides.length) state.snap = null
        }
        restoreBase(d.base)
        transformSelected([1, 0, 0, 1, delta.x, delta.y])
        ed.changed()
        return
      }
      case 'scale': {
        const fx = d.corner.x - d.fixed.x, fy = d.corner.y - d.fixed.y
        let at = raw
        // Smart guides: the dragged edge(s) onto other objects' edges / centres.
        state.smart = []
        if (!e.ctrlKey && d.targets) {
          const mx = Math.abs(fx) > 1e-9, my = Math.abs(fy) > 1e-9
          const b = {
            x0: mx ? Math.min(d.fixed.x, raw.x) : d.box.x0, x1: mx ? Math.max(d.fixed.x, raw.x) : d.box.x1,
            y0: my ? Math.min(d.fixed.y, raw.y) : d.box.y0, y1: my ? Math.max(d.fixed.y, raw.y) : d.box.y1,
          }
          const r = smartSnap(b, mx ? [raw.x] : [], my ? [raw.y] : [], d.targets, tol(SMART_PX))
          at = {x: raw.x + (r.dx ?? 0), y: raw.y + (r.dy ?? 0)}
          state.smart = r.guides
        }
        let sx = Math.abs(fx) > 1e-9 ? (at.x - d.fixed.x) / fx : 1
        let sy = Math.abs(fy) > 1e-9 ? (at.y - d.fixed.y) / fy : 1
        if (Math.abs(fx) < 1e-9) sx = e.shiftKey ? sy : 1
        if (Math.abs(fy) < 1e-9) sy = e.shiftKey ? sx : 1
        if (e.shiftKey && Math.abs(fx) > 1e-9 && Math.abs(fy) > 1e-9) { const k = Math.max(Math.abs(sx), Math.abs(sy)); sx = Math.sign(sx || 1) * k; sy = Math.sign(sy || 1) * k }
        if (Math.abs(sx) < 1e-3) sx = 1e-3
        if (Math.abs(sy) < 1e-3) sy = 1e-3
        // Option: from the reference point (the centre unless moved)
        const c = e.altKey ? pivotIn(d.box, state.pivot) : d.fixed
        restoreBase(d.base)
        transformSelected([sx, 0, 0, sy, c.x - sx * c.x, c.y - sy * c.y])
        ed.changed()
        return
      }
      case 'artboard': return artboardMove(d, raw, e)
      case 'pivot': {
        const b = d.box, w = b.x1 - b.x0, h = b.y1 - b.y0
        // near one of the nine points: onto it; else anywhere (also outside the box)
        const near = PIVOT_POINTS.find(q => dist(pivotIn(b, q), raw) < tol(8))
        setPivot(near ?? {u: w > 1e-9 ? (raw.x - b.x0) / w : 0.5, v: h > 1e-9 ? (raw.y - b.y0) / h : 0.5})
        return
      }
      case 'rotate': {
        let a = Math.atan2(raw.y - d.center.y, raw.x - d.center.x) - d.startAngle
        if (e.shiftKey) { const step = (ed.doc.angle_step || 15) * Math.PI / 180; a = Math.round(a / step) * step }
        const cos = Math.cos(a), sin = Math.sin(a), c = d.center
        restoreBase(d.base)
        transformSelected([cos, sin, -sin, cos, c.x - cos * c.x + sin * c.y, c.y - sin * c.x - cos * c.y])
        ed.changed()
        return
      }
      case 'anchors': {
        if (!d.moved && dist(raw, d.start) < tol(2)) return
        d.moved = true
        let target = add(d.origin, sub(raw, d.start))
        if (e.shiftKey) {
          const v = sub(raw, d.start)
          target = Math.abs(v.x) > Math.abs(v.y) ? add(d.origin, {x: v.x, y: 0}) : add(d.origin, {x: 0, y: v.y})
        } else {
          const free = target
          target = snap(target, e.ctrlKey)
          // Smart guides: the grabbed point onto other objects' edges / centres.
          state.smart = []
          if (!e.ctrlKey && d.targets) {
            const r = smartSnap({x0: free.x, y0: free.y, x1: free.x, y1: free.y}, [free.x], [free.y], d.targets, tol(SMART_PX))
            if (r.dx !== null) target = {...target, x: free.x + r.dx}
            if (r.dy !== null) target = {...target, y: free.y + r.dy}
            state.smart = r.guides
            if (r.guides.length) state.snap = null
          }
        }
        const delta = sub(target, d.origin)
        for (const a of ed.anchors) {
          const n = nodeOf(a)
          const b = d.base.get(akey(a))
          if (n && b) n.p = add(b, delta)
        }
        ed.changed()
        return
      }
      case 'handle': {
        const n = nodeOf(d.at)
        if (!n) return
        const p = e.shiftKey ? angleFrom(n.p, raw) : snap(raw, e.ctrlKey)
        const v = sub(p, n.p)
        if (e.altKey) n.smooth = false
        if (d.side === 'in') {
          n.incoming = v; n.exact_incoming = false
          if (n.smooth) { n.outgoing = scale(v, -len(n.outgoing) / Math.max(len(v), 1e-9)); n.exact_outgoing = false }
        } else {
          n.outgoing = v; n.exact_outgoing = false
          if (n.smooth) { n.incoming = scale(v, -len(n.incoming) / Math.max(len(v), 1e-9)); n.exact_incoming = false }
        }
        ed.changed()
        return
      }
      case 'pen': {
        const n = nodeOf(d.at)
        if (!n) return
        if (!d.moved && dist(raw, d.start) < tol(3)) return
        d.moved = true
        const v = e.shiftKey ? sub(angleFrom(n.p, raw), n.p) : sub(raw, n.p)
        if (d.mode === 'out') {
          // Redefine only the departing handle of the last anchor.
          n.outgoing = v; n.exact_outgoing = false; n.smooth = false
        } else if (d.mode === 'close') {
          n.incoming = scale(v, -1); n.exact_incoming = false
          if (!e.altKey) { n.outgoing = v; n.exact_outgoing = false; n.smooth = true } else n.smooth = false
        } else {
          n.outgoing = v; n.exact_outgoing = false
          if (!e.altKey) { n.incoming = scale(v, -1); n.exact_incoming = false; n.smooth = true } else n.smooth = false
        }
        ed.changed()
        return
      }
      case 'convert': {
        const n = nodeOf(d.at)
        if (!n) return
        if (!d.moved && dist(raw, d.start) < tol(3)) return
        d.moved = true
        const v = sub(raw, n.p)
        n.outgoing = v; n.incoming = scale(v, -1); n.smooth = true
        n.exact_incoming = n.exact_outgoing = false
        ed.changed()
        return
      }
      case 'width': {
        // The grip is drawn GRIP px beyond the edge: remove that offset so the
        // edge follows the pointer without a jump.
        const h = widthHandles(stroke(d.stroke)!).find(v => v.target === d.target)
        const back = h ? scale(h.dir, (d.side === 'left' ? 1 : -1) * GRIP / view.zoom) : ZERO
        const r = ed.coreEdit<{left: number; right: number}>('moveWidthHandle', {stroke: d.stroke, target: d.target, side: d.side, p: sub(raw, back), independent: e.altKey, base: d.base})
        if (r) ed.status = e.altKey
          ? `この側だけ：${d.side === 'left' ? '左' : '右'} ${readout(r[d.side])}（Optionを離すと両側）`
          : `両側に同じ量：左 ${readout(r.left)} ／ 右 ${readout(r.right)}（Optionでこの側だけ）`
        return
      }
      case 'corner': {
        const r = ed.coreEdit<number>('dragCorner', {stroke: d.stroke, node: d.node, p: raw, zoom: view.zoom})
        if (r !== undefined) ed.status = `角丸 半径 ${readout(r)}（Option＋クリックで種類を切替）`
        return
      }
      case 'widthSlide': {
        ed.coreEdit('slideWidth', {stroke: d.stroke, key: d.key, p: raw})
        return
      }
      case 'cap': {
        const v = sub(raw, d.anchor)
        if (len(v) < tol(4)) return
        let angle = Math.atan2(v.y, v.x) * 180 / Math.PI
        if (e.shiftKey) { const step = ed.doc.angle_step || 15; angle = Math.round(angle / step) * step }
        ed.coreEdit('capAngle', {stroke: d.stroke, end: d.end, angle})
        ed.status = `切り口 ${(((angle % 180) + 180) % 180).toFixed(1)}°（Shiftで${ed.doc.angle_step || 15}°刻み）`
        return
      }
    }
  }

  function hover(e: PointerEvent, local: Vec2, p: Vec2) {
    const h = state.hover
    state.snap = null
    state.snapAt = null
    h.segment = null
    h.anchor = null
    if (state.space) { h.cursor = 'grab'; return }
    if (local.x < RULER || local.y < RULER) { h.cursor = local.x < RULER ? 'col-resize' : 'row-resize'; return }
    if (ed.tool === 'artboard') {
      const hi = artboardHandleAt(p)
      h.cursor = hi >= 0 ? ['nwse-resize', 'ns-resize', 'nesw-resize', 'ew-resize'][hi % 4] : artboardAt(p) ? 'move' : 'crosshair'
      return
    }
    if (ed.tool === 'pen' || ed.tool === 'direct' || ed.tool === 'convert') {
      const wh = widthHandleAt(p)
      state.hoverGrip = wh?.side ? {stroke: wh.stroke, target: wh.target, side: wh.side} : null
      if (wh?.side && (e.metaKey || state.meta) && typeof wh.target === 'number') { h.cursor = 'ew-resize'; return }
      if (wh?.side) { h.cursor = e.altKey ? 'var(--width-one)' : 'var(--width)'; return }
      if (e.metaKey || state.meta) {
        if (wh) { h.cursor = 'grab'; return }
        const seg = segmentAt(p, 8)
        if (seg) { h.segment = {stroke: seg.stroke, point: seg.point}; h.cursor = 'copy'; return }
      }
    }
    if (ed.tool === 'pen') {
      if (state.penPath !== null) {
        const s = stroke(state.penPath)
        h.cursor = s && s.nodes.length > 1 && dist(s.nodes[0].p, p) < tol(8) ? 'var(--pen-close)' : 'var(--pen)'
        if (e.shiftKey) {
          const seg = segmentAt(p, 8, state.penPath)
          if (seg) { h.segment = {stroke: seg.stroke, point: seg.point}; h.cursor = 'var(--pen-on-path)'; h.pointer = seg.point; return }
        }
        // Preview where the next click lands (normal / tangent / grid; Shift: angle).
        const lastNode = s?.nodes.at(-1)
        h.pointer = e.shiftKey && lastNode ? angleFrom(lastNode.p, p) : hoverSnap(p, e.ctrlKey)
        return
      }
      const end = anchorAt(p, s => !s.closed)
      if (end) {
        const s = stroke(end.stroke)!
        if (s.nodes[0].id === end.node || s.nodes.at(-1)!.id === end.node) { h.anchor = end; h.cursor = 'var(--pen-continue)'; return }
      }
      const seg = segmentAt(p, e.shiftKey ? 8 : 5)
      if (seg) { h.segment = {stroke: seg.stroke, point: seg.point}; h.cursor = e.shiftKey ? 'var(--pen-on-path)' : 'var(--pen-add)'; return }
      h.cursor = 'var(--pen)'
      hoverSnap(p, e.ctrlKey)
      return
    }
    if (ed.tool === 'text') { h.cursor = 'text'; return }
    if (ed.tool === 'shape') { h.cursor = 'crosshair'; hoverSnap(p, e.ctrlKey); return }
    if (ed.tool === 'direct' || ed.tool === 'convert') {
      if (ed.tool === 'direct' && ed.selected.some(id => { const s = stroke(id); return s && !s.contours && cornerWidgets(s).some(w => dist(w.p, p) < tol(6)) })) {
        h.cursor = 'pointer'
        return
      }
      if (handleAt(p) || anchorAt(p)) { h.cursor = 'var(--direct-anchor)'; return }
      const seg = segmentAt(p)
      if (seg) {
        h.segment = {stroke: seg.stroke, point: seg.point}
        h.cursor = e.altKey ? 'var(--pen-add)' : 'var(--direct)'
        return
      }
      h.cursor = 'var(--direct)'
      return
    }
    const box = ed.selected.length ? selectionBounds() : null
    if (box) {
      const hs = boxHandles(box)
      const i = hs.findIndex(q => dist(q, p) < tol(6))
      const onPivot = dist(pivotIn(box, state.pivot), p) < tol(7)
      if (onPivot && e.metaKey) { h.cursor = 'crosshair'; return }
      if (i >= 0) { h.cursor = ['nwse-resize', 'ns-resize', 'nesw-resize', 'ew-resize'][i % 4]; return }
      if (onPivot && pathAt(p) === null) { h.cursor = 'crosshair'; return }
      if ([0, 2, 4, 6].some(i => dist(hs[i], p) < tol(20)) && !inBox(box, p)) { h.cursor = 'var(--rotate)'; return }
    }
    if (pathAt(p) !== null) { h.cursor = 'move'; return }
    h.cursor = guideAt(p) !== null ? 'grab' : 'default'
  }

  function up(e: PointerEvent, local: Vec2) {
    const d = state.drag
    state.drag = null
    state.snap = null
    state.smart = []
    if (!d) return
    switch (d.kind) {
      case 'pan': return
      case 'pivot': return
      case 'artboard': return artboardUp(d)
      case 'marquee': {
        const b: Bounds = {x0: Math.min(d.start.x, d.current.x), y0: Math.min(d.start.y, d.current.y), x1: Math.max(d.start.x, d.current.x), y1: Math.max(d.start.y, d.current.y)}
        if (b.x1 - b.x0 < tol(2) && b.y1 - b.y0 < tol(2)) return
        if (d.anchors) {
          for (const s of editable()) for (const n of s.nodes) {
            const a = {stroke: s.id, node: n.id}
            if (n.p.x >= b.x0 && n.p.x <= b.x1 && n.p.y >= b.y0 && n.p.y <= b.y1 && !ed.isAnchorSelected(a)) ed.selectAnchor(a, true)
          }
        } else {
          for (const s of ed.doc.strokes) {
            const r = ed.renders.get(s.id)
            if (!pickable(s) || !r) continue
            if (rectHitsRings(b, rings(r)) && !ed.selected.includes(s.id)) ed.selected = [...ed.selected, s.id]
          }
          // A compound path is taken whole, also when only its fill was touched.
          ed.selected = ed.withCompounds(ed.selected)
        }
        return
      }
      case 'guide': {
        if (local.x < RULER || local.y < RULER || local.x > view.width || local.y > view.height) {
          ed.doc.guides.splice(d.index, 1)
          ed.changed()
          ed.status = 'ガイドを削除しました'
        }
        ed.commit()
        return
      }
      case 'pen': {
        if (d.mode === 'close') { state.penPath = null; ed.status = 'パスを閉じました' }
        ed.commit()
        return
      }
      case 'shape': return shapeUp(d)
      case 'convert': {
        if (!d.moved) ed.coreEdit('anchorKind', {anchors: [d.at], anchorKind: 'corner'})
        ed.commit()
        return
      }
      default:
        ed.commit()
    }
  }

  function cancelDrag() {
    if (!state.drag) return
    if (state.drag.kind !== 'pan' && state.drag.kind !== 'marquee') ed.cancel()
    state.drag = null
    state.snap = null
    state.smart = []
  }

  function wheel(e: WheelEvent, local: Vec2) {
    // ⌘/Ctrl-wheel and trackpad pinch (ctrlKey) zoom; plain wheel follows the setting.
    if (e.ctrlKey || e.metaKey || settings.wheel === 'zoom') {
      zoomAt(local, wheelZoom(e.deltaY, e.ctrlKey))
    } else {
      view.panX -= e.deltaX
      view.panY -= e.deltaY
    }
  }
  function zoomAt(local: Vec2, factor: number) {
    const before = toWorld(local)
    view.zoom = Math.min(64, Math.max(0.05, view.zoom * factor))
    view.panX = local.x - before.x * view.zoom
    view.panY = local.y - before.y * view.zoom
  }
  /** Everything drawn and every artboard in view. */
  function fit() {
    let b: Bounds | null = null
    const grow = (sb: Bounds) => { b = b ? {x0: Math.min(b.x0, sb.x0), y0: Math.min(b.y0, sb.y0), x1: Math.max(b.x1, sb.x1), y1: Math.max(b.y1, sb.y1)} : sb }
    for (const s of ed.doc.strokes) {
      const r = ed.renders.get(s.id)
      const sb = r && ed.shown(s) ? renderBounds(r, s) : null
      if (sb) grow(sb)
    }
    for (const a of ed.doc.artboards ?? []) grow({x0: a.x, y0: a.y, x1: a.x + a.w, y1: a.y + a.h})
    fitBounds(b)
  }
  /** One artboard filling the view. */
  function showArtboard(id: number) {
    const a = ed.doc.artboards?.find(v => v.id === id)
    if (a) fitBounds({x0: a.x, y0: a.y, x1: a.x + a.w, y1: a.y + a.h})
  }
  function fitBounds(b: Bounds | null) {
    const w = view.width - RULER, h = view.height - RULER
    if (!b) { view.zoom = 1; view.panX = RULER + w / 2; view.panY = RULER + h / 2; return }
    const bw = Math.max(b.x1 - b.x0, 1), bh = Math.max(b.y1 - b.y0, 1)
    view.zoom = Math.min(16, Math.max(0.05, Math.min(w / bw, h / bh) * 0.85))
    view.panX = RULER + w / 2 - (b.x0 + bw / 2) * view.zoom
    view.panY = RULER + h / 2 - (b.y0 + bh / 2) * view.zoom
  }

  function nudge(dx: number, dy: number) {
    if (ed.anchors.length) {
      ed.edit(() => { for (const a of ed.anchors) { const n = nodeOf(a); if (n) n.p = add(n.p, {x: dx, y: dy}) } })
    } else if (ed.selected.length) {
      ed.edit(() => transformSelected([1, 0, 0, 1, dx, dy]))
    }
  }

  /** Canvas keyboard shortcuts. Returns true when handled. */
  function key(e: KeyboardEvent): boolean {
    const k = e.key.toLowerCase()
    const mod = e.metaKey || e.ctrlKey
    if (k === 'escape') {
      if (state.textEdit) { finishText(); return true }
      if (state.drag) { cancelDrag(); return true }
      if (state.widthTarget) { state.widthTarget = null; return true }
      if (state.penPath !== null) { finishPen(); return true }
      ed.clearSelection()
      return true
    }
    if (k === 'enter' && state.penPath !== null) { finishPen(); return true }
    // While drawing a polygon / star / spiral: ↑↓ change the count.
    if (state.drag?.kind === 'shape' && (k === 'arrowup' || k === 'arrowdown')) {
      const d = state.drag
      if (['polygon', 'star', 'spiral'].includes(d.params.kind)) {
        d.params = {...d.params, sides: Math.max(d.params.kind === 'polygon' ? 3 : 2, d.params.sides + (k === 'arrowup' ? 1 : -1))}
        d.strokes = applyShape(d.strokes, d.params)
        ed.changed()
      }
      return true
    }
    if (mod) {
      if (k === 'a') { ed.selected = ed.doc.strokes.filter(pickable).map(s => s.id); ed.anchors = []; return true }
      if (k === '0') { fit(); return true }
      if (k === '1') { zoomAt({x: view.width / 2, y: view.height / 2}, 1 / view.zoom); return true }
      if (k === '=' || k === '+') { zoomAt({x: view.width / 2, y: view.height / 2}, 1.25); return true }
      if (k === '-') { zoomAt({x: view.width / 2, y: view.height / 2}, 0.8); return true }
      if (k === 'd') { ed.duplicateSelection(); return true }
      return false
    }
    if (e.altKey) return false
    const tools: Record<string, typeof ed.tool> = {v: 'select', a: 'direct', p: 'pen', t: 'text'}
    if (k === 'c' && e.shiftKey) { setTool('convert'); return true }
    if (k === 'o' && e.shiftKey) { setTool('artboard'); return true }
    if ((k === 'delete' || k === 'backspace') && ed.tool === 'artboard') {
      if (state.artboard !== null) deleteArtboard(state.artboard)
      return true
    }
    const shapeKeys: Record<string, ShapeKind> = {m: 'rect', l: 'ellipse', '\\': 'line'}
    if (shapeKeys[k] && !e.shiftKey) { setShapeKind(shapeKeys[k]); return true }
    if (tools[k] && !e.shiftKey) { setTool(tools[k]); return true }
    if (k === 'f' || k === 'home') { fit(); return true }
    if (k === 'delete' || k === 'backspace') {
      if (state.widthTarget && typeof state.widthTarget.target === 'number') {
        ed.coreEdit('deleteWidth', {stroke: state.widthTarget.stroke, key: state.widthTarget.target})
        state.widthTarget = null
        return true
      }
      if (state.penPath !== null) finishPen()
      ed.deleteSelection()
      return true
    }
    const step = e.shiftKey ? 10 : 1
    const arrows: Record<string, [number, number]> = {arrowleft: [-step, 0], arrowright: [step, 0], arrowup: [0, -step], arrowdown: [0, step]}
    if (arrows[k]) { nudge(...arrows[k]); return true }
    return false
  }

  function setPaper(v: Paper) {
    settings.paper = v
  }
  function setShowWidth(v: boolean) {
    settings.showWidth = v
  }
  function setTool(t: typeof ed.tool) {
    if (t !== 'pen') finishPen()
    if (state.textEdit) finishText()
    ed.tool = t
    state.widthTarget = null
  }

  return reactive({
    view, state, toWorld, toScreen, down, move, up, wheel, zoomAt, fit, key, setTool, cancelDrag, setPaper,
    selectionBounds, boxHandles, widthHandles, widthGrips, finishPen, activePathFrame, cornerWidgets, widthVisible, setShowWidth, pathAt,
    editText, finishText, adjustShape, shapeAdjustable, setShapeKind, setPivot, deleteArtboard, artboardAt, showArtboard,
    arrangeUnits, keyUnit, alignBoard, align, distribute, strokeBounds,
  })
}
export type Canvas = ReturnType<typeof createCanvas>
export {affine}
