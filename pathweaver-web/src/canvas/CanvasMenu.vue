<script setup lang="ts">
// Right-click menu at the pointer: what can be done to what is under it /
// selected. Click an item or press Enter; Esc or a click outside closes.
import {computed, nextTick, onBeforeUnmount, onMounted, ref} from 'vue'

export interface MenuItem {label: string; shortcut?: string; action?: () => void; disabled?: boolean; separator?: boolean}
const props = defineProps<{at: {x: number; y: number}; bounds: {width: number; height: number}; items: MenuItem[]}>()
const emit = defineEmits<{close: []}>()
const root = ref<HTMLElement>()
const size = ref({w: 220, h: 300})
const index = ref(-1)
const live = computed(() => props.items.map((it, i) => ({it, i})).filter(x => !x.it.separator && !x.it.disabled))
const style = computed(() => ({
  left: `${Math.max(4, Math.min(props.at.x, props.bounds.width - size.value.w - 4))}px`,
  top: `${Math.max(4, Math.min(props.at.y, props.bounds.height - size.value.h - 4))}px`,
}))
function run(it: MenuItem) {
  if (it.disabled || it.separator) return
  emit('close')
  it.action?.()
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') { emit('close'); e.preventDefault(); e.stopPropagation(); return }
  const n = live.value.length
  if (!n) return
  const pos = live.value.findIndex(x => x.i === index.value)
  if (e.key === 'ArrowDown') index.value = live.value[(pos + 1) % n].i
  else if (e.key === 'ArrowUp') index.value = live.value[(pos - 1 + n) % n].i
  else if (e.key === 'Enter' && index.value >= 0) run(props.items[index.value])
  else return
  e.preventDefault(); e.stopPropagation()
}
const outside = (e: PointerEvent) => { if (!root.value?.contains(e.target as Node)) emit('close') }
onMounted(() => {
  window.addEventListener('keydown', onKey, true)
  window.addEventListener('pointerdown', outside, true)
  nextTick(() => { if (root.value) size.value = {w: root.value.offsetWidth, h: root.value.offsetHeight} })
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey, true)
  window.removeEventListener('pointerdown', outside, true)
})
</script>

<template>
  <div ref="root" class="canvas-menu" role="menu" data-testid="canvas-menu" :style="style" @pointerdown.stop @contextmenu.prevent>
    <template v-for="(it, i) in items" :key="i">
      <div v-if="it.separator" class="sep" role="separator" />
      <button
        v-else type="button" role="menuitem" class="item" :class="{active: index === i}" :disabled="it.disabled"
        @pointerenter="index = it.disabled ? -1 : i" @click="run(it)"
      >
        <span class="label">{{ it.label }}</span><span v-if="it.shortcut" class="key">{{ it.shortcut }}</span>
      </button>
    </template>
  </div>
</template>

<style scoped>
.canvas-menu {
  position: absolute; z-index: 30; min-width: 200px; padding: 4px; display: flex; flex-direction: column;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 6px 20px rgba(0, 0, 0, .18); font-size: 12px;
}
.item {
  display: flex; align-items: center; gap: 16px; padding: 4px 8px; border: 0; border-radius: 3px;
  background: transparent; color: var(--tq-color-text); font: inherit; text-align: left; cursor: pointer;
}
.item.active {background: var(--tq-color-accent); color: var(--tq-color-on-accent)}
.item:disabled {opacity: .35; cursor: default}
.label {flex: 1}
.key {font-size: 11px; opacity: .65; font-variant-numeric: tabular-nums}
.sep {height: 1px; margin: 4px 6px; background: var(--tq-color-border)}
</style>
