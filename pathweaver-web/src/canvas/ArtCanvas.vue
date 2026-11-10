<script setup lang="ts">
import {gestureZoom, settings} from '../core/settings'
import {Icon} from '../../vendor/tweeq/src/Icon'
import {computed, onBeforeUnmount, onMounted, ref} from 'vue'
import type {Editor} from '../core/editor'
import type {Stroke, Vec2} from '../core/types'
import {RULER, pivotIn, type Canvas} from './controller'
import {add, sub, scale} from './geometry'
import InputButtonToggle from '../../vendor/tweeq/src/InputButtonToggle/InputButtonToggle.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import CanvasMenu, {type MenuItem} from './CanvasMenu.vue'
import TextEditor from './TextEditor.vue'
import ShapeAdjust from './ShapeAdjust.vue'
import {placedHref} from '../core/knockout'

type Flag = 'snap_guides' | 'snap_grid' | 'snap_tangent' | 'snap_normal'
const snapFlags: [Flag, string, string][] = [
  ['snap_guides', 'ガイド', '定規から引き出したガイドと交点に吸着'],
  ['snap_grid', 'グリッド', 'グリッドを表示して交点に吸着'],
  ['snap_tangent', '接線', '他のアンカーの接線方向に吸着'],
  ['snap_normal', '法線', '他のアンカーの法線方向に吸着。パスの途中から垂直に引くには、ペンでShift＋パスをクリックして描き始める'],
]
function toggleSnap(f: Flag, v: boolean) { props.ed.edit(d => { d[f] = v }) }

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const emit = defineEmits<{command: [name: string]}>()
const ed = props.ed
const cv = props.canvas
const root = ref<HTMLDivElement>()
const view = cv.view

let observer: ResizeObserver | null = null
onMounted(() => {
  observer = new ResizeObserver(([entry]) => {
    view.width = entry.contentRect.width
    view.height = entry.contentRect.height
  })
  observer.observe(root.value!)
})
onBeforeUnmount(() => observer?.disconnect())

const local = (e: {clientX: number; clientY: number}): Vec2 => {
  const r = root.value!.getBoundingClientRect()
  return {x: e.clientX - r.left, y: e.clientY - r.top}
}
function onDown(e: PointerEvent) {
  if (e.button === 2) return // the context menu (below)
  ;(document.activeElement as HTMLElement | null)?.blur?.()
  root.value!.setPointerCapture(e.pointerId)
  cv.down(e, local(e))
  e.preventDefault()
}
function onMove(e: PointerEvent) { cv.move(e, local(e)) }
function onUp(e: PointerEvent) {
  if (root.value!.hasPointerCapture(e.pointerId)) root.value!.releasePointerCapture(e.pointerId)
  cv.up(e, local(e))
}
function onWheel(e: WheelEvent) {
  e.preventDefault()
  cv.wheel(e, local(e))
}
// ---- right-click menu (Illustrator: what applies to what is under the pointer) ----
const menu = ref<{x: number; y: number} | null>(null)
function onContext(e: MouseEvent) {
  if (e.button !== 2) return // Ctrl+click is a canvas gesture, not this menu
  cv.finishPen()
  const at = local(e)
  const hit = cv.pathAt(cv.toWorld(at))
  if (hit !== null && !ed.selected.includes(hit)) ed.selectPath(hit, false)
  menu.value = at
}
const menuItems = computed<MenuItem[]>(() => {
  void ed.revision
  const sel = ed.selected.map(id => ed.findStroke(id)).filter((s): s is Stroke => !!s)
  const any = sel.length > 0
  const guides = sel.filter(s => s.guide).length
  const one = sel.length === 1 ? sel[0] : null
  return [
    {label: 'カット', shortcut: '⌘X', disabled: !any, action: () => emit('command', 'cut')},
    {label: 'コピー', shortcut: '⌘C', disabled: !any, action: () => emit('command', 'copy')},
    {label: 'ペースト', shortcut: '⌘V', disabled: !ed.hasClipboard(), action: () => emit('command', 'paste')},
    {label: '複製', shortcut: '⌘D', disabled: !any, action: () => ed.duplicateSelection()},
    {label: '削除', shortcut: '⌫', disabled: !any, action: () => ed.deleteSelection()},
    {label: '', separator: true},
    {label: 'ガイドを作成', shortcut: '⌘5', disabled: sel.length - guides === 0, action: () => ed.makeGuides(true)},
    {label: 'ガイドを解除', shortcut: '⌥⌘5', disabled: guides === 0, action: () => ed.makeGuides(false)},
    {label: '', separator: true},
    {label: '最前面へ', disabled: !any, action: () => ed.arrange('front')},
    {label: '前面へ', disabled: !any, action: () => ed.arrange('forward')},
    {label: '背面へ', disabled: !any, action: () => ed.arrange('backward')},
    {label: '最背面へ', disabled: !any, action: () => ed.arrange('back')},
    {label: '', separator: true},
    {label: 'フォルダにまとめる', shortcut: '⌘G', action: () => emit('command', 'group')},
    {label: 'ブレンド', shortcut: '⌥⌘B', disabled: sel.length !== 2 || sel.some(s => ed.blendOf(s.id)), action: () => emit('command', 'makeBlend')},
    {label: 'ブレンドを解除', shortcut: '⌥⇧⌘B', disabled: !sel.some(s => ed.blendOf(s.id)), action: () => emit('command', 'releaseBlend')},
    {label: one?.appearance ? '外観ノードを編集' : '外観ノードを開く', disabled: !one || !!one.guide, action: () => emit('command', 'openNodes')},
    {label: '', separator: true},
    {label: 'すべてを選択', shortcut: '⌘A', action: () => emit('command', 'selectAll')},
    {label: '選択を解除', disabled: !any, action: () => { ed.selected = []; ed.anchors = [] }},
  ]
})

// Safari trackpad pinch.
let gestureBase = 1
function onGestureStart(e: Event) { e.preventDefault(); gestureBase = 1 }
function onGestureChange(e: Event) {
  e.preventDefault()
  const g = e as Event & {scale: number; clientX: number; clientY: number}
  cv.zoomAt(local(g), gestureZoom(g.scale / gestureBase))
  gestureBase = g.scale
}

const zoom = computed(() => view.zoom)
/** The last selected point is the one the inspector edits (like the active path / node). */
function isActiveAnchor(stroke: number, node: number) {
  const a = ed.anchors[ed.anchors.length - 1]
  return ed.anchors.length > 1 && !!a && a.stroke === stroke && a.node === node
}
const worldTransform = computed(() => `translate(${view.panX} ${view.panY}) scale(${view.zoom})`)
const S = (p: Vec2) => cv.toScreen(p)

const visible = computed(() => ed.doc.strokes.filter(s => ed.shown(s)))
const selectedStrokes = computed(() => {
  void ed.revision
  return ed.selected.map(id => ed.findStroke(id)).filter((s): s is Stroke => !!s && ed.shown(s))
})
const showAnchors = computed(() => ed.tool !== 'select')
const anchorStrokes = computed(() => {
  void ed.revision
  // Direct/convert tools show anchors of every editable path so any can be picked.
  if (ed.tool === 'direct' || ed.tool === 'convert') return visible.value.filter(s => !s.contours)
  return selectedStrokes.value.filter(s => !s.contours)
})

const box = computed(() => {
  void ed.revision
  if (ed.tool !== 'select' || !ed.selected.length) return null
  const b = cv.selectionBounds()
  if (!b) return null
  const a = S({x: b.x0, y: b.y0}), c = S({x: b.x1, y: b.y1})
  return {x: a.x, y: a.y, w: c.x - a.x, h: c.y - a.y, handles: cv.boxHandles(b).map(S), pivot: S(pivotIn(b, cv.state.pivot))}
})

const marquee = computed(() => {
  const d = cv.state.drag
  if (!d || d.kind !== 'marquee') return null
  const a = S(d.start), b = S(d.current)
  return {x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y)}
})

const penPreview = computed(() => {
  void ed.revision
  if (ed.tool !== 'pen' || cv.state.penPath === null || cv.state.drag) return null
  const s = ed.findStroke(cv.state.penPath)
  const p = cv.state.hover.pointer
  if (!s || !p || !s.nodes.length) return null
  const last = s.nodes[s.nodes.length - 1]
  const a = S(last.p), c1 = S(add(last.p, last.outgoing)), b = S(p)
  return `M${a.x} ${a.y}C${c1.x} ${c1.y} ${b.x} ${b.y} ${b.x} ${b.y}`
})

const widthOverlay = computed(() => {
  void ed.revision
  if (!cv.widthVisible()) return []
  const d = cv.state.drag
  // The side being dragged, else the one under the pointer.
  const hot = d?.kind === 'width' ? {stroke: d.stroke, target: d.target, side: d.side} : cv.state.hoverGrip
  const alt = cv.state.alt
  return selectedStrokes.value.filter(s => !s.contours).flatMap(s => cv.widthHandles(s).map(h => {
    const g = cv.widthGrips(h)
    const t = cv.state.widthTarget
    const mine = !!hot && hot.stroke === s.id && hot.target === h.target
    const c = S(h.center)
    const side = (k: 'left' | 'right') => {
      const p = S(g[k])
      const isHot = mine && hot!.side === k
      const v = {x: p.x - c.x, y: p.y - c.y}
      const n = Math.hypot(v.x, v.y) || 1
      return {
        p, hot: isHot,
        // With Option the opposite side stays put: show it faded.
        dim: mine && alt && hot!.side !== k,
        // Partner moves along with a normal drag: hint it.
        moves: mine && !alt && hot!.side !== k,
        label: isHot ? (alt ? 'この側だけ' : '両側') : '',
        lx: p.x + (v.x / n) * 12, ly: p.y + (v.y / n) * 12,
      }
    }
    return {
      id: `${s.id}:${h.target}`, c, sides: [side('left'), side('right')],
      active: !!t && t.stroke === s.id && t.target === h.target,
      key: typeof h.target === 'number', interpolated: h.interpolated,
    }
  }))
})

// Live corner widgets in the direct tool (selected paths).
const cornerOverlay = computed(() => {
  void ed.revision
  if (ed.tool !== 'direct') return []
  return selectedStrokes.value.filter(s => !s.contours).flatMap(s => cv.cornerWidgets(s).map(w => ({key: `${s.id}:${w.node}`, p: S(w.p), on: w.radius > 0, kind: w.kind})))
})

// Normal (and tangent) guide through a pen point that sits on another path.
const pathFrameLines = computed(() => {
  void ed.revision
  void cv.state.hover.pointer
  const f = cv.activePathFrame()
  if (!f) return []
  const far = 1e5 / view.zoom
  const lines = [{d: {x: -f.tangent.y, y: f.tangent.x}, label: '法線'}]
  if (ed.doc.snap_tangent) lines.push({d: f.tangent, label: '接線'})
  return lines.map(({d, label}) => ({
    a: S(sub(f.origin, scale(d, far))), b: S(add(f.origin, scale(d, far))), label,
    at: S(add(f.origin, scale(d, 40 / view.zoom))),
  }))
})

const snapLine = computed(() => {
  const g = cv.state.snap
  if (!g) return null
  const far = 1e5 / view.zoom
  const at = cv.state.snapAt && !cv.state.drag ? S(cv.state.snapAt) : null
  if (g.kind === 'Grid' || g.kind === 'Intersection') return {point: S(g.origin), at: null, label: ''}
  if (g.kind === 'PathGuide') return {point: S(g.origin), at: null, label: 'ガイド'}
  const a = S(sub(g.origin, scale(g.direction, far))), b = S(add(g.origin, scale(g.direction, far)))
  // Normal / tangent: mark the anchor it comes from and name the line.
  const deg = ((Math.round(Math.atan2(-g.direction.y, g.direction.x) * 180 / Math.PI) % 360) + 360) % 360
  const label = g.kind === 'Normal' ? '法線' : g.kind === 'Tangent' ? '接線' : g.kind === 'Angle' ? `${deg}°` : g.kind === 'PathGuide' ? 'ガイド' : ''
  return {a, b, point: S(g.origin), at, label, source: label !== ''}
})

// ---- rulers & grid ----
function niceStep(px: number) {
  const raw = px / view.zoom
  const p = Math.pow(10, Math.floor(Math.log10(raw)))
  for (const m of [1, 2, 5, 10]) if (m * p >= raw) return m * p
  return 10 * p
}
const ticks = computed(() => {
  const step = niceStep(70)
  const xs: {pos: number; label: string}[] = []
  const ys: {pos: number; label: string}[] = []
  const x0 = Math.floor(cv.toWorld({x: RULER, y: 0}).x / step) * step
  for (let x = x0; S({x, y: 0}).x < view.width; x += step) xs.push({pos: S({x, y: 0}).x, label: fmt(x)})
  const y0 = Math.floor(cv.toWorld({x: 0, y: RULER}).y / step) * step
  for (let y = y0; S({x: 0, y}).y < view.height; y += step) ys.push({pos: S({x: 0, y}).y, label: fmt(y)})
  // a label that would run under the corner square is left out (its tick stays)
  for (const t of xs) if (t.pos + 3 < RULER) t.label = ''
  for (const t of ys) if (t.pos + 3 - t.label.length * 5.5 < RULER) t.label = ''
  return {xs, ys, minor: step / 5 * view.zoom}
})
const fmt = (v: number) => (Math.abs(v) < 1e-9 ? '0' : String(+v.toFixed(3)))
const grid = computed(() => {
  const s = ed.doc.grid_spacing * view.zoom
  if (!ed.doc.snap_grid || s < 8) return null
  return {s, x: view.panX % s, y: view.panY % s}
})
const cursor = computed(() => {
  const d = cv.state.drag
  if (d?.kind === 'pan') return 'grabbing'
  return cv.state.hover.cursor
})
</script>

<template>
  <div
    ref="root" class="art-canvas" :class="`paper-${cv.state.paper}`" :style="{cursor}" data-testid="canvas" :data-paper="cv.state.paper"
    @pointerdown="onDown" @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp"
    @wheel="onWheel" @gesturestart="onGestureStart" @gesturechange="onGestureChange"
    @contextmenu.prevent="onContext"
  >
    <svg class="layer" :width="view.width" :height="view.height">
      <defs>
        <pattern id="checker" width="16" height="16" patternUnits="userSpaceOnUse">
          <rect width="16" height="16" fill="#ffffff" /><rect width="8" height="8" fill="#e4e4e4" /><rect x="8" y="8" width="8" height="8" fill="#e4e4e4" />
        </pattern>
        <pattern v-if="grid" id="grid" :width="grid.s" :height="grid.s" patternUnits="userSpaceOnUse" :x="grid.x" :y="grid.y">
          <path :d="`M${grid.s} 0V${grid.s}H0`" fill="none" class="grid-line" />
        </pattern>
      </defs>
      <rect class="paper" :width="view.width" :height="view.height" />
      <rect v-if="grid" :width="view.width" :height="view.height" fill="url(#grid)" />

      <!-- artwork -->
      <g :transform="worldTransform">
        <g v-for="s in visible" :key="s.id" :data-stroke="s.id">
          <template v-for="(l, i) in ed.renders.get(s.id)?.layers ?? []" :key="i">
            <image
              v-if="l.placed" :href="placedHref(ed.doc.assets, l.placed.asset, l.placed.knockout)" :width="l.placed.w" :height="l.placed.h" :opacity="l.opacity"
              :transform="`matrix(${l.placed.matrix.join(' ')})`" preserveAspectRatio="none" class="placed" :class="{reference: l.placed.reference}" :data-placed="s.id"
            />
            <image v-else-if="l.image" :href="l.image.href" :x="l.image.x" :y="l.image.y" :width="l.image.w" :height="l.image.h" :opacity="l.opacity" preserveAspectRatio="none" class="raster" />
            <path v-else :d="l.d" :fill="l.color" :fill-opacity="l.opacity" fill-rule="nonzero" />
          </template>
        </g>
      </g>

      <!-- guides -->
      <g class="guides">
        <template v-for="(g, i) in ed.doc.guides" :key="i">
          <line v-if="g.vertical" :x1="S({x: g.position, y: 0}).x" :x2="S({x: g.position, y: 0}).x" y1="0" :y2="view.height" />
          <line v-else :y1="S({x: 0, y: g.position}).y" :y2="S({x: 0, y: g.position}).y" x1="0" :x2="view.width" />
        </template>
      </g>

      <!-- guides made from paths -->
      <g :transform="worldTransform" class="path-guides">
        <path v-for="s in visible.filter(v => v.guide)" :key="s.id" :d="ed.renders.get(s.id)?.center" :data-guide-path="s.id"
          :class="{selected: ed.selected.includes(s.id)}" vector-effect="non-scaling-stroke" />
      </g>

      <!-- selection: centerlines -->
      <g :transform="worldTransform" class="centerlines">
        <path v-for="s in selectedStrokes" :key="s.id" :d="ed.renders.get(s.id)?.center" class="center" vector-effect="non-scaling-stroke" />
        <path v-if="cv.state.hover.segment" :d="ed.renders.get(cv.state.hover.segment.stroke)?.center" class="center hover" vector-effect="non-scaling-stroke" />
      </g>
      <path v-if="penPreview" :d="penPreview" class="pen-preview" />

      <!-- anchors & handles -->
      <g v-if="showAnchors">
        <template v-for="s in anchorStrokes" :key="s.id">
          <template v-if="ed.selected.includes(s.id)">
            <template v-for="n in s.nodes" :key="`h${n.id}`">
              <template v-for="v in [n.incoming, n.outgoing]">
                <g v-if="Math.hypot(v.x, v.y) > 0.01">
                  <line :x1="S(n.p).x" :y1="S(n.p).y" :x2="S(add(n.p, v)).x" :y2="S(add(n.p, v)).y" class="handle-line" />
                  <circle :cx="S(add(n.p, v)).x" :cy="S(add(n.p, v)).y" r="3.5" class="handle" />
                </g>
              </template>
            </template>
          </template>
          <rect
            v-for="n in s.nodes" :key="n.id" :data-anchor="`${s.id}:${n.id}`"
            :x="S(n.p).x - 3.5" :y="S(n.p).y - 3.5" width="7" height="7"
            class="anchor" :class="{selected: ed.isAnchorSelected({stroke: s.id, node: n.id}), active: isActiveAnchor(s.id, n.id), first: !s.closed && n === s.nodes[0]}"
          />
        </template>
      </g>
      <circle v-if="cv.state.hover.segment && ed.tool !== 'select'" :cx="S(cv.state.hover.segment.point).x" :cy="S(cv.state.hover.segment.point).y" r="4" class="add-marker" />

      <!-- live corners -->
      <g v-for="c in cornerOverlay" :key="c.key" class="corner-widget" :class="{on: c.on}" :data-corner="c.key">
        <circle :cx="c.p.x" :cy="c.p.y" r="4" />
        <circle v-if="c.kind === 'Clothoid'" :cx="c.p.x" :cy="c.p.y" r="1.5" class="dot" />
        <line v-if="c.kind === 'Bevel'" :x1="c.p.x - 2.5" :y1="c.p.y + 2.5" :x2="c.p.x + 2.5" :y2="c.p.y - 2.5" />
      </g>

      <!-- width handles -->
      <g v-for="w in widthOverlay" :key="w.id" class="width-handle" :class="{active: w.active, interpolated: w.interpolated}">
        <g v-for="(sd, i) in w.sides" :key="i" class="grip" :class="{hot: sd.hot, dim: sd.dim, moves: sd.moves}" :data-grip="i === 0 ? 'left' : 'right'">
          <line :x1="w.c.x" :y1="w.c.y" :x2="sd.p.x" :y2="sd.p.y" />
          <circle :cx="sd.p.x" :cy="sd.p.y" :r="sd.hot ? 4.5 : 3.5" />
          <text v-if="sd.label" :x="sd.lx" :y="sd.ly + 3" :text-anchor="sd.lx < sd.p.x ? 'end' : 'start'">{{ sd.label }}</text>
        </g>
        <circle v-if="w.key" :cx="w.c.x" :cy="w.c.y" r="2.5" class="key-dot" />
      </g>

      <!-- bounding box -->
      <g v-if="box" class="bbox">
        <rect :x="box.x" :y="box.y" :width="box.w" :height="box.h" />
        <rect v-for="(h, i) in box.handles" :key="i" :x="h.x - 3.5" :y="h.y - 3.5" width="7" height="7" class="bbox-handle" />
        <!-- reference point: rotation and Option-scale turn around it -->
        <g class="pivot" data-testid="pivot" :transform="`translate(${box.pivot.x} ${box.pivot.y})`">
          <circle r="5" /><line x1="-8" x2="8" y1="0" y2="0" /><line x1="0" x2="0" y1="-8" y2="8" />
        </g>
      </g>

      <g v-for="l in pathFrameLines" :key="l.label" class="path-frame">
        <line :x1="l.a.x" :y1="l.a.y" :x2="l.b.x" :y2="l.b.y" />
        <text :x="l.at.x + 6" :y="l.at.y - 6">{{ l.label }}</text>
      </g>
      <rect v-if="marquee" class="marquee" :x="marquee.x" :y="marquee.y" :width="marquee.w" :height="marquee.h" />
      <g v-if="snapLine" class="snap">
        <line v-if="snapLine.a" :x1="snapLine.a.x" :y1="snapLine.a.y" :x2="snapLine.b!.x" :y2="snapLine.b!.y" />
        <circle :cx="snapLine.point.x" :cy="snapLine.point.y" r="4" :class="{source: snapLine.source}" />
        <circle v-if="snapLine.at" :cx="snapLine.at.x" :cy="snapLine.at.y" r="3" class="at" />
        <text v-if="snapLine.label" class="snap-label" :x="(snapLine.at ?? snapLine.point).x + 8" :y="(snapLine.at ?? snapLine.point).y - 8">{{ snapLine.label }}</text>
      </g>

      <!-- rulers -->
      <g class="ruler">
        <rect x="0" y="0" :width="view.width" :height="RULER" />
        <rect x="0" y="0" :width="RULER" :height="view.height" />
        <g v-for="t in ticks.xs" :key="`x${t.pos}`">
          <line :x1="t.pos" :x2="t.pos" y1="0" :y2="RULER" />
          <text :x="t.pos + 3" y="10">{{ t.label }}</text>
        </g>
        <g v-for="t in ticks.ys" :key="`y${t.pos}`">
          <line :y1="t.pos" :y2="t.pos" x1="0" :x2="RULER" />
          <text :transform="`translate(10 ${t.pos + 3}) rotate(-90)`" text-anchor="end">{{ t.label }}</text>
        </g>
        <rect x="0" y="0" :width="RULER" :height="RULER" class="corner" />
      </g>
    </svg>
    <ShapeAdjust v-if="cv.shapeAdjustable()" :ed="ed" :canvas="cv" />
    <TextEditor v-if="cv.state.textEdit" :key="cv.state.textEdit.stroke" :ed="ed" :canvas="cv" :id="cv.state.textEdit.stroke" />
    <slot :bounds="{width: view.width, height: view.height}" />
    <div class="snap-bar" data-testid="snap-bar" @pointerdown.stop @wheel.stop>
      <span class="snap-title" aria-hidden="true" title="吸着"><Icon icon="mdi:magnet" /></span>
      <InputButtonToggle
        v-for="[f, label, hint] in snapFlags" :key="f" v-tooltip="{content: hint, placement: 'top'}"
        :model-value="ed.doc[f]" :label="label" :aria-label="`${label}に吸着`" :data-snap="f"
        @update:model-value="v => toggleSnap(f, v)"
      />
    </div>
    <div v-if="ed.renderPending" class="busy-badge" data-testid="render-busy">計算中…</div>
    <CanvasMenu v-if="menu" :at="menu" :bounds="{width: view.width, height: view.height}" :items="menuItems" @close="menu = null" />
    <button
      type="button" class="looks-badge" :class="{off: !settings.showLooks}" data-testid="looks-toggle" :aria-pressed="settings.showLooks"
      v-tooltip="{content: settings.showLooks ? '外観ノードを表示中：クリックで全部オフ（軽くなる。ファイルと書き出しは変わらない）' : '外観ノードを表示していません：クリックで表示', placement: 'top'}"
      @pointerdown.stop @click="settings.showLooks = !settings.showLooks"
    ><Icon icon="material-symbols:account-tree-outline" /><span>{{ settings.showLooks ? '外観' : '外観オフ' }}</span></button>
    <button type="button" class="zoom-badge" data-testid="zoom-badge" title="クリックで100%（⌘1）" @pointerdown.stop @click="cv.zoomAt({x: view.width / 2, y: view.height / 2}, 1 / view.zoom)">{{ Math.round(zoom * 100) }}%</button>
  </div>
</template>

<style scoped>
.art-canvas {
  position: relative; width: 100%; height: 100%; overflow: hidden; touch-action: none;
  --pen: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M3 3l6 15 2.5-5.5L17 10z' fill='white' stroke='black' stroke-width='1.2'/></svg>") 3 3, crosshair;
  --pen-add: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M3 3l6 15 2.5-5.5L17 10z' fill='white' stroke='black' stroke-width='1.2'/><path d='M15 17h7M18.5 13.5v7' stroke='black' stroke-width='1.6'/></svg>") 3 3, copy;
  --pen-close: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M3 3l6 15 2.5-5.5L17 10z' fill='white' stroke='black' stroke-width='1.2'/><circle cx='18.5' cy='17' r='3' fill='none' stroke='black' stroke-width='1.4'/></svg>") 3 3, crosshair;
  --pen-on-path: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M3 3l6 15 2.5-5.5L17 10z' fill='white' stroke='black' stroke-width='1.2'/><path d='M14 21l8-8M18 17l-3-3' stroke='black' stroke-width='1.6'/></svg>") 3 3, crosshair;
  --pen-continue: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M3 3l6 15 2.5-5.5L17 10z' fill='white' stroke='black' stroke-width='1.2'/><path d='M15 17h7' stroke='black' stroke-width='1.6'/></svg>") 3 3, crosshair;
  --direct: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M4 3v15l4-4 3 6 2-1-3-6h5z' fill='white' stroke='black' stroke-width='1.2'/></svg>") 4 3, default;
  --direct-anchor: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M4 3v15l4-4 3 6 2-1-3-6h5z' fill='white' stroke='black' stroke-width='1.2'/><rect x='15' y='15' width='6' height='6' fill='white' stroke='black'/></svg>") 4 3, pointer;
  --width: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M12 2v20M7 7l5-5 5 5M7 17l5 5 5-5' fill='none' stroke='white' stroke-width='4'/><path d='M12 2v20M7 7l5-5 5 5M7 17l5 5 5-5' fill='none' stroke='black' stroke-width='1.5'/></svg>") 12 12, ns-resize;
  --width-one: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M12 3v18M7 8l5-5 5 5' fill='none' stroke='white' stroke-width='4'/><path d='M12 3v18M7 8l5-5 5 5' fill='none' stroke='black' stroke-width='1.5'/><path d='M6 21h12' stroke='black' stroke-width='2'/></svg>") 12 12, n-resize;
  --rotate: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24'><path d='M6 14a7 7 0 1 1 7 6' fill='none' stroke='white' stroke-width='4'/><path d='M6 14a7 7 0 1 1 7 6' fill='none' stroke='black' stroke-width='1.6'/><path d='M2 12l4 4 4-4z'/></svg>") 12 12, crosshair;
}
.layer {position: absolute; inset: 0; display: block}
.paper {fill: var(--pw-paper)}
/* Pictures never take the pointer (WebKit would start a native image drag);
   picking is done by the controller's own hit test. */
.layer image {pointer-events: none; -webkit-user-drag: none}
.paper-checker .paper {fill: url(#checker)}
/* Overlay colours follow the artwork background, not the UI theme. */
.paper-gray {--pw-paper: #8a8c90; --pw-grid: rgba(0,0,0,.12)}
.paper-black {--pw-paper: #111214; --pw-ink: #e8e8ea; --pw-grid: rgba(255,255,255,.09)}
.grid-line {stroke: var(--pw-grid); stroke-width: 1}
.guides line {stroke: var(--pw-guide); stroke-width: 1}
.path-guides path {fill: none; stroke: var(--pw-guide); stroke-width: 1}
.path-guides path.selected {stroke-width: 2}
.center {fill: none; stroke: var(--tq-color-accent); stroke-width: 1}
.center.hover {stroke-width: 2}
.pen-preview {fill: none; stroke: var(--tq-color-accent); stroke-width: 1; stroke-dasharray: 4 3}
.handle-line {stroke: var(--tq-color-accent); stroke-width: 1}
.handle {fill: var(--pw-paper); stroke: var(--tq-color-accent); stroke-width: 1}
.anchor {fill: var(--pw-paper); stroke: var(--tq-color-accent); stroke-width: 1}
.anchor.selected {fill: var(--tq-color-accent)}
.anchor.active {stroke: var(--pw-ink); stroke-width: 2}
.add-marker {fill: var(--tq-color-accent); stroke: var(--pw-paper); stroke-width: 1.5}
/* As in the first PathWeaver: a thin guide, small paper grips with an ink
   ring, the selected width point in orange. */
.corner-widget circle {fill: var(--pw-paper); stroke: var(--tq-color-accent); stroke-width: 1}
.corner-widget.on circle:first-child {stroke-width: 1.6}
.corner-widget .dot {fill: var(--tq-color-accent)}
.corner-widget line {stroke: var(--tq-color-accent); stroke-width: 1}
.width-handle line {stroke: var(--pw-guide); stroke-width: .8}
.width-handle.interpolated line {stroke-dasharray: 3 2}
.width-handle .grip circle {fill: var(--pw-paper); stroke: var(--pw-ink); stroke-width: 1}
.width-handle.active .grip circle {stroke: var(--pw-width)}
.width-handle .key-dot {fill: var(--pw-guide)}
.width-handle.active .key-dot {fill: var(--pw-width)}
.width-handle .grip.hot circle {fill: var(--pw-width); stroke: var(--pw-width)}
.width-handle .grip.hot line {stroke: var(--pw-width); stroke-width: 1.6}
.width-handle .grip.moves circle {stroke: var(--pw-width); stroke-dasharray: 2 1.5}
.width-handle .grip.dim {opacity: .3}
.width-handle text {font-size: 10px; font-weight: 600; fill: var(--pw-width); paint-order: stroke; stroke: var(--pw-paper); stroke-width: 3px; pointer-events: none}
.bbox rect {fill: none; stroke: var(--tq-color-accent); stroke-width: 1}
.bbox .bbox-handle {fill: var(--pw-paper)}
.bbox .pivot circle {fill: none; stroke: var(--tq-color-accent); stroke-width: 1}
.bbox .pivot line {stroke: var(--tq-color-accent); stroke-width: 1}
.marquee {fill: color-mix(in srgb, var(--tq-color-accent) 10%, transparent); stroke: var(--tq-color-accent); stroke-dasharray: 3 2}
.snap line {stroke: var(--pw-snap); stroke-width: 1}
.path-frame line {stroke: var(--pw-snap); stroke-width: 1; stroke-dasharray: 5 4; opacity: .7}
.path-frame text {fill: var(--pw-snap); font-size: 10px; pointer-events: none}
.snap circle {fill: none; stroke: var(--pw-snap); stroke-width: 1.5}
.snap circle.source {stroke-width: 2}
.snap circle.at {fill: var(--pw-snap); stroke: var(--pw-paper); stroke-width: 1}
.snap-label {fill: var(--pw-snap); font-size: 10px; font-weight: 600; pointer-events: none; paint-order: stroke; stroke: var(--pw-paper); stroke-width: 3px}
.ruler rect {fill: var(--tq-color-surface, var(--tq-color-input)); }
.ruler .corner {fill: var(--tq-color-input)}
.ruler line {stroke: var(--tq-color-border); stroke-width: 1}
.ruler text {font-size: 9px; fill: var(--tq-color-text-mute); font-variant-numeric: tabular-nums; pointer-events: none}
.snap-bar {
  position: absolute; left: 28px; bottom: 10px; display: flex; align-items: center; gap: 2px;
  padding: 3px; border-radius: 6px; font-size: 11px; cursor: default;
  background: color-mix(in srgb, var(--tq-color-background) 88%, transparent);
  -webkit-backdrop-filter: blur(12px); backdrop-filter: blur(12px);
  border: 1px solid var(--tq-color-border);
  box-shadow: 0 2px 8px rgba(0, 0, 0, .12), 0 0 1px rgba(0, 0, 0, .1);
}
.snap-title {display: grid; place-items: center; padding: 0 4px 0 3px; margin-right: 2px; font-size: 14px; color: var(--tq-color-text-mute); border-right: 1px solid var(--tq-color-border); height: 18px}
.snap-bar :deep(.TqInputButtonToggle) {padding: 0 8px; height: 22px; font-size: 11px; border-radius: 3px}
.busy-badge {
  position: absolute; right: 144px; bottom: 10px; font-size: 11px;
  color: var(--tq-color-text-mute); pointer-events: none;
}
.zoom-badge {
  position: absolute; right: 8px; bottom: 8px; height: 22px; padding: 0 8px; border-radius: 3px; border: 0;
  font: inherit; font-size: 11px; color: var(--tq-color-text); background: var(--tq-color-neutral, var(--tq-color-input));
  font-variant-numeric: tabular-nums; cursor: pointer;
}
.zoom-badge:hover, .looks-badge:hover {background: var(--tq-color-input-hover)}
.looks-badge {
  position: absolute; right: 64px; bottom: 8px; height: 22px; padding: 0 8px; border-radius: 3px; border: 0;
  display: flex; align-items: center; gap: 4px;
  font: inherit; font-size: 11px; color: var(--tq-color-text-mute); background: var(--tq-color-neutral, var(--tq-color-input)); cursor: pointer;
}
.looks-badge.off {color: var(--tq-color-text); box-shadow: inset 0 0 0 1px var(--tq-color-text-mute)}
</style>
