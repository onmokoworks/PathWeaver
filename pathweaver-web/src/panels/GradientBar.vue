<script setup lang="ts">
// The stops of a gradient on a bar: click the bar to add a stop there (in
// the colour the gradient already has at that point), drag a stop to move it,
// drag it off the bar (down) to remove it. Each gesture is one undo step.
import {ref} from 'vue'
import type {Gradient} from '../core/types'
import {hexOf, stopAt, stopsCss} from '../core/paint'

const props = defineProps<{gradient: Gradient; selected: number; label: string}>()
const emit = defineEmits<{select: [i: number]; begin: []; change: [g: Gradient]; commit: []}>()
const bar = ref<HTMLDivElement>()
const dragging = ref<number | null>(null)
const removing = ref(false)

const clone = (g: Gradient): Gradient => JSON.parse(JSON.stringify(g))
const at = (e: PointerEvent) => {
  const r = bar.value!.getBoundingClientRect()
  return Math.min(1, Math.max(0, (e.clientX - r.left) / Math.max(r.width, 1)))
}
const round = (v: number) => Math.round(v * 1000) / 1000

function addAt(e: PointerEvent) {
  if (e.button !== 0) return
  const t = round(at(e))
  const g = clone(props.gradient)
  const {color, opacity} = stopAt(g, t)
  g.stops.push({offset: t, color, opacity})
  emit('begin')
  emit('change', g)
  emit('commit')
  emit('select', g.stops.length - 1)
}

let base: Gradient | null = null
function grab(e: PointerEvent, i: number) {
  if (e.button !== 0) return
  e.stopPropagation()
  e.preventDefault()
  emit('select', i)
  base = clone(props.gradient)
  dragging.value = i
  removing.value = false
  emit('begin')
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  window.addEventListener('pointercancel', up)
}
function move(e: PointerEvent) {
  const i = dragging.value
  if (i === null || !base) return
  const r = bar.value!.getBoundingClientRect()
  // pulled well below the bar: let go to remove (a gradient keeps two stops)
  removing.value = base.stops.length > 2 && e.clientY > r.bottom + 24
  const g = clone(base)
  if (removing.value) {
    g.stops.splice(i, 1)
  } else {
    g.stops[i].offset = round(at(e))
  }
  emit('change', g)
}
function up() {
  window.removeEventListener('pointermove', move)
  window.removeEventListener('pointerup', up)
  window.removeEventListener('pointercancel', up)
  if (dragging.value === null) return
  const removed = removing.value
  dragging.value = null
  removing.value = false
  emit('commit')
  if (removed) emit('select', 0)
}
</script>

<template>
  <div class="gradient-bar" :data-gradient-bar="label">
    <div ref="bar" class="track" :aria-label="`${label}のグラデーション`" @pointerdown="addAt">
      <div class="checker" />
      <div class="fill" :style="{background: stopsCss(gradient)}" />
    </div>
    <div class="stops">
      <button
        v-for="(s, i) in gradient.stops" :key="i" type="button" class="stop"
        :class="{selected: i === selected, dragging: i === dragging, removing: i === dragging && removing}"
        :style="{left: `${s.offset * 100}%`, '--c': hexOf(s.color)}" :data-stop="i"
        :aria-label="`分岐点 ${Math.round(s.offset * 100)}%`" @pointerdown="e => grab(e, i)"
      />
    </div>
  </div>
</template>

<style scoped>
.gradient-bar {position: relative; width: 100%; min-width: 0; padding-bottom: 10px}
.track {position: relative; height: calc(var(--tq-input-height, 20px) - 6px); margin-top: 3px; border-radius: 3px; overflow: hidden; cursor: copy; box-shadow: inset 0 0 0 1px var(--pw-hairline)}
.checker, .fill {position: absolute; inset: 0}
.checker {background: repeating-conic-gradient(#d8d8d8 0 25%, #fff 0 50%) 0 0 / 8px 8px}
.stops {position: absolute; left: 0; right: 0; bottom: 0; height: 10px}
.stop {
  position: absolute; top: 0; width: 9px; height: 9px; margin-left: -4.5px; padding: 0;
  background: var(--c); border: 1px solid var(--tq-color-background); border-radius: 2px;
  box-shadow: 0 0 0 1px var(--tq-color-text-mute); cursor: ew-resize;
}
.stop.selected {box-shadow: 0 0 0 1.5px var(--tq-color-text)}
.stop.dragging {box-shadow: 0 0 0 1.5px var(--tq-color-accent)}
.stop.removing {opacity: .3}
</style>
