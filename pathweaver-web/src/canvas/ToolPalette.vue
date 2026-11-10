<script setup lang="ts">
// Floating tool palette over the canvas. Drag the grip to move it,
// double-click the grip to switch vertical / horizontal. Position and
// orientation are a per-viewer preference (localStorage).
import {computed, onBeforeUnmount, onMounted, reactive, ref, watch} from 'vue'
import InputButtonToggle from '../../vendor/tweeq/src/InputButtonToggle/InputButtonToggle.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import {Icon} from '../../vendor/tweeq/src/Icon'
import type {Tool} from '../core/editor'

const props = defineProps<{
  tools: {id: Tool; icon: string; label: string; key: string}[]
  active: Tool
  bounds: {width: number; height: number}
  /** Shape tool group (Illustrator's flyout): every kind, and the current one. */
  shapes?: {kind: string; label: string; icon: string; key?: string}[]
  shapeKind?: string
}>()
const emit = defineEmits<{select: [tool: Tool]; shape: [kind: string]}>()

// The shape button holds all shapes: click it again (or right-click) for the list.
const flyout = ref(false)
function onTool(id: Tool) {
  if (id === 'shape' && props.active === 'shape') { flyout.value = !flyout.value; return }
  flyout.value = false
  emit('select', id)
}
function pickShape(kind: string) { flyout.value = false; emit('shape', kind) }
const onDocDown = (e: PointerEvent) => { if (flyout.value && !root.value?.contains(e.target as Node)) flyout.value = false }
onMounted(() => document.addEventListener('pointerdown', onDocDown, true))
onBeforeUnmount(() => document.removeEventListener('pointerdown', onDocDown, true))

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
      <svg v-if="!pos.horizontal" viewBox="0 0 6 12" aria-hidden="true" data-dots="columns">
        <circle v-for="i in 6" :key="i" :cx="i <= 3 ? 1.2 : 4.8" :cy="1.5 + ((i - 1) % 3) * 4.5" r="1.1" />
      </svg>
      <svg v-else viewBox="0 0 12 6" aria-hidden="true" data-dots="rows">
        <circle v-for="i in 6" :key="i" :cx="1.5 + ((i - 1) % 3) * 4.5" :cy="i <= 3 ? 1.2 : 4.8" r="1.1" />
      </svg>
    </div>
    <div v-for="t in tools" :key="t.id" class="slot" :class="{group: t.id === 'shape'}">
      <InputButtonToggle
        v-tooltip="{content: t.id === 'shape' ? `${t.label}（${t.key}）：もう一度押すと図形の一覧` : `${t.label}（${t.key}）`, placement: pos.horizontal ? 'bottom' : 'right'}"
        :model-value="active === t.id" :icon="t.icon" :aria-label="t.label" :data-tool="t.id"
        @update:model-value="onTool(t.id)" @contextmenu.prevent="t.id === 'shape' && (flyout = true)"
      />
      <div v-if="t.id === 'shape' && flyout" class="flyout" :class="{below: pos.horizontal}" role="menu" data-testid="shape-flyout">
        <button
          v-for="sh in shapes ?? []" :key="sh.kind" type="button" role="menuitemradio" :aria-checked="sh.kind === shapeKind"
          :class="{on: sh.kind === shapeKind}" :data-shape="sh.kind" @click="pickShape(sh.kind)"
        >
          <Icon :icon="sh.icon" /><span>{{ sh.label }}</span><kbd v-if="sh.key">{{ sh.key }}</kbd>
        </button>
      </div>
    </div>
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
/* Two columns of three dots (⋮⋮) on a vertical palette, two rows on a horizontal one. */
.grip {
  display: grid; place-items: center; width: 26px; height: 16px; border-radius: 3px; cursor: grab;
  color: var(--tq-color-text-mute); touch-action: none;
}
.horizontal .grip {width: 16px; height: 26px}
.grip:hover {background: var(--tq-color-input-hover)}
.grip:active {cursor: grabbing}
.grip svg {width: 6px; height: 12px; fill: currentColor}
.horizontal .grip svg {width: 12px; height: 6px}
.slot {position: relative}
/* a group button: small corner mark, like Illustrator's tool groups */
.slot.group::after {content: ''; position: absolute; right: 2px; bottom: 2px; border: 3px solid transparent; border-right-color: currentColor; border-bottom-color: currentColor; opacity: .5; pointer-events: none}
.flyout {
  position: absolute; left: calc(100% + 8px); top: -4px; z-index: 10; display: flex; flex-direction: column; min-width: 150px; padding: 4px;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, .18);
}
.flyout.below {left: -4px; top: calc(100% + 8px)}
.flyout button {display: flex; align-items: center; gap: 8px; height: 26px; padding: 0 8px; border: 0; border-radius: 3px; background: transparent; color: var(--tq-color-text); font: inherit; font-size: 12px; text-align: left; cursor: pointer}
.flyout button:hover {background: var(--tq-color-input-hover)}
.flyout button.on {background: var(--tq-color-accent); color: var(--tq-color-on-accent)}
.flyout button span {flex: 1}
.flyout kbd {font: inherit; font-size: 10px; opacity: .6}
.tool-palette :deep(.TqInputButtonToggle) {width: 28px; height: 28px; min-width: 28px; font-size: 16px; justify-content: center; border-radius: 4px}
</style>
