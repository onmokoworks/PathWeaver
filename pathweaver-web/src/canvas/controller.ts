// Pointer and keyboard behaviour of the artwork canvas.
// Follows Illustrator conventions where PathWeaver has the same concept:
//   Pen: click = corner, drag = smooth, Option-drag breaks the handle,
//        hovering a segment shows "+" and clicking adds an anchor,
//        clicking an open endpoint continues that path, first point closes.
//   Direct (A): anchors/handles/segments; Option-click or double-click a
//        segment adds an anchor; Option-drag a handle breaks the pair.
import {reactive, watch} from 'vue'
import {call} from '../core/wasm'
import {baselineTop, DEFAULT_FONT} from '../core/textOutline'
import {settings, wheelZoom, type Paper} from '../core/settings'
import type {Editor} from '../core/editor'
import type {Anchor, CornerWidget, Document, Frame, PathNode, SnapGuide, Stroke, Vec2, WidthHandle} from '../core/types'
import {add, affine, Bounds, dist, inside, len, rectHitsRings, renderBounds, rings, scale, sub, ZERO} from './geometry'

export interface View {panX: number; panY: number; zoom: number; width: number; height: number}
export const RULER = 20

type Matrix = [number, number, number, number, number, number]
type Drag =
  | {kind: 'pan'; last: Vec2}
  | {kind: 'marquee'; start: Vec2; current: Vec2; add: boolean; anchors: boolean}
  | {kind: 'move'; start: Vec2; origin: Vec2; base: string; moved: boolean; duplicate: boolean}
  | {kind: 'scale'; base: string; box: Bounds; corner: Vec2; fixed: Vec2}
  | {kind: 'rotate'; base: string; center: Vec2; startAngle: number}
  | {kind: 'anchors'; start: Vec2; origin: Vec2; base: Map<string, Vec2>; moved: boolean}
  | {kind: 'handle'; at: Anchor; side: 'in' | 'out'}
  | {kind: 'pen'; at: Anchor; start: Vec2; moved: boolean; mode: 'new' | 'out' | 'close'}
  | {kind: 'convert'; at: Anchor; start: Vec2; moved: boolean}
  | {kind: 'width'; stroke: number; target: WidthHandle['target']; side: 'left' | 'right'; base: {left: number; right: number}}
  | {kind: 'widthSlide'; stroke: number; key: number}
  | {kind: 'cap'; stroke: number; end: 'start' | 'end'; anchor: Vec2}
  | {kind: 'corner'; stroke: number; node: number}
  | {kind: 'guide'; index: number}

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
  })
  let frames: Frame[] = []
  let framesKey = ''  // what `frames` was captured for while hovering
  let lastClick = {t: 0, p: ZERO}

  // A replaced document (new / open / example) owns none of the old
  // interaction state: a half-drawn path must not continue into a path that
  // happens to share its id, and a drag must not apply to the new document.
  watch(() => ed.generation, () => {
    state.drag = null
    state.snap = null
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
  const pickable = (s: Stroke) => ed.shown(s) && !s.image?.reference

  const editable = () => ed.doc.strokes.filter(s => ed.shown(s) && !s.contours && !s.guide)
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
  function selectionBounds(): Bounds | null {
    let b: Bounds | null = null
    for (const id of ed.selected) {
      const s = stroke(id)
      const r = ed.renders.get(id)
      if (!s || !r) continue
      const sb = renderBounds(r, s)
      if (!sb) continue
      b = b ? {x0: Math.min(b.x0, sb.x0), y0: Math.min(b.y0, sb.y0), x1: Math.max(b.x1, sb.x1), y1: Math.max(b.y1, sb.y1)} : sb
    }
    return b
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
      case 'pen': return penDown(e, p)
      case 'direct': return directDown(e, p, double)
      case 'convert': return convertDown(e, p)
      default: return selectDown(e, p, double)
    }
  }

  // ---------- text ----------
  /** Click with the text tool: edit the text under the pointer, or start a new one there. */
  function textDown(p: Vec2) {
    const hit = pathAt(p)
    if (hit !== null && stroke(hit)?.text) return editText(hit)
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
      text: {text: '', font: DEFAULT_FONT, size, line_height: lh, matrix: [1, 0, 0, 1, p.x, top]},
    })
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
      for (let i = 0; i < hs.length; i++) {
        if (dist(hs[i], p) < tol(6)) {
          const opposite = hs[(i + 4) % 8]
          ed.begin()
          state.drag = {kind: 'scale', base: JSON.stringify(ed.doc.strokes), box, corner: hs[i], fixed: opposite}
          return
        }
      }
      // Just outside a corner: rotate.
      for (const i of [0, 2, 4, 6]) {
        const d = dist(hs[i], p)
        if (d >= tol(6) && d < tol(20) && !inBox(box, p)) {
          const center = {x: (box.x0 + box.x1) / 2, y: (box.y0 + box.y1) / 2}
          ed.begin()
          state.drag = {kind: 'rotate', base: JSON.stringify(ed.doc.strokes), center, startAngle: Math.atan2(p.y - center.y, p.x - center.x)}
          return
        }
      }
    }
    const hit = pathAt(p)
    if (hit !== null) {
      ed.selectPath(hit, e.shiftKey)
      if (!ed.selected.includes(hit)) return
      ed.begin()
      if (e.altKey) ed.duplicateSelection()
      const origin = nearestSelectedAnchor(p) ?? p
      captureFrames(null, () => false)
      state.drag = {kind: 'move', start: p, origin, base: JSON.stringify(ed.doc.strokes), moved: false, duplicate: e.altKey}
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
    state.drag = {kind: 'anchors', start: p, origin: {...nodeOf(grab)!.p}, base, moved: false}
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
    const seg = segmentAt(p, 8)
    if (seg) {
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
      case 'move': {
        if (!d.moved && dist(raw, d.start) < tol(2)) return
        d.moved = true
        let target = add(d.origin, sub(raw, d.start))
        if (e.shiftKey) {
          const v = sub(raw, d.start)
          target = Math.abs(v.x) > Math.abs(v.y) ? add(d.origin, {x: v.x, y: 0}) : add(d.origin, {x: 0, y: v.y})
        } else if (!e.ctrlKey && snapping()) {
          const r = call<{p: Vec2; guide: SnapGuide | null}>('snapTranslation', {doc: ed.doc, p: raw, start: d.start, origin: d.origin, frames, zoom: view.zoom})
          target = r.p
          state.snap = r.guide
        }
        const delta = sub(target, d.origin)
        restoreBase(d.base)
        transformSelected([1, 0, 0, 1, delta.x, delta.y])
        ed.changed()
        return
      }
      case 'scale': {
        const fx = d.corner.x - d.fixed.x, fy = d.corner.y - d.fixed.y
        let sx = Math.abs(fx) > 1e-9 ? (raw.x - d.fixed.x) / fx : 1
        let sy = Math.abs(fy) > 1e-9 ? (raw.y - d.fixed.y) / fy : 1
        if (Math.abs(fx) < 1e-9) sx = e.shiftKey ? sy : 1
        if (Math.abs(fy) < 1e-9) sy = e.shiftKey ? sx : 1
        if (e.shiftKey && Math.abs(fx) > 1e-9 && Math.abs(fy) > 1e-9) { const k = Math.max(Math.abs(sx), Math.abs(sy)); sx = Math.sign(sx || 1) * k; sy = Math.sign(sy || 1) * k }
        if (Math.abs(sx) < 1e-3) sx = 1e-3
        if (Math.abs(sy) < 1e-3) sy = 1e-3
        const c = e.altKey ? {x: (d.box.x0 + d.box.x1) / 2, y: (d.box.y0 + d.box.y1) / 2} : d.fixed
        restoreBase(d.base)
        transformSelected([sx, 0, 0, sy, c.x - sx * c.x, c.y - sy * c.y])
        ed.changed()
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
        } else target = snap(target, e.ctrlKey)
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
          ? `この側だけ：${d.side === 'left' ? '左' : '右'} ${r[d.side].toFixed(1)}（Optionを離すと両側）`
          : `両側に同じ量：左 ${r.left.toFixed(1)} ／ 右 ${r.right.toFixed(1)}（Optionでこの側だけ）`
        return
      }
      case 'corner': {
        const r = ed.coreEdit<number>('dragCorner', {stroke: d.stroke, node: d.node, p: raw, zoom: view.zoom})
        if (r !== undefined) ed.status = `角丸 半径 ${r.toFixed(1)}（Option＋クリックで種類を切替）`
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
      if (i >= 0) { h.cursor = ['nwse-resize', 'ns-resize', 'nesw-resize', 'ew-resize'][i % 4]; return }
      if ([0, 2, 4, 6].some(i => dist(hs[i], p) < tol(20)) && !inBox(box, p)) { h.cursor = 'var(--rotate)'; return }
    }
    if (pathAt(p) !== null) { h.cursor = 'move'; return }
    h.cursor = guideAt(p) !== null ? 'grab' : 'default'
  }

  function up(e: PointerEvent, local: Vec2) {
    const d = state.drag
    state.drag = null
    state.snap = null
    if (!d) return
    switch (d.kind) {
      case 'pan': return
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
  function fit() {
    let b: Bounds | null = null
    for (const s of ed.doc.strokes) {
      const r = ed.renders.get(s.id)
      const sb = r && ed.shown(s) ? renderBounds(r, s) : null
      if (sb) b = b ? {x0: Math.min(b.x0, sb.x0), y0: Math.min(b.y0, sb.y0), x1: Math.max(b.x1, sb.x1), y1: Math.max(b.y1, sb.y1)} : sb
    }
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
    editText, finishText,
  })
}
export type Canvas = ReturnType<typeof createCanvas>
export {affine}
