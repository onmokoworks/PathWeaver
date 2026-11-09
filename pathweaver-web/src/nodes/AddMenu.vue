<script setup lang="ts">
// Add menu at the pointer: type to search (Japanese or English), ↑↓ Enter.
// Click an item to add it (it then follows the pointer), or drag an item out
// and release it over a link to insert it there. Always fits in the editor.
import {computed, nextTick, onMounted, ref} from 'vue'
import type {NodeKind, Vec2} from '../core/types'
import {CATEGORIES, categoryOf, searchKinds} from './catalog'

const props = defineProps<{at: Vec2; bounds: {width: number; height: number}; hint?: string}>()
const emit = defineEmits<{pick: [kind: NodeKind]; drag: [kind: NodeKind, e: PointerEvent]; close: []}>()
const query = ref('')
const index = ref(0)
const input = ref<HTMLInputElement>()
const root = ref<HTMLDivElement>()
const size = ref({w: 220, h: 340})
const results = computed(() => searchKinds(query.value))
// While searching, the category holding the best match comes first (Enter picks it).
const grouped = computed(() => {
  const groups = CATEGORIES.map(c => ({...c, items: results.value.filter(r => c.kinds.includes(r.kind))})).filter(c => c.items.length)
  if (!query.value.trim()) return groups
  const rank = (g: typeof groups[number]) => Math.min(...g.items.map(i => results.value.indexOf(i)))
  return groups.sort((a, b) => rank(a) - rank(b))
})
const flat = computed(() => grouped.value.flatMap(g => g.items))
onMounted(() => nextTick(() => {
  input.value?.focus()
  if (root.value) size.value = {w: root.value.offsetWidth, h: root.value.offsetHeight}
}))
function onKey(e: KeyboardEvent) {
  e.stopPropagation()
  if (e.key === 'ArrowDown') { index.value = Math.min(flat.value.length - 1, index.value + 1); e.preventDefault() }
  else if (e.key === 'ArrowUp') { index.value = Math.max(0, index.value - 1); e.preventDefault() }
  else if (e.key === 'Enter' && !e.isComposing) { const k = flat.value[index.value]; if (k) emit('pick', k.kind) }
  else if (e.key === 'Escape') emit('close')
}
// Fit inside the editor: shift left/up instead of running off the bottom.
const maxHeight = computed(() => Math.max(160, Math.min(340, props.bounds.height - 16)))
const style = computed(() => {
  const h = Math.min(size.value.h, maxHeight.value)
  const x = Math.max(8, Math.min(props.at.x, props.bounds.width - size.value.w - 8))
  const y = Math.max(8, Math.min(props.at.y, props.bounds.height - h - 8))
  return {left: `${x}px`, top: `${y}px`, maxHeight: `${maxHeight.value}px`}
})
// Press on an item and move: drag it out as a new node.
let press: {kind: NodeKind; x: number; y: number} | null = null
function onItemDown(e: PointerEvent, kind: NodeKind) { press = {kind, x: e.clientX, y: e.clientY} }
function onItemMove(e: PointerEvent) {
  if (!press || !(e.buttons & 1)) return
  if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return
  const kind = press.kind
  press = null
  emit('drag', kind, e)
}
</script>

<template>
  <div ref="root" class="add-menu" :style="style" data-testid="add-menu" @pointerdown.stop @wheel.stop @pointermove="onItemMove" @pointerup="press = null">
    <div v-if="hint" class="hint">{{ hint }}</div>
    <input ref="input" v-model="query" class="search" placeholder="ノードを検索（項目はドラッグでも追加）" @input="index = 0" @keydown="onKey">
    <div class="list">
      <template v-for="g in grouped" :key="g.id">
        <div class="group" :style="{'--cat': g.color}">{{ g.label }}</div>
        <button
          v-for="k in g.items" :key="k.kind" type="button" class="item" :data-kind="k.kind"
          :class="{active: flat[index]?.kind === k.kind}" :style="{'--cat': categoryOf(k.kind).color}"
          @pointerenter="index = flat.indexOf(k)" @pointerdown="onItemDown($event, k.kind)" @click="emit('pick', k.kind)"
        >
          <span class="dot" />{{ k.label }}<span class="en">{{ k.kind }}</span>
        </button>
      </template>
      <div v-if="!flat.length" class="none">該当するノードはありません</div>
    </div>
  </div>
</template>

<style scoped>
.add-menu {
  position: absolute; z-index: 20; width: 220px; display: flex; flex-direction: column;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 4px;
  box-shadow: 0 4px 16px var(--tq-color-shadow, rgba(0,0,0,.2)); overflow: hidden; font-size: 12px;
}
.hint {padding: 6px 8px 0; font-size: 10px; color: var(--tq-color-text-mute)}
.search {
  margin: 6px; padding: 4px 8px; height: 24px; border: 0; border-radius: 3px; outline: none;
  background: var(--tq-color-input); color: var(--tq-color-text); font: inherit;
}
.search:focus {box-shadow: 0 0 0 1px var(--tq-color-accent)}
.list {overflow-y: auto; padding: 0 4px 6px; min-height: 0}
.group {padding: 6px 6px 2px; font-size: 10px; font-weight: 600; color: var(--tq-color-text-mute)}
.item {
  display: flex; align-items: center; gap: 6px; width: 100%; padding: 4px 6px; border: 0; border-radius: 3px;
  background: transparent; color: var(--tq-color-text); font: inherit; text-align: left; cursor: pointer;
}
.item.active {background: var(--tq-color-accent); color: var(--tq-color-on-accent)}
.dot {width: 6px; height: 6px; border-radius: 1px; background: var(--cat); flex: none}
.en {margin-left: auto; font-size: 10px; opacity: .6}
.none {padding: 10px; color: var(--tq-color-text-mute)}
</style>
