<script setup lang="ts">
// Layers: front-most first. Folders group paths and nest; drag rows to
// reorder, onto a folder to put a path or folder inside, out to take it out.
// Click selects (Shift/⌘ adds), double-click renames, the eye hides.
import {computed, nextTick, ref} from 'vue'
import {Icon} from '../../vendor/tweeq/src/Icon'
import type {Editor} from '../core/editor'
import type {Folder, Stroke} from '../core/types'
import {renderBounds} from '../canvas/geometry'
import PanelPin from './PanelPin.vue'
import {layout} from '../core/layout'

const props = defineProps<{ed: Editor}>()
const ed = props.ed

type Row =
  | {type: 'folder'; key: string; folder: Folder; count: number; depth: number}
  | {type: 'path'; key: string; stroke: Stroke; depth: number}

/** Paths in list order (top first), including those in closed folders. */
const order = computed(() => {
  void ed.revision
  return [...ed.doc.strokes].reverse()
})
const rows = computed<Row[]>(() => {
  void ed.revision
  const folders = ed.doc.folders ?? []
  const byId = new Map(folders.map(f => [f.id, f]))
  /** The item directly under `parent` that holds this path (the path itself or a folder). */
  const childOf = (s: Stroke, parent: number | null): Folder | Stroke => {
    if ((s.folder ?? null) === parent) return s
    let f = byId.get(s.folder!)
    for (let n = 0; f && (f.parent ?? null) !== parent && n < 64; n++) f = byId.get(f.parent!)
    return f ?? s
  }
  const out: Row[] = []
  const level = (parent: number | null, depth: number) => {
    const members = parent == null ? order.value : order.value.filter(s => ed.inFolder(s, parent))
    // empty folders sit at the top of their level
    for (const f of folders) {
      if ((f.parent ?? null) === parent && !members.some(s => ed.inFolder(s, f.id))) out.push({type: 'folder', key: `f${f.id}`, folder: f, count: 0, depth})
    }
    const emitted = new Set<number>()
    for (const s of members) {
      const c = childOf(s, parent)
      if ('nodes' in c) { out.push({type: 'path', key: `p${s.id}`, stroke: s, depth}); continue }
      if (emitted.has(c.id)) continue
      emitted.add(c.id)
      const inside = members.filter(m => ed.inFolder(m, c.id)).length
      out.push({type: 'folder', key: `f${c.id}`, folder: c, count: inside, depth})
      if (c.open) level(c.id, depth + 1)
    }
  }
  level(null, 0)
  return out
})

// ---- thumbnails ----
function thumb(s: Stroke) {
  const r = ed.renders.get(s.id)
  const b = r ? renderBounds(r, s) : null
  if (!r || !b) return null
  const w = Math.max(b.x1 - b.x0, 1), h = Math.max(b.y1 - b.y0, 1)
  const pad = Math.max(w, h) * 0.08
  return {viewBox: `${b.x0 - pad} ${b.y0 - pad} ${w + pad * 2} ${h + pad * 2}`, layers: r.layers}
}

// ---- selection ----
/** The folder row last clicked (also when empty): the target of 解除. */
const focusFolder = ref<number | null>(null)
function clickPath(e: MouseEvent, id: number) { focusFolder.value = null; ed.selectPath(id, e.shiftKey || e.metaKey) }
function clickFolder(e: MouseEvent, f: Folder) {
  focusFolder.value = f.id
  const ids = order.value.filter(s => ed.inFolder(s, f.id) && ed.shown(s)).map(s => s.id)
  ed.anchors = []
  ed.selected = e.shiftKey || e.metaKey ? [...new Set([...ed.selected, ...ids])] : ids
}
const folderSelected = (f: Folder) => {
  if (focusFolder.value === f.id) return true
  const ids = order.value.filter(s => ed.inFolder(s, f.id)).map(s => s.id)
  return ids.length > 0 && ids.every(id => ed.selected.includes(id))
}

// ---- rename ----
const renaming = ref<string | null>(null)
const draft = ref('')
const input = ref<HTMLInputElement[]>()
function startRename(key: string, name: string) {
  renaming.value = key
  draft.value = name
  nextTick(() => { input.value?.[0]?.focus(); input.value?.[0]?.select() })
}
function finishRename(apply: boolean) {
  const key = renaming.value
  renaming.value = null
  const name = draft.value.trim()
  if (!apply || !key || !name) return
  if (key.startsWith('p')) {
    const s = ed.findStroke(+key.slice(1))
    if (s && s.name !== name) ed.edit(() => { s.name = name })
  } else {
    const f = ed.doc.folders?.find(x => x.id === +key.slice(1))
    if (f && f.name !== name) ed.edit(() => { f.name = name })
  }
}

// ---- visibility / folders ----
function toggleVisible(s: Stroke) {
  ed.edit(() => { s.visible = !s.visible })
  if (!ed.shown(s)) { ed.selected = ed.selected.filter(v => v !== s.id); ed.anchors = ed.anchors.filter(a => a.stroke !== s.id) }
}
function toggleFolderVisible(f: Folder) {
  ed.edit(() => { f.visible = !f.visible })
  const hidden = new Set(ed.doc.strokes.filter(s => !ed.shown(s)).map(s => s.id))
  ed.selected = ed.selected.filter(v => !hidden.has(v))
  ed.anchors = ed.anchors.filter(a => !hidden.has(a.stroke))
}
function toggleOpen(f: Folder) { f.open = !f.open; ed.changed() } // view state: no undo step
function group() {
  const id = ed.groupSelection()
  if (id !== undefined && !ed.selected.length) { focusFolder.value = id; startRename(`f${id}`, ed.folderOf(id)?.name ?? '') }
}
const activeFolder = computed(() => {
  void ed.revision
  if (focusFolder.value != null && ed.folderOf(focusFolder.value)) return ed.folderOf(focusFolder.value)
  const s = ed.active
  return s?.folder != null ? ed.folderOf(s.folder) : undefined
})
function ungroupActive() {
  const f = activeFolder.value
  if (!f) return
  ed.ungroup(f.id)
  focusFolder.value = null
}

// ---- drag to reorder / move in and out of folders ----
const list = ref<HTMLElement>()
type Drop = {index: number; into: number | null; line: number; folderRow: number | null}
const dragging = ref<{row: Row; startY: number; active: boolean} | null>(null)
const drop = ref<Drop | null>(null)

function onRowDown(e: PointerEvent, row: Row) {
  if (e.button !== 0 || (e.target as HTMLElement).closest('button, input')) return
  dragging.value = {row, startY: e.clientY, active: false}
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
}
function onRowMove(e: PointerEvent) {
  const d = dragging.value
  if (!d) return
  if (!d.active && Math.abs(e.clientY - d.startY) < 4) return
  d.active = true
  drop.value = dropAt(e.clientY, d.row)
}
function onRowUp() {
  const d = dragging.value
  dragging.value = null
  const t = drop.value
  drop.value = null
  if (!d?.active || !t) return
  if (d.row.type === 'path') ed.moveInList({stroke: d.row.stroke.id}, t.index, t.into)
  else ed.moveInList({folder: d.row.folder.id}, t.index, t.into)
}
/** Where a drop at this height goes, in path positions from the top. */
function dropAt(y: number, moving: Row): Drop | null {
  const els = [...(list.value?.querySelectorAll<HTMLElement>('[data-row]') ?? [])]
  if (!els.length) return null
  const movingTree = moving.type === 'folder' ? ed.folderTree(moving.folder.id) : new Set<number>()
  const movingIds = new Set(moving.type === 'path' ? [moving.stroke.id] : order.value.filter(s => ed.inFolder(s, moving.folder.id)).map(s => s.id))
  const remaining = order.value.filter(s => !movingIds.has(s.id))
  const pos = (id: number) => remaining.findIndex(s => s.id === id)
  /** First / after-last position of a folder's block among the remaining paths. */
  const range = (f: Folder): [number, number] => {
    const m = remaining.filter(s => ed.inFolder(s, f.id))
    if (m.length) return [pos(m[0].id), pos(m[m.length - 1].id) + 1]
    const parent = f.parent != null ? ed.folderOf(f.parent) : undefined
    const start = parent ? range(parent)[0] : 0
    return [start, start]
  }
  const ok = (into: number | null) => into == null || !movingTree.has(into)
  const base = list.value!.getBoundingClientRect().top - list.value!.scrollTop
  for (let i = 0; i < els.length; i++) {
    const r = els[i].getBoundingClientRect()
    if (y > r.bottom && i < els.length - 1) continue
    const row = rows.value[i]
    const upper = y < r.top + r.height / 2
    if (row.type === 'folder') {
      if (movingTree.has(row.folder.id)) return null
      const [first, last] = range(row.folder)
      const parent = row.folder.parent ?? null
      const middle = y > r.top + r.height * 0.25 && y < r.bottom - r.height * 0.25
      if (middle) return {index: first, into: row.folder.id, line: r.top - base, folderRow: i}
      if (upper) return ok(parent) ? {index: first, into: parent, line: r.top - base, folderRow: null} : null
      return row.folder.open
        ? {index: first, into: row.folder.id, line: r.bottom - base, folderRow: null}
        : ok(parent) ? {index: last, into: parent, line: r.bottom - base, folderRow: null} : null
    }
    if (movingIds.has(row.stroke.id)) return null
    const p = pos(row.stroke.id)
    const into = row.stroke.folder ?? null
    if (!ok(into)) return null
    return upper ? {index: p, into, line: r.top - base, folderRow: null} : {index: p + 1, into, line: r.bottom - base, folderRow: null}
  }
  return null
}
</script>

<template>
  <div class="path-list" data-testid="path-list">
    <div class="head">
      <span v-if="!layout.layers.floating">レイヤー</span>
      <span class="spacer" />
      <button type="button" :title="ed.selected.length ? '選択したパスをフォルダにまとめる（⌘G）' : '新しいフォルダ（⌘G）'" aria-label="フォルダにまとめる" @click="group"><Icon icon="material-symbols:create-new-folder-outline" /></button>
      <button type="button" title="フォルダを解除（⇧⌘G）：中身はひとつ上へ" aria-label="フォルダを解除" :disabled="!activeFolder" @click="ungroupActive"><Icon icon="material-symbols:folder-off-outline" /></button>
      <PanelPin id="layers" />
    </div>
    <ul ref="list" @pointermove="onRowMove" @pointerup="onRowUp" @pointercancel="onRowUp">
      <template v-for="row in rows" :key="row.key">
        <li
          v-if="row.type === 'folder'" data-row :data-folder="row.folder.id" class="folder" :style="{paddingLeft: 10 + row.depth * 16 + 'px'}"
          :class="{selected: folderSelected(row.folder), hidden: !row.folder.visible, target: drop?.folderRow !== null && rows[drop?.folderRow ?? -1] === row}"
          @pointerdown="onRowDown($event, row)" @click="clickFolder($event, row.folder)" @dblclick="startRename(row.key, row.folder.name)"
        >
          <button type="button" class="chevron" :aria-label="row.folder.open ? '閉じる' : '開く'" @click.stop="toggleOpen(row.folder)">
            <Icon :icon="row.folder.open ? 'material-symbols:keyboard-arrow-down' : 'material-symbols:keyboard-arrow-right'" />
          </button>
          <Icon class="folder-icon" :icon="row.folder.open ? 'material-symbols:folder-open-outline' : 'material-symbols:folder-outline'" />
          <input
            v-if="renaming === row.key" ref="input" v-model="draft" class="rename"
            @click.stop @keydown.enter.stop="finishRename(true)" @keydown.escape.stop="finishRename(false)" @blur="finishRename(true)"
          >
          <span v-else class="name" :title="row.folder.name">{{ row.folder.name }}</span>
          <span class="count">{{ row.count }}</span>
          <button type="button" class="eye" :aria-label="row.folder.visible ? 'フォルダを隠す' : 'フォルダを表示'" @click.stop="toggleFolderVisible(row.folder)">
            <Icon :icon="row.folder.visible ? 'material-symbols:visibility-outline' : 'material-symbols:visibility-off-outline'" />
          </button>
        </li>
        <li
          v-else data-row :data-path="row.stroke.id"
          :class="{selected: ed.selected.includes(row.stroke.id), active: ed.active?.id === row.stroke.id, hidden: !ed.shown(row.stroke)}" :style="{paddingLeft: 10 + row.depth * 16 + (row.depth ? 8 : 0) + 'px'}"
          @pointerdown="onRowDown($event, row)" @click="clickPath($event, row.stroke.id)" @dblclick="startRename(row.key, row.stroke.name)"
        >
          <span class="thumb">
            <svg v-if="thumb(row.stroke)" :viewBox="thumb(row.stroke)!.viewBox" preserveAspectRatio="xMidYMid meet">
              <template v-for="(l, i) in thumb(row.stroke)!.layers" :key="i">
                <image v-if="l.image" :href="l.image.href" :x="l.image.x" :y="l.image.y" :width="l.image.w" :height="l.image.h" :opacity="l.opacity" preserveAspectRatio="none" />
                <path v-else :d="l.d" :fill="l.color" :fill-opacity="l.opacity" />
              </template>
            </svg>
          </span>
          <input
            v-if="renaming === row.key" ref="input" v-model="draft" class="rename"
            @click.stop @keydown.enter.stop="finishRename(true)" @keydown.escape.stop="finishRename(false)" @blur="finishRename(true)"
          >
          <span v-else class="name" :title="row.stroke.name">{{ row.stroke.name }}</span>
          <Icon v-if="row.stroke.appearance" class="badge" icon="material-symbols:account-tree-outline" title="外観ノードあり" />
          <button type="button" class="eye" :aria-label="row.stroke.visible ? '隠す' : '表示する'" @click.stop="toggleVisible(row.stroke)">
            <Icon :icon="row.stroke.visible ? 'material-symbols:visibility-outline' : 'material-symbols:visibility-off-outline'" />
          </button>
        </li>
      </template>
      <li v-if="!rows.length" class="none">まだパスがありません</li>
      <div v-if="drop && drop.folderRow === null" class="drop-line" :style="{top: drop.line - 1 + 'px'}" />
    </ul>
  </div>
</template>

<style scoped>
.path-list {display: flex; flex-direction: column; height: 100%; font-size: 12px}
.head {display: flex; align-items: center; gap: 2px; height: 28px; padding: 0 6px 0 10px; font-weight: 600; font-size: 11px; color: var(--tq-color-text-mute); flex: none}
.spacer {flex: 1}
.head button, .eye, .chevron {
  display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 2px;
  background: transparent; color: var(--tq-color-text-mute); cursor: pointer; font-size: 14px; flex: none;
}
.head button:hover, .eye:hover, .chevron:hover {color: var(--tq-color-text)}
.head button:disabled {opacity: .35; pointer-events: none}
ul {position: relative; list-style: none; margin: 0; padding: 0; overflow-y: auto; flex: 1}
/* Flat rows: one straight highlight, no rounded corners or side bars. */
li {display: flex; align-items: center; gap: 6px; height: 28px; padding: 0 6px 0 10px; cursor: default; border-bottom: 1px solid transparent}
li:hover {background: var(--tq-color-input)}
li.selected {background: var(--tq-color-accent-soft)}
li.active .name {font-weight: 600}
li.hidden .name, li.hidden .thumb {opacity: .4}
li.folder .chevron {margin-left: -6px; width: 16px}
li.folder.target {background: var(--tq-color-accent-soft); box-shadow: inset 0 0 0 1px var(--tq-color-accent)}
.folder-icon {color: var(--tq-color-text-mute); font-size: 15px; flex: none}
.count {font-size: 10px; color: var(--tq-color-text-mute)}
.thumb {
  width: 30px; height: 20px; flex: none; display: grid; place-items: center;
  background: var(--pw-paper); box-shadow: inset 0 0 0 1px var(--tq-color-border);
}
.thumb svg {width: 26px; height: 16px; display: block}
.name {flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis}
.rename {flex: 1; min-width: 0; height: 20px; border: 0; padding: 0 4px; font: inherit; color: var(--tq-color-text); background: var(--tq-color-input); outline: 1px solid var(--tq-color-accent)}
.badge {color: var(--tq-color-text-mute); font-size: 13px; flex: none}
.none {color: var(--tq-color-text-mute); padding-left: 10px}
.drop-line {position: absolute; left: 6px; right: 6px; height: 2px; background: var(--tq-color-accent); pointer-events: none}
</style>
