<script setup lang="ts">
// Floating tool palette over the canvas. Drag the grip to move it,
// double-click the grip to switch vertical / horizontal. Position and
// orientation are a per-viewer preference (localStorage).
import {computed, onBeforeUnmount, onMounted, reactive, ref, watch} from 'vue'
import InputButtonToggle from '../../vendor/tweeq/src/InputButtonToggle/InputButtonToggle.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import type {Tool} from '../core/editor'

const props = defineProps<{
  tools: {id: Tool; icon: string; label: string; key: string}[]
  active: Tool
  bounds: {width: number; height: number}
}>()
const emit = defineEmits<{select: [tool: Tool]}>()

const KEY = 'pw.toolPalette'
const load = () => {
  try { return {x: 28, y: 28, horizontal: false, ...JSON.parse(localStorage.getItem(KEY) ?? '{}')} } catch { return {x: 28, y: 28, horizontal: false} }
}
const pos = reactive(load())
const root = ref<HTMLDivElement>()
const size = reactive({w: 44, h: 200})
const save = () => { try { localStorage.setItem(KEY, JSON.stringify(pos)) } catch { /* preference only */ } }

// Keep the palette inside the canvas (rulers are 20px).
const clamped = computed(() => ({
  x: Math.max(24, Math.min(pos.x, props.bounds.width - size.w - 4)),
  y: Math.max(24, Math.min(pos.y, props.bounds.height - size.h - 4)),
}))

let observer: ResizeObserver | null = null
onMounted(() => {
  observer = new ResizeObserver(() => {
    if (!root.value) return
    size.w = root.value.offsetWidth
    size.h = root.value.offsetHeight
  })
  observer.observe(root.value!)
})
onBeforeUnmount(() => observer?.disconnect())

let drag: {x: number; y: number; px: number; py: number} | null = null
function onGripDown(e: PointerEvent) {
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  drag = {x: e.clientX, y: e.clientY, px: clamped.value.x, py: clamped.value.y}
  e.preventDefault()
}
function onGripMove(e: PointerEvent) {
  if (!drag) return
  pos.x = drag.px + e.clientX - drag.x
  pos.y = drag.py + e.clientY - drag.y
}
function onGripUp() {
  if (!drag) return
  drag = null
  Object.assign(pos, clamped.value)
  save()
}
function toggleOrientation() {
  pos.horizontal = !pos.horizontal
  save()
}
watch(() => pos.horizontal, () => requestAnimationFrame(() => Object.assign(pos, clamped.value)))
</script>

<template>
  <div
    ref="root" class="tool-palette" :class="{horizontal: pos.horizontal}" data-testid="tool-palette"
    :style="{left: clamped.x + 'px', top: clamped.y + 'px'}"
    role="toolbar" aria-label="ツール" :aria-orientation="pos.horizontal ? 'horizontal' : 'vertical'"
    @pointerdown.stop @wheel.stop @dblclick.stop
  >
    <div
      class="grip" data-testid="tool-palette-grip"
      v-tooltip="{content: 'ドラッグで移動・ダブルクリックで縦／横', placement: pos.horizontal ? 'bottom' : 'right'}"
      @pointerdown="onGripDown" @pointermove="onGripMove" @pointerup="onGripUp" @pointercancel="onGripUp"
      @dblclick="toggleOrientation"
    >
      <svg viewBox="0 0 12 6" aria-hidden="true"><circle v-for="i in 6" :key="i" :cx="1.5 + ((i - 1) % 3) * 4.5" :cy="i <= 3 ? 1.2 : 4.8" r="1.1" /></svg>
    </div>
    <InputButtonToggle
      v-for="t in tools" :key="t.id" v-tooltip="{content: `${t.label}（${t.key}）`, placement: pos.horizontal ? 'bottom' : 'right'}"
      :model-value="active === t.id" :icon="t.icon" :aria-label="t.label" :data-tool="t.id"
      @update:model-value="emit('select', t.id)"
    />
  </div>
</template>

<style scoped>
.tool-palette {
  position: absolute; z-index: 5; display: flex; flex-direction: column; align-items: center; gap: 2px;
  padding: 3px 4px 4px; border-radius: 6px;
  background: color-mix(in srgb, var(--tq-color-background) 88%, transparent);
  -webkit-backdrop-filter: blur(12px); backdrop-filter: blur(12px);
  border: 1px solid var(--tq-color-border);
  box-shadow: 0 2px 8px rgba(0, 0, 0, .12), 0 0 1px rgba(0, 0, 0, .1);
  cursor: default;
}
.tool-palette.horizontal {flex-direction: row; padding: 4px 4px 4px 3px}
.grip {
  display: grid; place-items: center; width: 26px; height: 10px; border-radius: 3px; cursor: grab;
  color: var(--tq-color-text-mute); touch-action: none;
}
.horizontal .grip {width: 10px; height: 26px}
.horizontal .grip svg {transform: rotate(90deg)}
.grip:hover {background: var(--tq-color-input-hover)}
.grip:active {cursor: grabbing}
.grip svg {width: 12px; height: 6px; fill: currentColor}
.tool-palette :deep(.TqInputButtonToggle) {width: 28px; height: 28px; min-width: 28px; font-size: 16px; justify-content: center; border-radius: 4px}
</style>
