<script setup lang="ts">
// Add menu, as in Blender: categories first, hover one (or →) to open its
// node types beside it; type to search instead (Japanese or English, ↑↓ Enter).
// Click an item to add it (it then follows the pointer until you click), or
// drag an item out and release it over a link to insert it there.
import {computed, nextTick, onMounted, ref, watch} from 'vue'
import type {NodeKind, Vec2} from '../core/types'
import {CATEGORIES, categoryOf, kindList, searchKinds} from './catalog'

const props = defineProps<{at: Vec2; bounds: {width: number; height: number}; hint?: string; title?: string; allow?: (k: NodeKind) => boolean}>()
const emit = defineEmits<{pick: [kind: NodeKind]; drag: [kind: NodeKind, e: PointerEvent]; close: []}>()
const query = ref('')
const index = ref(0)
const input = ref<HTMLInputElement>()
const root = ref<HTMLDivElement>()
const size = ref({w: 200, h: 240})
const searching = computed(() => query.value.trim() !== '')

// ---- browse: category → its node types ----
const label = (k: NodeKind) => kindList().find(v => v.kind === k)?.label ?? k
// Input and Output exist once per graph; they are never added from here.
const ok = (k: NodeKind) => k !== 'Input' && k !== 'Output' && (!props.allow || props.allow(k))
const cats = CATEGORIES.map(c => ({...c, items: c.kinds.filter(ok).map(kind => ({kind, label: label(kind)}))})).filter(c => c.items.length)
const cat = ref(-1)        // open category
const inSub = ref(false)   // keyboard focus is in the submenu
const subIndex = ref(0)
const catRows = ref<HTMLElement[]>([])
const sub = ref<HTMLDivElement>()
const subSize = ref({w: 190, h: 200})
watch(cat, () => nextTick(() => { if (sub.value) subSize.value = {w: sub.value.offsetWidth, h: sub.value.offsetHeight} }))

// ---- search: grouped results, best category first ----
const results = computed(() => searchKinds(query.value).filter(r => ok(r.kind)))
const grouped = computed(() => {
  const groups = CATEGORIES.map(c => ({...c, items: results.value.filter(r => c.kinds.includes(r.kind))})).filter(c => c.items.length)
  const rank = (g: typeof groups[number]) => Math.min(...g.items.map(i => results.value.indexOf(i)))
  return groups.sort((a, b) => rank(a) - rank(b))
})
const flat = computed(() => grouped.value.flatMap(g => g.items))

onMounted(() => nextTick(() => {
  input.value?.focus()
  if (root.value) size.value = {w: root.value.offsetWidth, h: root.value.offsetHeight}
}))
watch(searching, () => nextTick(() => { if (root.value) size.value = {w: root.value.offsetWidth, h: root.value.offsetHeight} }))

function onKey(e: KeyboardEvent) {
  e.stopPropagation()
  if (e.key === 'Escape') { if (inSub.value) { inSub.value = false } else emit('close'); e.preventDefault(); return }
  if (e.isComposing) return
  if (searching.value) {
    if (e.key === 'ArrowDown') { index.value = Math.min(flat.value.length - 1, index.value + 1); e.preventDefault() }
    else if (e.key === 'ArrowUp') { index.value = Math.max(0, index.value - 1); e.preventDefault() }
    else if (e.key === 'Enter') { const k = flat.value[index.value]; if (k) emit('pick', k.kind) }
    return
  }
  const items = cat.value >= 0 ? cats[cat.value].items : []
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    const d = e.key === 'ArrowDown' ? 1 : -1
    if (inSub.value) subIndex.value = (subIndex.value + d + items.length) % items.length
    else { cat.value = cat.value < 0 ? 0 : (cat.value + d + cats.length) % cats.length; subIndex.value = 0 }
    e.preventDefault()
  } else if (e.key === 'ArrowRight' && cat.value >= 0) { inSub.value = true; e.preventDefault() }
  else if (e.key === 'ArrowLeft') { inSub.value = false; e.preventDefault() }
  else if (e.key === 'Enter') {
    if (inSub.value && items[subIndex.value]) emit('pick', items[subIndex.value].kind)
    else if (cat.value >= 0) { inSub.value = true; subIndex.value = 0 }
  }
}
function hoverCat(i: number) {
  if (cat.value !== i) subIndex.value = 0
  cat.value = i
  inSub.value = false
}

// Fit inside the editor: shift left/up instead of running off the edges.
const maxHeight = computed(() => Math.max(160, Math.min(360, props.bounds.height - 16)))
const pos = computed(() => {
  const h = Math.min(size.value.h, maxHeight.value)
  return {
    x: Math.max(8, Math.min(props.at.x, props.bounds.width - size.value.w - 8)),
    y: Math.max(8, Math.min(props.at.y, props.bounds.height - h - 8)),
  }
})
const style = computed(() => ({left: `${pos.value.x}px`, top: `${pos.value.y}px`, maxHeight: `${maxHeight.value}px`}))
// The submenu opens to the right of its row, or to the left when there is no room.
const subStyle = computed(() => {
  const row = catRows.value[cat.value]
  const top = pos.value.y + (row?.offsetTop ?? 0) - 4
  const h = Math.min(subSize.value.h, maxHeight.value)
  const right = pos.value.x + size.value.w - 2
  const x = right + subSize.value.w + 8 <= props.bounds.width ? right : pos.value.x - subSize.value.w + 2
  return {left: `${Math.max(8, x)}px`, top: `${Math.max(8, Math.min(top, props.bounds.height - h - 8))}px`, maxHeight: `${maxHeight.value}px`}
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
  <div class="add-layer" data-testid="add-menu" @pointerdown.stop @wheel.stop @pointermove="onItemMove" @pointerup="press = null">
    <div ref="root" class="add-menu" :style="style">
      <div v-if="title" class="title">{{ title }}</div>
      <div v-if="hint" class="hint">{{ hint }}</div>
      <input ref="input" v-model="query" class="search" placeholder="検索…" aria-label="ノードを検索" @input="index = 0" @keydown="onKey">
      <div v-if="!searching" class="list" role="menu">
        <button
          v-for="(c, i) in cats" :key="c.id" ref="catRows" type="button" class="item cat" role="menuitem" aria-haspopup="menu"
          :data-category="c.id" :class="{open: cat === i, active: cat === i && !inSub}" :style="{'--cat': c.color}"
          @pointerenter="hoverCat(i)" @click="hoverCat(i)"
        >
          <span class="dot" /><span>{{ c.label }}</span><span class="chev">›</span>
        </button>
      </div>
      <div v-else class="list">
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
    <div v-if="!searching && cat >= 0" ref="sub" class="add-menu sub" role="menu" :aria-label="cats[cat].label" :style="subStyle">
      <button
        v-for="(k, i) in cats[cat].items" :key="k.kind" type="button" class="item" role="menuitem" :data-kind="k.kind"
        :class="{active: inSub && subIndex === i}" :style="{'--cat': cats[cat].color}"
        @pointerenter="inSub = true; subIndex = i" @pointerdown="onItemDown($event, k.kind)" @click="emit('pick', k.kind)"
      >
        <span class="dot" />{{ k.label }}<span class="en">{{ k.kind }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.add-layer {position: absolute; inset: 0; z-index: 20; pointer-events: none}
.add-menu {
  position: absolute; width: 200px; display: flex; flex-direction: column; pointer-events: auto;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 4px;
  box-shadow: 0 4px 16px var(--tq-color-shadow, rgba(0,0,0,.2)); overflow: hidden; font-size: 12px;
}
.add-menu.sub {width: auto; min-width: 170px; padding: 4px; overflow-y: auto}
.title {padding: 6px 8px 0; font-size: 11px; font-weight: 500; color: var(--tq-color-text-mute)}
.hint {padding: 6px 8px 0; font-size: 10px; color: var(--tq-color-text-mute)}
.search {
  margin: 6px; padding: 4px 8px; height: 24px; border: 0; border-radius: 3px; outline: none;
  background: var(--tq-color-input); color: var(--tq-color-text); font: inherit;
}
.search:focus {box-shadow: 0 0 0 1px var(--tq-color-accent)}
.list {overflow-y: auto; padding: 0 4px 6px; min-height: 0}
.group {padding: 6px 6px 2px; font-size: 10px; font-weight: 500; color: var(--tq-color-text-mute)}
.item {
  display: flex; align-items: center; gap: 6px; width: 100%; padding: 4px 6px; border: 0; border-radius: 3px;
  background: transparent; color: var(--tq-color-text); font: inherit; text-align: left; cursor: pointer; white-space: nowrap;
}
.item.cat.open {background: var(--tq-color-input-hover)}
.item.active {background: var(--tq-color-accent); color: var(--tq-color-on-accent)}
.dot {width: 6px; height: 6px; border-radius: 1px; background: var(--cat); flex: none}
.en {margin-left: auto; padding-left: 12px; font-size: 10px; opacity: .6}
.chev {margin-left: auto; opacity: .6}
.none {padding: 10px; color: var(--tq-color-text-mute)}
</style>
