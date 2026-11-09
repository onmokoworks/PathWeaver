<script setup lang="ts">
// A panel floating over the workspace: drag the top strip to move, the corner
// to resize; it stays inside the workspace and comes to the front when used.
import {computed} from 'vue'
import {layout, PANEL_TITLES, raise, setFloating, type PanelId} from '../core/layout'
import {Icon} from '../../vendor/tweeq/src/Icon'

const props = defineProps<{id: PanelId; bounds: {width: number; height: number}}>()
const place = computed(() => layout[props.id])
const MIN_W = 200, MIN_H = 120

const rect = computed(() => {
  const p = place.value
  const w = Math.max(MIN_W, Math.min(p.w, props.bounds.width - 8))
  const h = Math.max(MIN_H, Math.min(p.h, props.bounds.height - 8))
  return {
    x: Math.max(4, Math.min(p.x, props.bounds.width - w - 4)),
    y: Math.max(4, Math.min(p.y, props.bounds.height - h - 4)),
    w, h,
  }
})

let drag: {mode: 'move' | 'size'; x: number; y: number; r: {x: number; y: number; w: number; h: number}} | null = null
function down(e: PointerEvent, mode: 'move' | 'size') {
  if ((e.target as HTMLElement).closest('button')) return
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  drag = {mode, x: e.clientX, y: e.clientY, r: {...rect.value}}
  raise(props.id)
  e.preventDefault()
}
function move(e: PointerEvent) {
  if (!drag) return
  const dx = e.clientX - drag.x, dy = e.clientY - drag.y
  const p = place.value
  if (drag.mode === 'move') { p.x = drag.r.x + dx; p.y = drag.r.y + dy }
  else { p.w = Math.max(MIN_W, drag.r.w + dx); p.h = Math.max(MIN_H, drag.r.h + dy) }
}
function up() {
  if (!drag) return
  drag = null
  Object.assign(place.value, rect.value) // keep what is actually shown
}
</script>

<template>
  <section
    class="floating" :data-floating="id" role="dialog" :aria-label="PANEL_TITLES[id]"
    :style="{left: rect.x + 'px', top: rect.y + 'px', width: rect.w + 'px', height: rect.h + 'px', zIndex: 10 + place.z}"
    @pointerdown.capture="raise(id)"
  >
    <header class="bar" data-testid="floating-bar" @pointerdown="down($event, 'move')" @pointermove="move" @pointerup="up" @pointercancel="up">
      <span class="title">{{ PANEL_TITLES[id] }}</span>
      <button type="button" class="dock" :aria-label="`${PANEL_TITLES[id]}を元の場所に戻す`" title="元の場所に戻す" @click="setFloating(id, false)">
        <Icon icon="material-symbols:close-fullscreen" />
      </button>
    </header>
    <div class="content"><slot /></div>
    <div class="resize" aria-hidden="true" @pointerdown="down($event, 'size')" @pointermove="move" @pointerup="up" @pointercancel="up" />
  </section>
</template>

<style scoped>
.floating {
  position: absolute; display: flex; flex-direction: column; overflow: hidden;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 6px 24px rgba(0, 0, 0, .18), 0 0 1px rgba(0, 0, 0, .2);
}
.bar {
  display: flex; align-items: center; height: 24px; padding: 0 4px 0 10px; flex: none; cursor: grab;
  font-size: 11px; font-weight: 600; color: var(--tq-color-text-mute); border-bottom: 1px solid var(--tq-color-border);
  touch-action: none;
}
.bar:active {cursor: grabbing}
.title {flex: 1}
.dock {
  display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 50%;
  background: transparent; color: var(--tq-color-text-mute); cursor: pointer; font-size: 14px;
}
.dock:hover {background: var(--tq-color-input-hover); color: var(--tq-color-text)}
.content {flex: 1; min-height: 0; position: relative; overflow: hidden}
.resize {position: absolute; right: 0; bottom: 0; width: 14px; height: 14px; cursor: nwse-resize; touch-action: none;
  background: linear-gradient(135deg, transparent 50%, var(--tq-color-border) 50%, var(--tq-color-border) 60%, transparent 60%, transparent 75%, var(--tq-color-border) 75%, var(--tq-color-border) 85%, transparent 85%)}
</style>
