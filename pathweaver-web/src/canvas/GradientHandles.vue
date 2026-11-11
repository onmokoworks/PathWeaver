<script setup lang="ts">
// On-canvas gradient handles (direct selection tool): a linear gradient is a
// line from its start to its end; a radial one a centre and a point on its
// rim. Dragging a grip changes the gradient — one undo step per drag. The
// stops sit on the line in their own colours. Grey at rest, accent while
// dragged (blue only for what is being touched).
import {computed, onBeforeUnmount, ref} from 'vue'
import type {Editor} from '../core/editor'
import type {Canvas} from './controller'
import type {Gradient, Stroke, Vec2} from '../core/types'
import {gradientEnd, gradientOf, gradientStart, hexOf, lineMatrix, setGradient, sortedStops, type PaintKey} from '../core/paint'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const ed = props.ed
const cv = props.canvas
type Part = 'start' | 'end' | 'line'
const drag = ref<{key: PaintKey; part: Part} | null>(null)

const target = computed<Stroke | null>(() => {
  void ed.revision
  if (ed.tool !== 'direct' || ed.selected.length !== 1) return null
  const s = ed.findStroke(ed.selected[0])
  return s && ed.shown(s) && !s.image && !s.guide ? s : null
})
const S = (p: Vec2) => cv.toScreen(p)
const items = computed(() => {
  void ed.revision
  void cv.view.zoom; void cv.view.panX; void cv.view.panY
  const s = target.value
  if (!s) return []
  return (['fill', 'edge'] as const).flatMap(key => {
    const g = s[key] ? gradientOf(s, key) : null
    if (!g) return []
    const a = S(gradientStart(g)), b = S(gradientEnd(g))
    const r = Math.hypot(b.x - a.x, b.y - a.y)
    const lerp = (t: number) => ({x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t})
    return [{
      key, a, b, r, radial: g.kind === 'Radial',
      stops: sortedStops(g).map((st, i) => ({i, p: lerp(st.offset), color: hexOf(st.color)})),
    }]
  })
})

let startPointer: Vec2 = {x: 0, y: 0}
let startGradient: Gradient | null = null
let svgRect: DOMRect | null = null
const world = (e: PointerEvent) => cv.toWorld({x: e.clientX - svgRect!.left, y: e.clientY - svgRect!.top})
function down(e: PointerEvent, key: PaintKey, part: Part) {
  if (e.button !== 0) return
  e.stopPropagation()
  e.preventDefault()
  const s = target.value
  const g = s && gradientOf(s, key)
  if (!s || !g) return
  svgRect = ((e.currentTarget as Element).closest('svg') as SVGSVGElement).getBoundingClientRect()
  startPointer = world(e)
  startGradient = JSON.parse(JSON.stringify(g)) as Gradient
  drag.value = {key, part}
  ed.begin()
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  window.addEventListener('pointercancel', up)
}
function move(e: PointerEvent) {
  const d = drag.value, s = target.value, g0 = startGradient
  if (!d || !s || !g0 || !svgRect) return
  const p = world(e)
  const a0 = gradientStart(g0), b0 = gradientEnd(g0)
  let a = a0, b = b0
  if (d.part === 'line') {
    const dx = p.x - startPointer.x, dy = p.y - startPointer.y
    a = {x: a0.x + dx, y: a0.y + dy}; b = {x: b0.x + dx, y: b0.y + dy}
  } else {
    const fixed = d.part === 'start' ? b0 : a0
    let q = p
    if (e.shiftKey) {
      // Shift: the axis turns in the document's angle steps.
      const step = (ed.doc.angle_step || 15) * Math.PI / 180
      const v = {x: q.x - fixed.x, y: q.y - fixed.y}
      const len = Math.hypot(v.x, v.y)
      const ang = Math.round(Math.atan2(v.y, v.x) / step) * step
      q = {x: fixed.x + Math.cos(ang) * len, y: fixed.y + Math.sin(ang) * len}
    }
    if (d.part === 'start') {
      // a radial centre carries its rim along; a linear start moves alone
      if (g0.kind === 'Radial') { a = q; b = {x: b0.x + q.x - a0.x, y: b0.y + q.y - a0.y} } else a = q
    } else b = q
  }
  setGradient(s, d.key, {...g0, matrix: lineMatrix(a, b)})
  ed.changed()
}
function up() {
  window.removeEventListener('pointermove', move)
  window.removeEventListener('pointerup', up)
  window.removeEventListener('pointercancel', up)
  if (drag.value) ed.commit()
  drag.value = null
}
onBeforeUnmount(up)
const on = (key: PaintKey, part: Part) => drag.value?.key === key && (drag.value.part === part || drag.value.part === 'line')
</script>

<template>
  <g class="gradient-handles">
    <g v-for="it in items" :key="it.key" :data-gradient="it.key" :class="{dragging: drag?.key === it.key}">
      <circle v-if="it.radial" :cx="it.a.x" :cy="it.a.y" :r="it.r" class="rim" />
      <line :x1="it.a.x" :y1="it.a.y" :x2="it.b.x" :y2="it.b.y" class="axis" />
      <line :x1="it.a.x" :y1="it.a.y" :x2="it.b.x" :y2="it.b.y" class="grab" data-grip="line" @pointerdown="e => down(e, it.key, 'line')" />
      <rect v-for="st in it.stops" :key="st.i" :x="st.p.x - 3" :y="st.p.y - 3" width="6" height="6" :fill="st.color" class="stop" />
      <circle :cx="it.a.x" :cy="it.a.y" r="5" class="grip" :class="{on: on(it.key, 'start')}" data-grip="start" @pointerdown="e => down(e, it.key, 'start')" />
      <circle :cx="it.b.x" :cy="it.b.y" r="4" class="grip end" :class="{on: on(it.key, 'end')}" data-grip="end" @pointerdown="e => down(e, it.key, 'end')" />
    </g>
  </g>
</template>

<style scoped>
.axis {stroke: var(--pw-ink); stroke-width: 1; opacity: .55; pointer-events: none}
.dragging .axis {stroke: var(--tq-color-accent); opacity: 1}
.grab {stroke: transparent; stroke-width: 8; cursor: move}
.rim {fill: none; stroke: var(--pw-ink); stroke-width: 1; stroke-dasharray: 4 3; opacity: .4; pointer-events: none}
.dragging .rim {stroke: var(--tq-color-accent); opacity: .8}
.stop {stroke: var(--pw-paper); stroke-width: 1; pointer-events: none}
.grip {fill: var(--pw-paper); stroke: var(--pw-ink); stroke-width: 1; cursor: grab}
.grip.end {cursor: crosshair}
.grip.on {fill: var(--tq-color-accent); stroke: var(--tq-color-accent)}
</style>
