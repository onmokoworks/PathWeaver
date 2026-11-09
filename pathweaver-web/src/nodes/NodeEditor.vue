<script setup lang="ts">
// Blender-style node editor for the active path's appearance graph.
//  - values are edited directly on the nodes (Tweeq inputs)
//  - drag from a socket to connect; drag a linked input to detach/reconnect
//  - drop an unconnected node onto a link to insert it in between
//  - Shift+A / right click: add menu at the pointer; X / Delete: remove
import {computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch} from 'vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import {Icon} from '../../vendor/tweeq/src/Icon'
import type {Editor} from '../core/editor'
import {call} from '../core/wasm'
import type {Graph, GraphNode, NodeKind, ParamKey, Picture, Stroke, Vec2} from '../core/types'
import {loadPicture, pickImageFile} from './picture'
import PanelPin from '../panels/PanelPin.vue'
import {gestureZoom, settings, wheelZoom} from '../core/settings'
import {renderBounds} from '../canvas/geometry'
import NodeCard from './NodeCard.vue'
import AddMenu from './AddMenu.vue'
import LookPicker from './LookPicker.vue'
import {fitsLink, hasOutput, kindInfo, nodeInfo, outputsNumber} from './catalog'

const props = defineProps<{ed: Editor}>()
const ed = props.ed
const root = ref<HTMLDivElement>()
const layer = ref<HTMLDivElement>()
const view = reactive({panX: 40, panY: 30, zoom: 1})
const sockets = ref(new Map<string, Vec2>())
// `link`: a link dragged into empty space waits here for the node to attach.
type PendingLink = {from: number; fromNumber: boolean} | {reverse: {node: number; port: number | null; param: ParamKey | null}}
const menu = ref<{screen: Vec2; world: Vec2; link?: PendingLink} | null>(null)

type Drag =
  | {kind: 'pan'; last: Vec2}
  | {kind: 'move'; start: Vec2; origins: Map<number, [number, number]>; moved: boolean}
  | {kind: 'link'; from: number; fromNumber: boolean; pointer: Vec2; detachedFrom?: number}
  | {kind: 'box'; start: Vec2; current: Vec2; add: boolean}
  // Blender grab (G, or a node just added): follows the pointer with no button
  // held; click/Enter places it (inserting into a highlighted link), Esc /
  // right-click cancels (a new node is removed).
  | {kind: 'grab'; start: Vec2; origins: Map<number, [number, number]>; isNew: boolean; releaseConfirms?: boolean}
  // Node Wrangler Lazy Connect: Option-drag (or Alt+right-drag) node → node.
  | {kind: 'lazy'; from: number; pointer: Vec2; target: number | null}
  // Blender Cut Links: Ctrl+right-drag (or Ctrl-drag on empty space) across links.
  // Ctrl-drag on a node detaches it instead (then it can be dropped on another link).
  | {kind: 'cut'; points: Vec2[]}
  | {kind: 'right'; start: Vec2; node: number | null; alt: boolean}
const drag = ref<Drag | null>(null)
const insertTarget = ref<Link | null>(null)

const stroke = computed<Stroke | undefined>(() => ed.active)
const graph = computed<Graph | null>(() => {
  void ed.revision
  return stroke.value?.appearance ?? null
})
const activeNode = computed(() => ed.selectedNodes[ed.selectedNodes.length - 1])

// Effective values of connected parameters, computed by the same evaluator as rendering.
const effective = computed(() => {
  void ed.revision
  const m = new Map<string, number>()
  const s = stroke.value
  if (!s?.appearance) return m
  try {
    for (const v of call<{node: number; key: ParamKey; value: number}[]>('parameterValues', {stroke: s})) m.set(`${v.node}:${v.key}`, v.value)
  } catch { /* shown as 計算不可 */ }
  return m
})

interface Link {from: number; to: number; port: number | null; param: ParamKey | null; number: boolean}
const links = computed<Link[]>(() => {
  void ed.revision // the graph is edited in place; identity alone never changes
  const g = graph.value
  if (!g) return []
  const out: Link[] = []
  for (const n of g.nodes) {
    n.inputs.forEach((src, port) => { if (src !== null) out.push({from: src, to: n.id, port, param: null, number: false}) })
    for (const [param, src] of n.bindings) out.push({from: src, to: n.id, port: null, param, number: true})
  }
  return out
})
const linkKey = (l: Link) => `${l.from}>${l.to}:${l.port ?? l.param}`
const socketOf = (l: Link) => l.param ? `param:${l.to}:${l.param}` : `in:${l.to}:${l.port}`
function curve(a: Vec2, b: Vec2) {
  const dx = Math.max(40, Math.abs(b.x - a.x) * 0.5)
  return `M${a.x} ${a.y}C${a.x + dx} ${a.y} ${b.x - dx} ${b.y} ${b.x} ${b.y}`
}
const wires = computed(() => links.value.flatMap(l => {
  const a = sockets.value.get(`out:${l.from}`), b = sockets.value.get(socketOf(l))
  return a && b ? [{key: linkKey(l), d: curve(a, b), link: l, a, b}] : []
}))
const outputLinked = computed(() => new Set(links.value.map(l => l.from)))

// ---- coordinates ----
const localOf = (e: {clientX: number; clientY: number}): Vec2 => {
  const r = root.value!.getBoundingClientRect()
  return {x: e.clientX - r.left, y: e.clientY - r.top}
}
const toWorld = (p: Vec2): Vec2 => ({x: (p.x - view.panX) / view.zoom, y: (p.y - view.panY) / view.zoom})

/** Measure socket centres in graph coordinates after layout. */
function measure() {
  if (!layer.value) return
  const base = layer.value.getBoundingClientRect()
  const m = new Map<string, Vec2>()
  for (const el of layer.value.querySelectorAll<HTMLElement>('[data-socket]')) {
    const r = el.getBoundingClientRect()
    m.set(el.dataset.socket!, {x: (r.left + r.width / 2 - base.left) / view.zoom, y: (r.top + r.height / 2 - base.top) / view.zoom})
  }
  sockets.value = m
}
watch(() => [ed.revision, graph.value, view.zoom], () => nextTick(measure), {flush: 'post'})
let resize: ResizeObserver | null = null
onMounted(() => {
  nextTick(measure)
  resize = new ResizeObserver(() => measure())
  resize.observe(root.value!)
})
onBeforeUnmount(() => resize?.disconnect())

// ---- graph mutation ----
function mutate(fn: (g: Graph) => void) {
  ed.edit(() => {
    const g = stroke.value?.appearance
    if (g) fn(g)
  })
}
function replaceGraph(g: Graph) {
  const s = stroke.value
  if (!s) return
  s.appearance = g
  ed.changed()
}
function node(id: number) {
  return graph.value?.nodes.find(n => n.id === id)
}
/** Start a path's appearance from the node you want, not from an empty
 *  graph: Input → (picked node) → Output, as one undo step. */
function startWith(kind: NodeKind) {
  const s = stroke.value
  if (!s) return
  menu.value = null
  const g = call<Graph>('graphDefault')
  const input = g.nodes.find(n => n.kind === 'Input')!
  const output = g.nodes.find(n => n.kind === 'Output')!
  const id = Math.max(...g.nodes.map(n => n.id)) + 1
  const n = call<GraphNode>('nodeNew', {kind, id, position: [Math.round((input.position[0] + output.position[0]) / 2 - 100), input.position[1]]})
  g.nodes.push(n)
  if (!kindInfo(kind).number) {
    // a filter goes between Input and Output; a source (画像) feeds Output
    const before = output.inputs[0]
    if (n.inputs.length) n.inputs[0] = input.id
    output.inputs[0] = id
    try { call('graphValidate', {graph: g}) } catch { output.inputs[0] = before; if (n.inputs.length) n.inputs[0] = null }
  }
  ed.edit(() => { s.appearance = g })
  ed.selectedNodes = [id]
  ed.status = `「${nodeInfo(n).label}」から外観ノードを始めました`
  nextTick(() => { measure(); fit() })
}
function onPick(kind: NodeKind) {
  const m = menu.value
  if (!m) return
  if (!graph.value) startWith(kind)
  else addNode(kind, m.world, m.screen, {link: m.link})
}
/** The same add menu, opened from a button (header / empty state). */
function openMenuAt(e: MouseEvent) {
  const r = root.value!.getBoundingClientRect()
  const b = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const screen = {x: b.left - r.left, y: b.bottom - r.top + 4}
  menu.value = {screen, world: toWorld(screen)}
}

// Parameter edits: one gesture per focus → commit.
function onParam(n: GraphNode, key: ParamKey, value: number) {
  ed.begin()
  const picture = n.picture
  const next = call<GraphNode>('nodeSetNumber', {node: {...n, picture: null}, key, value})
  Object.assign(n, next, {picture})
  ed.changed()
}

// ---- 画像 node: choose / drop a picture (embedded in the document) ----
/** Put a picture into a node, placed over the path's bounds (aspect kept). */
function setPicture(n: GraphNode, pic: Picture) {
  const s = stroke.value
  const r = s && ed.renders.get(s.id)
  const b = r && s ? renderBounds(r, s) : null
  const width = Math.min(1000, Math.max(1, b ? b.x1 - b.x0 : 200))
  ed.edit(() => {
    n.picture = pic
    n.translation = b ? [Math.round(b.x0), Math.round(b.y0)] : [0, 0]
    n.amount = Math.round(width * 100) / 100
  })
  ed.status = `画像「${pic.name}」を読み込みました（${pic.width}×${pic.height}px、書類に埋め込み）`
}
async function choosePicture(n: GraphNode) {
  const file = await pickImageFile()
  if (!file) return
  try { setPicture(n, await loadPicture(file)) } catch (err) { ed.error = err instanceof Error ? err.message : String(err) }
}
function onDragOver(e: DragEvent) {
  if (graph.value && [...(e.dataTransfer?.items ?? [])].some(i => i.kind === 'file')) { e.preventDefault(); e.dataTransfer!.dropEffect = 'copy' }
}
/** Dropping an image file on the node editor adds a 画像 node there (or fills the one under it). */
async function onDrop(e: DragEvent) {
  const file = [...(e.dataTransfer?.files ?? [])].find(f => f.type.startsWith('image/'))
  if (!file || !graph.value) return
  e.preventDefault()
  let pic: Picture
  try { pic = await loadPicture(file) } catch (err) { ed.error = err instanceof Error ? err.message : String(err); return }
  const under = (e.target as HTMLElement).closest<HTMLElement>('[data-node]')
  const existing = under ? node(+under.dataset.node!) : undefined
  if (existing?.kind === 'Image') { setPicture(existing, pic); return }
  const g = graph.value
  const at = toWorld(localOf(e))
  const id = Math.max(0, ...g.nodes.map(v => v.id)) + 1
  ed.begin()
  const n = call<GraphNode>('nodeNew', {kind: 'Image', id, position: [Math.round(at.x - 100), Math.round(at.y - 12)]})
  g.nodes.push(n)
  ed.selectedNodes = [id]
  setPicture(g.nodes[g.nodes.length - 1], pic)
  ed.commit()
}
function onSet(n: GraphNode, patch: (n: GraphNode) => void) {
  ed.begin()
  patch(n)
  ed.changed()
}
function onCommit() { ed.commit() }
function toggleBypass(n: GraphNode) { mutate(() => { n.bypass = !n.bypass }) }

// ---- pointer ----
function onDown(e: PointerEvent) {
  const target = e.target as HTMLElement
  const p = localOf(e)
  menu.value = null
  if (e.button === 1 || (e.button === 0 && spaceHeld)) {
    drag.value = {kind: 'pan', last: p}
    capture(e)
    return
  }
  const g0 = drag.value
  if (g0?.kind === 'grab') {
    if (e.button === 0) confirmGrab()
    else cancelGrab()
    e.preventDefault()
    return
  }
  if (!graph.value) return
  const onNode = target.closest<HTMLElement>('[data-node]')
  if (e.ctrlKey && e.button === 0 && onNode && !target.closest('[data-socket], [data-nodrag], input, button')) {
    // Ctrl-drag a node: pull it out of its links (the chain closes behind it).
    const id = +onNode.dataset.node!
    selectNode(id, false)
    ed.begin()
    detachNode(id)
    drag.value = {kind: 'move', start: toWorld(p), origins: new Map([[id, [...node(id)!.position] as [number, number]]]), moved: false}
    capture(e)
    e.preventDefault()
    return
  }
  if (e.ctrlKey && (e.button === 0 || e.button === 2)) {
    drag.value = {kind: 'cut', points: [toWorld(p)]}
    capture(e)
    e.preventDefault()
    return
  }
  if (e.button === 2) {
    // Decided on move: Alt = lazy connect from the node; otherwise the add menu on release.
    drag.value = {kind: 'right', start: p, node: onNode ? +onNode.dataset.node! : null, alt: e.altKey}
    capture(e)
    return
  }
  if (e.button !== 0) return
  if (e.altKey && onNode && !target.closest('[data-socket], [data-nodrag], input, button')) {
    const from = +onNode.dataset.node!
    if (hasOutput(node(from)!.kind)) {
      drag.value = {kind: 'lazy', from, pointer: toWorld(p), target: null}
      capture(e)
      e.preventDefault()
      return
    }
  }
  const socket = target.closest<HTMLElement>('[data-socket]')
  if (socket) {
    startLink(socket.dataset.socket!, p)
    capture(e)
    e.preventDefault()
    return
  }
  // Controls inside a node keep their own pointer behaviour.
  if (target.closest('[data-nodrag], input, button, .TqInputNumber, .TqInputColor, .TqInputDropdown, .TqInputCheckbox')) {
    const el = target.closest<HTMLElement>('[data-node]')
    if (el) selectNode(+el.dataset.node!, false, true)
    return
  }
  const el = target.closest<HTMLElement>('[data-node]')
  if (el) {
    const id = +el.dataset.node!
    if (e.shiftKey && ed.selectedNodes.includes(id) && activeNode.value === id) {
      ed.selectedNodes = ed.selectedNodes.filter(v => v !== id)
      return
    }
    selectNode(id, e.shiftKey)
    ed.begin()
    const origins = new Map<number, [number, number]>()
    for (const sid of ed.selectedNodes) { const n = node(sid); if (n) origins.set(sid, [...n.position] as [number, number]) }
    drag.value = {kind: 'move', start: toWorld(p), origins, moved: false}
    capture(e)
    e.preventDefault()
    return
  }
  if (!e.shiftKey) ed.selectedNodes = []
  drag.value = {kind: 'box', start: toWorld(p), current: toWorld(p), add: e.shiftKey}
  capture(e)
}
function capture(e: PointerEvent) { root.value!.setPointerCapture(e.pointerId) }
function selectNode(id: number, add: boolean, keep = false) {
  if (add) ed.selectedNodes = [...ed.selectedNodes.filter(v => v !== id), id]
  else if (keep && ed.selectedNodes.includes(id)) ed.selectedNodes = [...ed.selectedNodes.filter(v => v !== id), id]
  else if (!ed.selectedNodes.includes(id)) ed.selectedNodes = [id]
  else ed.selectedNodes = [...ed.selectedNodes.filter(v => v !== id), id]
}

function startLink(id: string, p: Vec2) {
  const [kind, a, b] = id.split(':')
  const nodeId = +a
  if (kind === 'out') {
    drag.value = {kind: 'link', from: nodeId, fromNumber: outputsNumber(node(nodeId)!.kind), pointer: toWorld(p)}
    return
  }
  // Dragging a linked input picks the link up from its source.
  const n = node(nodeId)!
  let src: number | null = null
  ed.begin()
  if (kind === 'in') {
    src = n.inputs[+b]
    if (src !== null) { n.inputs[+b] = null; ed.changed() }
  } else {
    const binding = n.bindings.find(v => v[0] === b)
    if (binding) { src = binding[1]; n.bindings = n.bindings.filter(v => v[0] !== b); ed.changed() }
  }
  if (src === null) {
    // Empty input: drag out a new link backwards (to an output).
    drag.value = {kind: 'link', from: -nodeId, fromNumber: kind === 'param', pointer: toWorld(p)}
    reverseTarget = {node: nodeId, port: kind === 'in' ? +b : null, param: kind === 'param' ? b as ParamKey : null}
    return
  }
  drag.value = {kind: 'link', from: src, fromNumber: kind === 'param', pointer: toWorld(p), detachedFrom: nodeId}
}
let reverseTarget: {node: number; port: number | null; param: ParamKey | null} | null = null

function onMove(e: PointerEvent) {
  const p = localOf(e)
  const d = drag.value
  if (!d) return
  if (d.kind === 'grab') {
    const w = toWorld(p)
    const dx = w.x - d.start.x, dy = w.y - d.start.y
    for (const [id, o] of d.origins) { const n = node(id); if (n) n.position = [Math.round(o[0] + dx), Math.round(o[1] + dy)] }
    ed.changed()
    insertTarget.value = d.origins.size === 1 ? insertCandidate([...d.origins.keys()][0], w) : null
    return
  }
  if (d.kind === 'right') {
    if (Math.hypot(p.x - d.start.x, p.y - d.start.y) < 5) return
    if ((d.alt || e.altKey) && d.node !== null && hasOutput(node(d.node)!.kind)) drag.value = {kind: 'lazy', from: d.node, pointer: toWorld(p), target: null}
    return
  }
  if (d.kind === 'cut') { d.points.push(toWorld(p)); return }
  if (d.kind === 'lazy') {
    d.pointer = toWorld(p)
    const el = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>('[data-node]')
    const id = el ? +el.dataset.node! : null
    d.target = id !== null && id !== d.from ? id : null
    return
  }
  if (d.kind === 'pan') {
    view.panX += p.x - d.last.x
    view.panY += p.y - d.last.y
    d.last = p
    return
  }
  const w = toWorld(p)
  if (d.kind === 'link') { d.pointer = w; return }
  if (d.kind === 'box') { d.current = w; return }
  if (d.kind === 'move') {
    const dx = w.x - d.start.x, dy = w.y - d.start.y
    if (!d.moved && Math.hypot(dx, dy) * view.zoom < 3) return
    d.moved = true
    for (const [id, o] of d.origins) {
      const n = node(id)
      if (n) n.position = [o[0] + dx, o[1] + dy]
    }
    ed.changed()
    insertTarget.value = d.origins.size === 1 ? insertCandidate([...d.origins.keys()][0], w) : null
  }
}

/** A free node whose box lies over a compatible link can be inserted into it. */
function insertCandidate(id: number, p: Vec2): Link | null {
  const n = node(id)
  if (!n || links.value.some(l => l.from === id || l.to === id)) return null
  const shapeThrough = n.inputs.length > 0 && hasOutput(n.kind) && !outputsNumber(n.kind)
  const numberThrough = n.kind === 'Math'
  const el = layer.value?.querySelector<HTMLElement>(`[data-node="${id}"]`)
  const w = el?.offsetWidth ?? 200, h = el?.offsetHeight ?? 60
  const pad = 6 / view.zoom
  const box = {x0: n.position[0] - pad, y0: n.position[1] - pad, x1: n.position[0] + w + pad, y1: n.position[1] + h + pad}
  const centre = {x: n.position[0] + w / 2, y: n.position[1] + h / 2}
  let best: Link | null = null
  let bestD = Infinity
  for (const wire of wires.value) {
    if (wire.link.number ? !numberThrough : !shapeThrough) continue
    // The wire passes under the node box (or right under the pointer).
    const under = curveSamples(wire.a, wire.b).some(q => q.x > box.x0 && q.x < box.x1 && q.y > box.y0 && q.y < box.y1)
    const nearPointer = distanceToCurve(wire.a, wire.b, p) < 14 / view.zoom
    if (!under && !nearPointer) continue
    const d = distanceToCurve(wire.a, wire.b, centre)
    if (d < bestD) { bestD = d; best = wire.link }
  }
  return best
}
function curveSamples(a: Vec2, b: Vec2, steps = 40): Vec2[] {
  const dx = Math.max(40, Math.abs(b.x - a.x) * 0.5)
  const c1 = {x: a.x + dx, y: a.y}, c2 = {x: b.x - dx, y: b.y}
  const out: Vec2[] = []
  // skip the ends: they sit on the sockets of the linked nodes
  for (let i = 2; i <= steps - 2; i++) {
    const t = i / steps, u = 1 - t
    out.push({
      x: u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x,
      y: u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y,
    })
  }
  return out
}
function distanceToCurve(a: Vec2, b: Vec2, p: Vec2) {
  const dx = Math.max(40, Math.abs(b.x - a.x) * 0.5)
  const c1 = {x: a.x + dx, y: a.y}, c2 = {x: b.x - dx, y: b.y}
  let best = Infinity
  for (let i = 0; i <= 40; i++) {
    const t = i / 40, u = 1 - t
    const x = u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x
    const y = u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y
    best = Math.min(best, Math.hypot(x - p.x, y - p.y))
  }
  return best
}

function onUp(e: PointerEvent) {
  const d = drag.value
  drag.value = null
  if (!d) return
  if (root.value!.hasPointerCapture(e.pointerId)) root.value!.releasePointerCapture(e.pointerId)
  if (d.kind === 'grab') {
    // dragged out of the add menu: releasing places it; otherwise the next click does
    drag.value = d
    if (d.releaseConfirms) confirmGrab()
    return
  }
  if (d.kind === 'pan') return
  if (d.kind === 'right') { menu.value = {screen: d.start, world: toWorld(d.start)}; return }
  if (d.kind === 'cut') { cutLinks(d.points); return }
  if (d.kind === 'lazy') { if (d.target !== null) lazyConnect(d.from, d.target); return }
  if (d.kind === 'box') {
    const x0 = Math.min(d.start.x, d.current.x), x1 = Math.max(d.start.x, d.current.x)
    const y0 = Math.min(d.start.y, d.current.y), y1 = Math.max(d.start.y, d.current.y)
    const hits = (graph.value?.nodes ?? []).filter(n => {
      const el = layer.value?.querySelector<HTMLElement>(`[data-node="${n.id}"]`)
      const w = el ? el.offsetWidth : 200, h = el ? el.offsetHeight : 60
      return n.position[0] < x1 && n.position[0] + w > x0 && n.position[1] < y1 && n.position[1] + h > y0
    }).map(n => n.id)
    ed.selectedNodes = d.add ? [...new Set([...ed.selectedNodes, ...hits])] : hits
    return
  }
  if (d.kind === 'move') {
    const target = insertTarget.value
    insertTarget.value = null
    if (target && d.moved) insertNode([...d.origins.keys()][0], target)
    ed.commit()
    return
  }
  if (d.kind === 'link') {
    const hit = document.elementFromPoint(e.clientX, e.clientY)
    const el = hit?.closest<HTMLElement>('[data-socket]')
    const onNode = hit?.closest<HTMLElement>('[data-node]')
    finishLink(d, el?.dataset.socket ?? nearSocket(d, toWorld(localOf(e))), onNode ? +onNode.dataset.node! : nearNode(toWorld(localOf(e))), localOf(e))
  }
}

/** Closest compatible socket within 22 screen px (fuzzy drop). */
function nearSocket(d: Extract<Drag, {kind: 'link'}>, p: Vec2): string | null {
  let best: string | null = null
  let bestD = 22 / view.zoom
  for (const [id, q] of sockets.value) {
    const out = id.startsWith('out:')
    if (d.from < 0 ? !out : out) continue // reverse drags end on outputs, normal ones on inputs
    if (d.from >= 0 && +id.split(':')[1] === d.from) continue
    if (d.from >= 0 && d.fromNumber !== id.startsWith('param:')) continue
    const dist = Math.hypot(q.x - p.x, q.y - p.y)
    if (dist < bestD) { bestD = dist; best = id }
  }
  return best
}
/** A node whose box is within 30 screen px of the drop point. */
function nearNode(p: Vec2): number | null {
  const m = 30 / view.zoom
  for (const n of [...(graph.value?.nodes ?? [])].reverse()) {
    const el = layer.value?.querySelector<HTMLElement>(`[data-node="${n.id}"]`)
    const w = el?.offsetWidth ?? 200, h = el?.offsetHeight ?? 60
    if (p.x > n.position[0] - m && p.x < n.position[0] + w + m && p.y > n.position[1] - m && p.y < n.position[1] + h + m) return n.id
  }
  return null
}

function finishLink(d: Extract<Drag, {kind: 'link'}>, socket: string | null, onNode: number | null = null, screen?: Vec2) {
  const g = graph.value
  if (!g) return
  try {
    if (d.from < 0) {
      // Started from an empty input: the drop is an output, or any node that has one.
      const t = reverseTarget!
      reverseTarget = null
      let from = socket?.startsWith('out:') ? +socket.split(':')[1] : onNode
      if (from !== null && (from === t.node || !hasOutput(node(from)!.kind))) from = null
      if (from === null) {
        // Empty space: offer the add menu; the new node feeds this input.
        if (screen) menu.value = {screen, world: toWorld(screen), link: {reverse: t}}
        ed.commit()
        return
      }
      replaceGraph(call<Graph>('graphConnect', {graph: g, from, to: t.node, port: t.port ?? 0, param: t.param}))
    } else if (socket && !socket.startsWith('out:')) {
      const [kind, to, x] = socket.split(':')
      replaceGraph(call<Graph>('graphConnect', {
        graph: g, from: d.from, to: +to,
        port: kind === 'in' ? +x : 0, param: kind === 'param' ? x : null,
      }))
    } else if (onNode !== null && onNode !== d.from && onNode !== d.detachedFrom) {
      // Dropped on (or next to) a node: connect to its best matching socket.
      ed.commit()
      lazyConnect(d.from, onNode)
      return
    } else if (screen && d.detachedFrom === undefined) {
      // Empty space: open the add menu; the chosen node attaches to this link.
      // (A link pulled off an input and dropped in empty space is just cut.)
      menu.value = {screen, world: toWorld(screen), link: {from: d.from, fromNumber: d.fromNumber}}
    }
    ed.error = ''
  } catch (err) {
    ed.error = err instanceof Error ? err.message : String(err)
  }
  ed.commit()
}

function insertNode(id: number, l: Link) {
  const g = graph.value!
  const n = node(id)!
  const to = node(l.to)!
  try {
    if (l.number) {
      to.bindings = to.bindings.map(b => (b[0] === l.param ? [b[0], id] : b))
      n.bindings = [...n.bindings.filter(b => b[0] !== 'A'), ['A', l.from]]
    } else {
      to.inputs[l.port!] = id
      n.inputs[0] = l.from
    }
    call('graphValidate', {graph: g})
    makeRoom(id, l.to)
    ed.changed()
    ed.status = 'リンクの間にノードを挿入しました'
  } catch (err) {
    ed.cancel()
    ed.error = err instanceof Error ? err.message : String(err)
  }
}

/** After an insert, shift the downstream chain right so nothing overlaps. */
function makeRoom(inserted: number, downstream: number) {
  const g = graph.value!
  const width = (id: number) => layer.value?.querySelector<HTMLElement>(`[data-node="${id}"]`)?.offsetWidth ?? 200
  const n = node(inserted)!
  const target = node(downstream)!
  const need = n.position[0] + width(inserted) + 40 - target.position[0]
  if (need <= 0) return
  const move = new Set<number>()
  const visit = (id: number) => {
    if (move.has(id)) return
    move.add(id)
    for (const m of g.nodes) if (m.inputs.includes(id) || m.bindings.some(b => b[1] === id)) visit(m.id)
  }
  visit(downstream)
  for (const id of move) { const m = node(id); if (m) m.position = [m.position[0] + need, m.position[1]] }
}

// ---- Node Wrangler / Blender operators ----
function startGrab(ids: number[], pointer: Vec2, isNew: boolean, releaseConfirms = false) {
  const origins = new Map<number, [number, number]>()
  for (const id of ids) { const n = node(id); if (n) origins.set(id, [...n.position] as [number, number]) }
  if (!origins.size) return
  ed.begin()
  drag.value = {kind: 'grab', start: toWorld(pointer), origins, isNew, releaseConfirms}
  ed.status = isNew ? 'クリックで配置（リンクの上なら割り込み）・Escで取消' : 'クリック／Enterで確定・Esc／右クリックで取消'
}
function confirmGrab() {
  const d = drag.value
  if (d?.kind !== 'grab') return
  drag.value = null
  const target = insertTarget.value
  insertTarget.value = null
  if (target) insertNode([...d.origins.keys()][0], target)
  ed.commit()
  ed.status = ''
}
function cancelGrab() {
  if (drag.value?.kind !== 'grab') return
  const isNew = drag.value.isNew
  drag.value = null
  insertTarget.value = null
  ed.cancel() // restores positions; a just-added node disappears
  if (isNew) ed.selectedNodes = []
  ed.status = ''
}

/** Connect the best matching sockets of two nodes (Lazy Connect). */
function lazyConnect(from: number, to: number) {
  const g = graph.value
  const a = node(from), b = node(to)
  if (!g || !a || !b) return
  try {
    if (outputsNumber(a.kind)) {
      const params = nodeInfo(b).params.map(p => p.key)
      if (!params.length) throw new Error(`「${nodeInfo(b).label}」には数値の入力がありません`)
      const key = params.find(k => !b.bindings.some(x => x[0] === k)) ?? params[0]
      ed.edit(() => replaceGraph(call<Graph>('graphConnect', {graph: g, from, to, port: 0, param: key})))
    } else {
      if (!b.inputs.length) throw new Error(`「${nodeInfo(b).label}」には形の入力がありません`)
      const port = Math.max(0, b.inputs.findIndex(v => v === null))
      ed.edit(() => replaceGraph(call<Graph>('graphConnect', {graph: g, from, to, port, param: null})))
    }
    ed.status = `「${nodeInfo(a).label}」→「${nodeInfo(b).label}」をつなぎました`
  } catch (err) {
    ed.error = err instanceof Error ? err.message : String(err)
  }
}

/** Remove every link crossed by the drawn stroke (Cut Links). */
function cutLinks(points: Vec2[]) {
  if (points.length < 2) return
  const crossing = (a: Vec2, b: Vec2, c: Vec2, d: Vec2) => {
    const r = (p: Vec2, q: Vec2, s: Vec2) => (q.x - p.x) * (s.y - p.y) - (q.y - p.y) * (s.x - p.x)
    return r(a, b, c) * r(a, b, d) < 0 && r(c, d, a) * r(c, d, b) < 0
  }
  const cut = wires.value.filter(w => {
    const pts = curvePoints(w.a, w.b)
    for (let i = 1; i < pts.length; i++) for (let j = 1; j < points.length; j++) if (crossing(pts[i - 1], pts[i], points[j - 1], points[j])) return true
    return false
  }).map(w => w.link)
  if (!cut.length) return
  mutate(() => {
    for (const l of cut) {
      const n = node(l.to)
      if (!n) continue
      if (l.param) n.bindings = n.bindings.filter(b => b[0] !== l.param)
      else n.inputs[l.port!] = null
    }
  })
  ed.status = `リンクを${cut.length}本切りました`
}
function curvePoints(a: Vec2, b: Vec2) {
  const dx = Math.max(40, Math.abs(b.x - a.x) * 0.5)
  const c1 = {x: a.x + dx, y: a.y}, c2 = {x: b.x - dx, y: b.y}
  return Array.from({length: 33}, (_, i) => {
    const t = i / 32, u = 1 - t
    return {x: u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x, y: u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y}
  })
}

/** Unplug a node, reconnecting what fed it to what it fed (Blender Alt-drag). */
function detachNode(id: number) {
  const g = graph.value
  const n = node(id)
  if (!g || !n || n.kind === 'Input' || n.kind === 'Output') return
  const through = n.inputs[0] ?? null
  const numberThrough = n.bindings.find(b => b[0] === 'A')?.[1] ?? null
  let changed = n.inputs.some(v => v !== null) || n.bindings.length > 0
  for (const m of g.nodes) {
    if (m.id === id) continue
    if (m.inputs.includes(id)) { m.inputs = m.inputs.map(v => (v === id ? through : v)); changed = true }
    if (m.bindings.some(b => b[1] === id)) {
      m.bindings = m.bindings.flatMap(b => (b[1] !== id ? [b] : numberThrough !== null ? [[b[0], numberThrough] as [ParamKey, number]] : []))
      changed = true
    }
  }
  n.inputs = n.inputs.map(() => null)
  n.bindings = []
  if (!changed) return
  try {
    call('graphValidate', {graph: g})
    ed.status = 'ノードをリンクから外しました'
  } catch (err) {
    ed.error = err instanceof Error ? err.message : String(err)
  }
  ed.changed()
}
const isShape = (id: number) => { const k = node(id)?.kind; return !!k && hasOutput(k) && !outputsNumber(k) }

/** Delete selected nodes but keep the chain connected (Blender Ctrl+X). */
function dissolveSelected() {
  const g = graph.value
  if (!g) return
  const ids = ed.selectedNodes.filter(id => { const k = node(id)?.kind; return k && k !== 'Input' && k !== 'Output' })
  if (!ids.length) return
  ed.edit(() => {
    for (const id of ids) {
      const n = node(id)
      const through = n?.inputs[0] ?? null
      for (const m of g.nodes) {
        m.inputs = m.inputs.map(v => (v === id ? through : v))
      }
    }
    replaceGraph(call<Graph>('graphRemove', {graph: g, ids}))
  })
  ed.selectedNodes = []
  ed.status = 'つなぎを保ったまま削除しました'
}

// ---- add / delete ----
function addNode(kind: NodeKind, at: Vec2, screen: Vec2, opts: {link?: PendingLink; dragged?: boolean} = {}) {
  const g = graph.value
  if (!g) return
  menu.value = null
  ed.begin()
  const id = Math.max(0, ...g.nodes.map(n => n.id)) + 1
  // Header centre under the pointer, then it follows the pointer (Blender).
  const n = call<GraphNode>('nodeNew', {kind, id, position: [Math.round(at.x - 100), Math.round(at.y - 12)]})
  g.nodes.push(n)
  ed.selectedNodes = [id]
  ed.changed()
  if (opts.link) {
    // Continue the dragged link into the new node, placed where it was dropped.
    try {
      if ('reverse' in opts.link) {
        const t = opts.link.reverse
        if (!hasOutput(n.kind)) throw new Error(`「${nodeInfo(n).label}」には出力がありません`)
        replaceGraph(call<Graph>('graphConnect', {graph: g, from: id, to: t.node, port: t.port ?? 0, param: t.param}))
      } else {
        const params = nodeInfo(n).params.map(p => p.key)
        if (opts.link.fromNumber) {
          if (!params.length) throw new Error(`「${nodeInfo(n).label}」には数値の入力がありません`)
          replaceGraph(call<Graph>('graphConnect', {graph: g, from: opts.link.from, to: id, port: 0, param: params[0]}))
        } else {
          if (!n.inputs.length) throw new Error(`「${nodeInfo(n).label}」には形の入力がありません`)
          replaceGraph(call<Graph>('graphConnect', {graph: g, from: opts.link.from, to: id, port: 0, param: null}))
        }
      }
      ed.commit()
      ed.status = `「${nodeInfo(n).label}」を追加してつなぎました`
    } catch (err) {
      ed.cancel()
      ed.error = err instanceof Error ? err.message : String(err)
    }
    nextTick(measure)
    return
  }
  startGrab([id], screen, true, opts.dragged)
  nextTick(measure)
}
function onMenuDrag(kind: NodeKind, e: PointerEvent) {
  const m = menu.value
  if (!m) return
  const p = localOf(e)
  addNode(kind, toWorld(p), p, {dragged: true})
}
function deleteSelected() {
  const g = graph.value
  if (!g) return
  const ids = ed.selectedNodes.filter(id => {
    const k = node(id)?.kind
    return k && k !== 'Input' && k !== 'Output'
  })
  if (!ids.length) return
  ed.edit(() => replaceGraph(call<Graph>('graphRemove', {graph: g, ids})))
  ed.selectedNodes = []
}

// ---- view ----
function onWheel(e: WheelEvent) {
  e.preventDefault()
  const p = localOf(e)
  if (e.ctrlKey || e.metaKey || settings.wheel === 'zoom') zoomAt(p, wheelZoom(e.deltaY, e.ctrlKey))
  else { view.panX -= e.deltaX; view.panY -= e.deltaY }
}
let gestureBase = 1
function onGestureStart(e: Event) { e.preventDefault(); gestureBase = 1 }
function onGestureChange(e: Event) {
  e.preventDefault()
  const g = e as Event & {scale: number; clientX: number; clientY: number}
  zoomAt(localOf(g), gestureZoom(g.scale / gestureBase))
  gestureBase = g.scale
}
function zoomAt(p: Vec2, f: number) {
  const before = toWorld(p)
  view.zoom = Math.min(2, Math.max(0.2, view.zoom * f))
  view.panX = p.x - before.x * view.zoom
  view.panY = p.y - before.y * view.zoom
}
function fit() {
  const g = graph.value
  if (!g || !root.value || !layer.value) return
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
  for (const n of g.nodes) {
    const el = layer.value.querySelector<HTMLElement>(`[data-node="${n.id}"]`)
    x0 = Math.min(x0, n.position[0]); y0 = Math.min(y0, n.position[1])
    x1 = Math.max(x1, n.position[0] + (el?.offsetWidth ?? 200)); y1 = Math.max(y1, n.position[1] + (el?.offsetHeight ?? 60))
  }
  const w = root.value.clientWidth, h = root.value.clientHeight
  if (!isFinite(x0) || w < 10 || h < 10) return
  const top = 56 // keep nodes clear of the floating set bar
  view.zoom = Math.min(1, Math.max(0.2, Math.min((w - 40) / (x1 - x0), (h - top - 20) / (y1 - y0))))
  view.panX = (w - (x1 - x0) * view.zoom) / 2 - x0 * view.zoom
  view.panY = top + (h - top - 20 - (y1 - y0) * view.zoom) / 2 - y0 * view.zoom
}
watch(() => stroke.value?.id, () => nextTick(fit))
// A replaced document drops any half-finished node gesture or open menu.
watch(() => props.ed.generation, () => { drag.value = null; menu.value = null; insertTarget.value = null; reverseTarget = null })

let spaceHeld = false
// Keys go to the node editor while the pointer is over it (Blender); shown
// by a thin accent line along its top edge.
const pointerInside = ref(false)
let lastPointer: Vec2 = {x: 100, y: 100}
function key(e: KeyboardEvent): boolean {
  const k = e.key.toLowerCase()
  if (k === 'escape') {
    if (drag.value?.kind === 'grab') { cancelGrab(); return true }
    if (menu.value) { menu.value = null; return true }
    if (drag.value?.kind === 'move' || drag.value?.kind === 'link') { ed.cancel(); drag.value = null; insertTarget.value = null; return true }
    ed.selectedNodes = []
    return true
  }
  if (k === 'a' && e.shiftKey && stroke.value) { menu.value = {screen: lastPointer, world: toWorld(lastPointer)}; return true }
  if (!graph.value) return false
  if (k === 'enter' && drag.value?.kind === 'grab') { confirmGrab(); return true }
  if (k === 'x' && e.ctrlKey) { dissolveSelected(); return true }
  if (k === 'g' && !e.metaKey && !e.ctrlKey && ed.selectedNodes.length) { startGrab(ed.selectedNodes, lastPointer, false); return true }
  if (k === 'x' || k === 'delete' || k === 'backspace') { deleteSelected(); return true }
  if (k === 'home' || k === 'f') { fit(); return true }
  if (k === 'm') { const n = activeNode.value !== undefined ? node(activeNode.value) : undefined; if (n) toggleBypass(n); return true }
  if ((e.metaKey || e.ctrlKey) && k === 'a') { ed.selectedNodes = graph.value.nodes.map(n => n.id); return true }
  return false
}
function trackPointer(e: PointerEvent) { lastPointer = localOf(e); pointerInside.value = true }
defineExpose({key, isPointerInside: () => pointerInside.value, setSpace: (v: boolean) => { spaceHeld = v }, fit})

const pendingWire = computed(() => {
  const d = drag.value
  if (d?.kind !== 'link') return null
  if (d.from < 0) {
    const t = reverseTarget
    const b = t && sockets.value.get(t.param ? `param:${t.node}:${t.param}` : `in:${t.node}:${t.port}`)
    return b ? curve(d.pointer, b) : null
  }
  const a = sockets.value.get(`out:${d.from}`)
  return a ? curve(a, d.pointer) : null
})
const box = computed(() => {
  const d = drag.value
  if (d?.kind !== 'box') return null
  return {x: Math.min(d.start.x, d.current.x), y: Math.min(d.start.y, d.current.y), w: Math.abs(d.start.x - d.current.x), h: Math.abs(d.start.y - d.current.y)}
})
const lazyLine = computed(() => {
  const d = drag.value
  if (d?.kind !== 'lazy') return null
  const a = sockets.value.get(`out:${d.from}`)
  return a ? curve(a, d.pointer) : null
})
const cutLine = computed(() => {
  const d = drag.value
  return d?.kind === 'cut' ? d.points.map(p => `${p.x},${p.y}`).join(' ') : null
})
const lazyTarget = computed(() => (drag.value?.kind === 'lazy' ? drag.value.target : null))
// ---- named sets (stored in the document) ----
const zoomLabel = computed(() => `${Math.round(view.zoom * 100)}%`)
function resetZoom() { zoomAt({x: (root.value?.clientWidth ?? 0) / 2, y: (root.value?.clientHeight ?? 0) / 2}, 1 / view.zoom) }
void kindInfo
</script>

<template>
  <div
    ref="root" class="node-editor" :class="{keys: pointerInside}" data-testid="node-editor"
    @pointerdown="onDown" @pointermove="e => { trackPointer(e); onMove(e) }" @pointerup="onUp" @pointercancel="onUp"
    @pointerleave="pointerInside = false" @dragover="onDragOver" @drop="onDrop" @wheel="onWheel" @gesturestart="onGestureStart" @gesturechange="onGestureChange"
    @contextmenu.prevent
  >
    <div class="pin-corner" @pointerdown.stop><PanelPin id="nodes" /></div>
    <div v-if="!stroke" class="empty">パスを選択すると、その外観ノードを編集できます</div>
    <div v-else-if="!graph" class="empty">
      <p>ノードを足すと（Shift＋A・右クリック・「追加」）入力と出力も一緒にできます</p>
      <p class="mute">上の <Icon icon="material-symbols:texture" class="inline" /> から、ほかのパスの外観を選んで共有できます</p>
    </div>
    <template v-else>
      <div ref="layer" class="layer" :style="{transform: `translate(${view.panX}px, ${view.panY}px) scale(${view.zoom})`}">
        <svg class="wires" width="1" height="1">
          <path
            v-for="w in wires" :key="w.key" :d="w.d" class="wire"
            :class="{number: w.link.number, insert: insertTarget && linkKey(insertTarget) === w.key}"
          />
          <path v-if="pendingWire" :d="pendingWire" class="wire pending" />
          <path v-if="lazyLine" :d="lazyLine" class="wire lazy" />
          <polyline v-if="cutLine" :points="cutLine" class="cut" />
          <rect v-if="box" :x="box.x" :y="box.y" :width="box.w" :height="box.h" class="box" />
        </svg>
        <NodeCard
          v-for="n in graph.nodes" :key="n.id" :node="n"
          :selected="ed.selectedNodes.includes(n.id) || lazyTarget === n.id" :active="activeNode === n.id"
          :rev="ed.revision" :is-output="graph.output === n.id" :effective="effective" :connected-output="outputLinked.has(n.id)"
          @param="(k, v) => onParam(n, k, v)" @set="p => onSet(n, p)"
          @begin="ed.begin()" @commit="onCommit" @bypass="toggleBypass(n)" @choose-picture="choosePicture(n)"
        />
      </div>
    </template>
    <div v-if="stroke" class="hud" data-testid="node-bar" @pointerdown.stop @wheel.stop @dblclick.stop>
      <LookPicker :ed="ed" :stroke="stroke" />
      <span class="sep" />
      <button type="button" class="add" data-testid="node-add" aria-haspopup="menu" title="ノードを追加（Shift＋A）" @click="openMenuAt">
        <Icon icon="material-symbols:add" />追加
      </button>
      <template v-if="graph">
        <span class="sep" />
        <InputCheckbox v-tooltip="'外観ノードを使う（オフで元の見た目）'" :model-value="graph.enabled" aria-label="外観ノードを使う" @update:model-value="v => mutate(g => { g.enabled = v })" />
        <span class="target" :title="`編集中のパス：${stroke.name}`">{{ stroke.name }}</span>
        <span class="sep" />
        <button type="button" class="zoom" title="100%に戻す" @click="resetZoom">{{ zoomLabel }}</button>
      </template>
    </div>
    <AddMenu
      v-if="menu && stroke" :at="menu.screen" :bounds="{width: root?.clientWidth ?? 600, height: root?.clientHeight ?? 300}"
      :hint="menu.link ? 'つなげるノードだけを表示しています' : !graph ? '入力と出力の間に入れて始めます' : undefined"
      :allow="menu.link ? (k => fitsLink(k, menu!.link!)) : undefined"
      @pick="onPick" @drag="(k, e) => graph ? onMenuDrag(k, e) : startWith(k)" @close="menu = null"
    />
  </div>
</template>

<style scoped>
.node-editor::after {
  content: ''; position: absolute; left: 0; right: 0; top: 0; height: 1px; pointer-events: none; z-index: 30;
  background: var(--tq-color-accent); opacity: 0; transition: opacity .12s;
}
.node-editor.keys::after {opacity: .9}
.node-editor {
  position: relative; width: 100%; height: 100%; overflow: hidden; touch-action: none;
  background: var(--tq-color-input);
}
.layer {position: absolute; left: 0; top: 0; transform-origin: 0 0}
.wires {position: absolute; left: 0; top: 0; overflow: visible; pointer-events: none}
.wire {fill: none; stroke: #4f9a6b; stroke-width: 2}
.wire.number {stroke: #4f7fc4; stroke-dasharray: 5 3}
.wire.insert {stroke: var(--tq-color-accent); stroke-width: 4}
.wire.pending {stroke: var(--tq-color-text-mute); stroke-dasharray: 4 3}
.wire.lazy {stroke: var(--tq-color-accent); stroke-dasharray: 6 4}
.cut {fill: none; stroke: #e0457b; stroke-width: 1.5; stroke-dasharray: 4 3}
.box {fill: color-mix(in srgb, var(--tq-color-accent) 10%, transparent); stroke: var(--tq-color-accent); stroke-dasharray: 3 2}
.empty {
  position: absolute; inset: 0; display: flex; flex-direction: column; gap: 10px;
  align-items: center; justify-content: center; color: var(--tq-color-text-mute); font-size: 12px;
}
.empty p {margin: 0}
.empty .inline {vertical-align: -2px}
.empty .mute {font-size: 10px; opacity: .8}
.add {
  display: inline-flex; align-items: center; gap: 2px; height: 20px; padding: 0 8px 0 4px; border: 0; border-radius: 10px;
  background: var(--tq-color-input); color: var(--tq-color-text); font: inherit; cursor: pointer;
}
.add:hover {background: var(--tq-color-input-hover)}
.pin-corner {position: absolute; right: 8px; top: 12px; z-index: 7}
.hud {
  position: absolute; left: 10px; top: 10px; z-index: 6; display: flex; align-items: center; gap: 4px;
  padding: 3px 4px 3px 8px; border-radius: 6px; font-size: 11px; max-width: calc(100% - 50px);
  background: color-mix(in srgb, var(--tq-color-background) 88%, transparent);
  -webkit-backdrop-filter: blur(12px); backdrop-filter: blur(12px);
  border: 1px solid var(--tq-color-border);
  box-shadow: 0 2px 8px rgba(0, 0, 0, .12), 0 0 1px rgba(0, 0, 0, .1);
}
.label {color: var(--tq-color-text-mute); white-space: nowrap; margin-left: 2px}
.set-name {width: 140px; flex: none}
.set-pick {width: 120px; flex: none}
.target {font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 180px; margin-right: 2px}
.sep {width: 1px; height: 12px; background: var(--tq-color-border)}
.zoom {border: 0; background: transparent; color: var(--tq-color-text-mute); font: inherit; cursor: pointer; padding: 2px 4px; border-radius: 3px; font-variant-numeric: tabular-nums}
.zoom:hover {background: var(--tq-color-input-hover)}
</style>
