<script setup lang="ts">
// The artboards panel (Illustrator's): every artboard in order — the number
// is also the export order. Click to make one active, double-click the name to
// rename, drag to reorder, ⤢ to bring it into view. Each change is one undo.
import {computed, nextTick, ref} from 'vue'
import {Icon} from '../../vendor/tweeq/src/Icon'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const ed = props.ed
const rows = computed(() => { void ed.revision; return (ed.doc.artboards ?? []).map(a => ({...a})) })
const active = computed(() => props.canvas.state.artboard)

function select(id: number) { props.canvas.state.artboard = id }
function show(id: number) { select(id); props.canvas.showArtboard(id) }
function add() {
  const v = props.canvas.view
  const c = props.canvas.toWorld({x: v.width / 2, y: v.height / 2})
  const last = rows.value.at(-1)
  // next to the last one (same size), like Illustrator's new artboard; else A4 at the view's centre
  const frame = last ? {x: last.x + last.w + 40, y: last.y, w: last.w, h: last.h} : {x: Math.round(c.x - 297), y: Math.round(c.y - 421), w: 595, h: 842}
  const id = ed.edit(d => {
    const id = ed.nextId()
    ;(d.artboards ??= []).push({id, name: `アートボード ${d.artboards.length + 1}`, ...frame})
    return id
  })
  if (id != null) show(id)
}
function remove() {
  if (active.value != null) props.canvas.deleteArtboard(active.value)
}

// ---- rename ----
const renaming = ref<number | null>(null)
const draft = ref('')
const input = ref<HTMLInputElement[]>()
function startRename(id: number, name: string) {
  renaming.value = id
  draft.value = name
  nextTick(() => { input.value?.[0]?.focus(); input.value?.[0]?.select() })
}
function finishRename(ok: boolean) {
  const id = renaming.value
  renaming.value = null
  const name = draft.value.trim()
  if (!ok || id == null || !name) return
  ed.edit(d => { const a = d.artboards?.find(b => b.id === id); if (a) a.name = name })
}

// ---- reorder (drag a row) ----
const dragging = ref<number | null>(null)
const dropAt = ref<number | null>(null)   // index the row will take
function onDragStart(e: DragEvent, id: number) {
  dragging.value = id
  e.dataTransfer?.setData('text/plain', String(id))
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}
function onDragOver(e: DragEvent, i: number) {
  if (dragging.value == null) return
  e.preventDefault()
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dropAt.value = e.clientY < r.top + r.height / 2 ? i : i + 1
}
function onDrop() {
  const id = dragging.value, at = dropAt.value
  dragging.value = dropAt.value = null
  if (id == null || at == null) return
  const from = rows.value.findIndex(a => a.id === id)
  const to = at > from ? at - 1 : at
  if (from < 0 || to === from) return
  ed.edit(d => {
    const list = d.artboards ?? []
    const [a] = list.splice(from, 1)
    list.splice(to, 0, a)
  })
}
function onDragEnd() { dragging.value = dropAt.value = null }
</script>

<template>
  <div class="artboard-list" data-testid="artboard-list">
    <ul v-if="rows.length" @dragleave.self="dropAt = null">
      <li
        v-for="(a, i) in rows" :key="a.id" :data-artboard-item="a.id" :class="{on: a.id === active, dragging: dragging === a.id, 'drop-before': dropAt === i, 'drop-after': dropAt === i + 1 && i === rows.length - 1}"
        :draggable="renaming !== a.id" @click="select(a.id)" @dragstart="onDragStart($event, a.id)" @dragover="onDragOver($event, i)" @drop.prevent="onDrop" @dragend="onDragEnd"
      >
        <span class="num" title="ダブルクリックで表示" @dblclick.stop="show(a.id)">{{ i + 1 }}</span>
        <input
          v-if="renaming === a.id" ref="input" v-model="draft" class="rename" data-testid="artboard-rename" @click.stop
          @keydown.enter.prevent="finishRename(true)" @keydown.esc.prevent.stop="finishRename(false)" @blur="finishRename(true)"
        >
        <span v-else class="name" title="ダブルクリックで名前を変える" @dblclick.stop="startRename(a.id, a.name)">{{ a.name }}</span>
        <span class="size">{{ Math.round(a.w) }}×{{ Math.round(a.h) }}</span>
        <button type="button" class="show" title="このアートボードを表示" aria-label="表示" @click.stop="show(a.id)"><Icon icon="mdi:crop-free" /></button>
      </li>
    </ul>
    <div v-else class="empty">
      <p>アートボードはまだありません。書き出す範囲をページとして決めておけます。</p>
    </div>
    <div class="foot">
      <button type="button" data-testid="artboard-list-add" title="新しいアートボード（最後のものの右に同じ大きさで）" @click="add"><Icon icon="material-symbols:add" />追加</button>
      <button type="button" title="アートボードツール（⇧O）：ドラッグで作る・動かす・大きさ" @click="canvas.setTool('artboard')"><Icon icon="mdi:artboard" />ツール</button>
      <span class="spacer" />
      <button type="button" data-testid="artboard-list-delete" title="選んでいるアートボードを削除" aria-label="削除" :disabled="active == null || !rows.some(a => a.id === active)" @click="remove"><Icon icon="material-symbols:delete-outline" /></button>
    </div>
  </div>
</template>

<style scoped>
.artboard-list {display: flex; flex-direction: column; min-height: 0; flex: 1}
ul {list-style: none; margin: 0; padding: 0 0 4px; overflow-y: auto; flex: 1; min-height: 0}
li {
  position: relative; display: flex; align-items: center; gap: 8px; height: 26px; padding: 0 6px 0 8px; cursor: default;
  font-size: 12px; color: var(--tq-color-text);
}
li:hover {background: var(--tq-color-input-hover)}
li.on {background: color-mix(in srgb, var(--tq-color-text) 9%, transparent)}
li.on .name {font-weight: 500}
li.dragging {opacity: .4}
li.drop-before::before, li.drop-after::after {content: ''; position: absolute; left: 6px; right: 6px; height: 2px; background: var(--tq-color-accent)}
li.drop-before::before {top: -1px}
li.drop-after::after {bottom: -1px}
.num {width: 18px; flex: none; text-align: right; color: var(--tq-color-text-mute); font-variant-numeric: tabular-nums; cursor: zoom-in}
.name {flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap}
.rename {flex: 1; min-width: 0; height: 20px; border: 0; padding: 0 4px; font: inherit; color: var(--tq-color-text); background: var(--tq-color-input); outline: 1px solid var(--tq-color-accent)}
.size {color: var(--tq-color-text-mute); font-variant-numeric: tabular-nums}
.show {display: flex; padding: 2px; border: 0; background: transparent; color: var(--tq-color-text-mute); cursor: pointer; border-radius: 3px; opacity: 0}
li:hover .show, li.on .show, .show:focus-visible {opacity: 1}
.show:hover {color: var(--tq-color-text); background: var(--tq-color-input)}
.empty {padding: 8px 12px; flex: 1}
.empty p {margin: 0; font-size: 12px; color: var(--tq-color-text-mute); line-height: 1.6}
.foot {display: flex; align-items: center; gap: 2px; padding: 4px 6px; border-top: 1px solid var(--tq-color-border); flex: none}
.foot button {display: flex; align-items: center; gap: 4px; height: 22px; padding: 0 6px; border: 0; border-radius: 3px; background: transparent; font: inherit; font-size: 11px; color: var(--tq-color-text-mute); cursor: pointer}
.foot button:hover {background: var(--tq-color-input-hover); color: var(--tq-color-text)}
.foot button:disabled {opacity: .35; pointer-events: none}
.spacer {flex: 1}
</style>
