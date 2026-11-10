<script setup lang="ts">
// Two docked areas with a divider, after Blender's areas:
// - dragging the divider resizes; dragging an area almost shut folds it to a
//   thin strip with its title (the panel stays mounted, its view is kept);
// - click the strip, or drag it out, to unfold to the previous size;
// - double-click the divider folds / unfolds the sized area;
// - `max` shows one side alone (Ctrl+Space in the app).
import {computed, ref} from 'vue'
import {area, type Side} from '../core/layout'
import {Icon} from '../../vendor/tweeq/src/Icon'

const props = withDefaults(defineProps<{
  name: string
  direction: 'horizontal' | 'vertical'
  fixed: Side                  // the side whose size is stored
  titles: [string, string]
  min?: number
  max?: Side | null
  hide?: Side | null           // a side whose panel floats elsewhere
}>(), {min: 120, max: null, hide: null})

const STRIP = 22
const st = computed(() => area(props.name))
const root = ref<HTMLElement>()
const horizontal = computed(() => props.direction === 'horizontal')
const other = (s: Side): Side => (s === 'first' ? 'second' : 'first')

const shown = (s: Side) => props.hide !== s && (props.max === null || props.max === s)
const folded = (s: Side) => props.max !== s && st.value[s]
const showDivider = computed(() => shown('first') && shown('second') && !folded('first') && !folded('second'))

function paneStyle(s: Side) {
  const dim = horizontal.value ? 'width' : 'height'
  if (!shown(s)) return {display: 'none'}
  if (!shown(other(s))) return {flex: '1 1 0'}
  if (folded(s)) return {flex: `0 0 ${STRIP}px`, [dim]: `${STRIP}px`}
  if (folded(other(s))) return {flex: '1 1 0'}
  if (s === props.fixed) return {flex: `0 1 ${st.value.size}px`, [dim]: `${st.value.size}px`}
  return {flex: '1 1 0'}
}

// ---- divider / strip drag ----
let drag: {start: number; size: number; moved: boolean; side: Side} | null = null
const total = () => (horizontal.value ? root.value?.clientWidth : root.value?.clientHeight) ?? 800
function down(e: PointerEvent, side: Side = props.fixed) {
  if (e.button !== 0) return
  // Follow the pointer on the window: the strip disappears when it unfolds.
  const onMove = (ev: PointerEvent) => move(ev)
  const onUp = () => { window.removeEventListener('pointermove', onMove); window.removeEventListener('pointerup', onUp); window.removeEventListener('pointercancel', onUp); up(side) }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
  const size = st.value[side] ? STRIP : side === props.fixed ? st.value.size : total() - st.value.size
  drag = {start: horizontal.value ? e.clientX : e.clientY, size, moved: false, side}
  e.preventDefault()
}
function move(e: PointerEvent) {
  if (!drag) return
  const d = (horizontal.value ? e.clientX : e.clientY) - drag.start
  if (Math.abs(d) > 3) drag.moved = true
  if (!drag.moved) return
  // Size of the dragged side: grows when the divider moves away from it.
  const sign = drag.side === 'first' ? 1 : -1
  const want = drag.size + sign * d
  const side = drag.side
  if (want < props.min * 0.5) { st.value[side] = true; return }
  st.value[side] = false
  const size = Math.max(props.min, Math.min(want, total() - props.min))
  if (side === props.fixed) st.value.size = size
  else st.value.size = Math.max(props.min, total() - size)
}
function up(side: Side) {
  if (!drag) return
  const clicked = !drag.moved
  drag = null
  if (clicked && st.value[side]) st.value[side] = false // a click on a strip unfolds
}
function toggle() { st.value[props.fixed] = !st.value[props.fixed] }
</script>

<template>
  <div ref="root" class="area-split" :class="direction" :data-area-split="name">
    <template v-for="s in (['first', 'second'] as Side[])" :key="s">
      <div v-if="s === 'second' && showDivider" class="divider" title="ドラッグで大きさ・端まで寄せるとたたむ・ダブルクリックでたたむ／戻す"
        @pointerdown="down($event)" @dblclick="toggle" />
      <div class="pane" :class="[s, {folded: folded(s)}]" :style="paneStyle(s)">
        <div v-show="!folded(s)" class="content"><slot :name="s" /></div>
        <button
          v-if="folded(s)" type="button" class="strip" :data-strip="titles[s === 'first' ? 0 : 1]"
          :aria-label="`${titles[s === 'first' ? 0 : 1]}を開く`" title="クリックで開く・ドラッグで好きな大きさに"
          @pointerdown="down($event, s)"
        >
          <Icon :icon="horizontal ? (s === 'first' ? 'material-symbols:chevron-right' : 'material-symbols:chevron-left') : (s === 'first' ? 'material-symbols:expand-more' : 'material-symbols:expand-less')" />
          <span class="title">{{ titles[s === 'first' ? 0 : 1] }}</span>
        </button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.area-split {display: flex; width: 100%; height: 100%; min-width: 0; min-height: 0}
.area-split.vertical {flex-direction: column}
.pane {position: relative; min-width: 0; min-height: 0; overflow: hidden}
.content {width: 100%; height: 100%}
.divider {flex: none; position: relative; z-index: 10; background: var(--tq-color-border); touch-action: none}
.horizontal > .divider {width: 1px; cursor: col-resize}
.vertical > .divider {height: 1px; cursor: row-resize}
.divider::before {content: ''; position: absolute; inset: 0}
.horizontal > .divider::before {left: -4px; right: -4px}
.vertical > .divider::before {top: -4px; bottom: -4px}
.divider:hover {background: var(--tq-color-accent)}
.strip {
  display: flex; align-items: center; gap: 4px; width: 100%; height: 100%; padding: 0 8px; border: 0; margin: 0;
  background: var(--tq-color-background); color: var(--tq-color-text-mute); font: inherit; font-size: 11px; font-weight: 500;
  cursor: pointer; touch-action: none; box-shadow: inset 0 0 0 1px var(--tq-color-border);
}
.strip:hover {color: var(--tq-color-text); background: var(--tq-color-input-hover)}
.horizontal > .pane > .strip {flex-direction: column; padding: 8px 0; justify-content: flex-start}
.horizontal > .pane > .strip .title {writing-mode: vertical-rl; letter-spacing: .1em}
</style>
