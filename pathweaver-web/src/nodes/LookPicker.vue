<script setup lang="ts">
// The path's appearance as a data-block, like Blender's material selector:
// browse (pick any appearance in the file — it is then shared and synced),
// its name, the user count (click = make it this path's own copy), new/copy,
// and unlink. Everything picked applies to ALL selected paths as one shared
// appearance. The list also holds the look library (.pwn files, every
// document) and the sets that come with the app.
import {computed, nextTick, onBeforeUnmount, ref} from 'vue'
import type {Editor} from '../core/editor'
import type {Stroke} from '../core/types'
import {applySet, BUILTIN_SETS, linkLook, lookOf, looks, makeSingleUser, newLook, unlinkLook, useBuiltin, useSet} from '../core/looks'
import {deleteFromLibrary, exportLook, importLooks, library, revealLibrary, saveToLibrary} from '../core/library'
import {Icon} from '../../vendor/tweeq/src/Icon'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'

const props = defineProps<{ed: Editor; stroke: Stroke}>()
const ed = props.ed
const all = computed(() => { void ed.revision; return looks(ed.doc) })
const mine = computed(() => { void ed.revision; return lookOf(ed.doc, props.stroke) })
const sets = computed(() => { void ed.revision; return ed.doc.appearance_presets.map(p => p.name) })
/** The selected paths (this one included): what a pick applies to. */
const targets = computed(() => {
  void ed.revision
  const sel = ed.selected.map(id => ed.findStroke(id)).filter((s): s is Stroke => !!s && !s.guide)
  return sel.includes(props.stroke) ? sel : [props.stroke]
})
const many = computed(() => targets.value.length > 1)
const open = ref(false)
const query = ref('')
const search = ref<HTMLInputElement>()
const root = ref<HTMLElement>()
const match = (name: string) => name.toLowerCase().includes(query.value.trim().toLowerCase())

function toggle() {
  open.value = !open.value
  query.value = ''
  if (open.value) nextTick(() => search.value?.focus())
}
function close() { open.value = false }
const onDoc = (e: PointerEvent) => { if (open.value && !root.value?.contains(e.target as Node)) close() }
document.addEventListener('pointerdown', onDoc, true)
onBeforeUnmount(() => document.removeEventListener('pointerdown', onDoc, true))

function pick(key: string) {
  const l = all.value.find(v => v.key === key)
  if (l) linkLook(ed, targets.value, l)
  close()
}
function pickSet(name: string) { useSet(ed, targets.value, name); close() }
function pickBuiltin(name: string) { useBuiltin(ed, targets.value, name); close() }
function pickLibrary(name: string) {
  const l = library.items.find(v => v.name === name)
  if (l) applySet(ed, targets.value, l.name, () => JSON.parse(JSON.stringify(l.graph)))
  close()
}
function saveLibrary() {
  const g = props.stroke.appearance
  if (!g) return
  const name = mine.value?.name ?? '外観'
  saveToLibrary(name, g)
  ed.status = `「${name}」をライブラリに保存しました（どのファイルからも使えます）`
  close()
}
function exportMine() { const g = props.stroke.appearance; if (g) exportLook(mine.value?.name ?? '外観', g); close() }
function deleteSet(name: string) { ed.edit(d => { d.appearance_presets = d.appearance_presets.filter(p => p.name !== name) }) }
function rename(v: string) {
  const g = props.stroke.appearance
  if (!g) return
  ed.begin()
  g.name = v
  ed.changed()
}
function onKey(e: KeyboardEvent) {
  e.stopPropagation()
  if (e.key === 'Escape') close()
  if (e.key === 'Enter' && !e.isComposing) {
    const l = all.value.find(v => match(v.name) && v.key !== mine.value?.key)
    if (l) pick(l.key)
  }
}
const pathNames = (users: Stroke[]) => users.map(s => s.name).join('、')
</script>

<template>
  <div ref="root" class="look" data-testid="look-picker">
    <button
      type="button" class="browse" :class="{on: open}" aria-haspopup="listbox" :aria-expanded="open" aria-label="外観を選ぶ"
      v-tooltip="'ほかのパスの外観を選ぶ（共有して同期）'" data-testid="look-browse" @click="toggle"
    >
      <Icon icon="material-symbols:texture" /><Icon class="caret" icon="material-symbols:arrow-drop-down" />
    </button>
    <template v-if="mine">
      <input
        class="name" :value="mine?.name" aria-label="外観の名前" data-testid="look-name"
        @focus="ed.begin()" @input="rename(($event.target as HTMLInputElement).value)" @change="ed.commit()" @blur="ed.commit()" @keydown.stop="e => { if ((e.key === 'Enter' && !e.isComposing) || e.key === 'Escape') (e.target as HTMLInputElement).blur() }"
      >
      <button
        v-if="(mine?.users.length ?? 0) > 1" type="button" class="users" data-testid="look-users"
        v-tooltip="`${mine!.users.length}つのパスで共有中（${pathNames(mine!.users)}）。押すとこのパス専用の複製にする`"
        :aria-label="`${mine!.users.length}つのパスで共有中。このパス専用にする`" @click="makeSingleUser(ed, stroke)"
      >{{ mine!.users.length }}</button>
      <button type="button" class="icon" v-tooltip="'複製して、このパス用の新しい外観にする'" aria-label="外観を複製" data-testid="look-copy" @click="newLook(ed, targets)">
        <Icon icon="material-symbols:content-copy-outline" />
      </button>
      <button type="button" class="icon" v-tooltip="'このパスから外観を外す'" aria-label="外観を外す" data-testid="look-unlink" @click="unlinkLook(ed, targets)">
        <Icon icon="material-symbols:close" />
      </button>
    </template>
    <button v-else type="button" class="new" data-testid="look-new" aria-label="新しい外観" @click="newLook(ed, targets)">
      <Icon icon="material-symbols:add" />新規
    </button>

    <div v-if="open" class="pop" role="listbox" aria-label="外観">
      <input ref="search" v-model="query" class="search" placeholder="検索…" aria-label="外観を検索" @keydown="onKey">
      <div class="list">
        <div class="group">このファイルの外観</div>
        <button
          v-for="l in all.filter(v => match(v.name))" :key="l.key" type="button" class="row" role="option"
          :aria-selected="l.key === mine?.key" :class="{current: l.key === mine?.key}" :data-look="l.name"
          :title="`使っているパス：${pathNames(l.users)}`" @click="pick(l.key)"
        >
          <span class="label">{{ l.name }}</span><span class="count">{{ l.users.length }}</span>
        </button>
        <div v-if="!all.length" class="none">まだ外観がありません</div>
        <div class="group lib">
          <span>ライブラリ（すべてのファイル）</span>
          <button type="button" class="mini" title="ファイル（.pwn）から読み込む" aria-label="ライブラリに読み込む" data-testid="lib-import" @click="importLooks(); close()"><Icon icon="material-symbols:download" /></button>
          <button v-if="library.native" type="button" class="mini" title="ライブラリのフォルダを開く" aria-label="ライブラリのフォルダを開く" @click="revealLibrary(); close()"><Icon icon="material-symbols:folder-open-outline" /></button>
        </div>
        <div v-for="l in library.items.filter(v => match(v.name))" :key="'lib' + l.name" class="row set">
          <button type="button" class="label" :data-library="l.name" @click="pickLibrary(l.name)">{{ l.name }}</button>
          <button type="button" class="del" :aria-label="`ライブラリの「${l.name}」を削除`" title="ライブラリから削除" @click="deleteFromLibrary(l.name)"><Icon icon="material-symbols:close" /></button>
        </div>
        <div v-if="!library.items.length" class="none">「ライブラリに保存」で、どのファイルからも使えます</div>
        <template v-if="sets.some(match)">
          <div class="group">このファイルのセット</div>
          <div v-for="n in sets.filter(match)" :key="n" class="row set">
            <button type="button" class="label" :data-set="n" @click="pickSet(n)">{{ n }}</button>
            <button type="button" class="del" :aria-label="`セット「${n}」を削除`" title="削除" @click="deleteSet(n)"><Icon icon="material-symbols:close" /></button>
          </div>
        </template>
        <template v-if="BUILTIN_SETS.some(b => match(b.name))">
          <div class="group">用意されたセット</div>
          <button v-for="b in BUILTIN_SETS.filter(v => match(v.name))" :key="b.name" type="button" class="row" :data-builtin="b.name" :title="b.hint" @click="pickBuiltin(b.name)">
            <span class="label">{{ b.name }}</span>
          </button>
        </template>
      </div>
      <div v-if="many" class="note">選んでいる{{ targets.length }}本のパスに、ひとつの外観としてかけます（共有）</div>
      <div v-if="mine" class="actions">
        <button type="button" class="save" data-testid="lib-save" @click="saveLibrary">
          <Icon icon="material-symbols:bookmark-add-outline" />ライブラリに保存
        </button>
        <button type="button" class="save" @click="exportMine">
          <Icon icon="material-symbols:upload" />書き出す…
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.look {position: relative; display: flex; align-items: center; gap: 2px}
button {font: inherit; color: var(--tq-color-text); cursor: pointer}
.browse, .icon, .users, .new {
  display: inline-flex; align-items: center; justify-content: center; height: 20px; border: 0; border-radius: 10px;
  background: var(--tq-color-input);
}
.browse {padding: 0 0 0 6px}
.browse .caret {margin-left: -2px}
.browse.on, .browse:hover, .icon:hover, .users:hover, .new:hover {background: var(--tq-color-input-hover)}
.icon {width: 20px; padding: 0; background: transparent; color: var(--tq-color-text-mute)}
.users {min-width: 20px; padding: 0 6px; font-variant-numeric: tabular-nums; font-weight: 600}
.new {padding: 0 8px 0 4px; gap: 2px}
.name {
  width: 120px; height: 20px; padding: 0 8px; border: 0; border-radius: 3px; outline: none;
  background: var(--tq-color-input); color: var(--tq-color-text); font: inherit;
}
.name:focus {box-shadow: 0 0 0 1px var(--tq-color-accent)}
.pop {
  position: absolute; top: 24px; left: 0; z-index: 40; width: 270px; display: flex; flex-direction: column;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 4px;
  box-shadow: 0 4px 16px var(--tq-color-shadow, rgba(0,0,0,.2)); font-size: 12px;
}
.search {
  margin: 6px; padding: 4px 8px; height: 24px; border: 0; border-radius: 3px; outline: none;
  background: var(--tq-color-input); color: var(--tq-color-text); font: inherit;
}
.list {max-height: 260px; overflow-y: auto; padding: 0 4px 4px}
.group {padding: 6px 6px 2px; font-size: 10px; font-weight: 600; color: var(--tq-color-text-mute)}
.row {display: flex; align-items: center; gap: 6px; width: 100%; padding: 4px 6px; border: 0; border-radius: 3px; background: transparent; text-align: left}
.row:hover {background: var(--tq-color-input-hover)}
.row.current {background: var(--tq-color-accent); color: var(--tq-color-on-accent)}
.row .label {flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap}
.row.set .label {border: 0; background: transparent; padding: 0; text-align: left}
.count {font-size: 10px; opacity: .7; font-variant-numeric: tabular-nums}
.del {display: grid; place-items: center; width: 18px; height: 18px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--tq-color-text-mute)}
.del:hover {background: var(--tq-color-input)}
.none {padding: 6px; color: var(--tq-color-text-mute)}
.actions {display: flex; border-top: 1px solid var(--tq-color-border); margin: 0 4px 4px}
.actions .save {border-top: 0; margin: 0; flex: 1; white-space: nowrap}
.note {padding: 4px 10px; font-size: 10px; color: var(--tq-color-accent)}
.group.lib {display: flex; align-items: center; gap: 2px}
.group.lib span {flex: 1}
.mini {display: grid; place-items: center; width: 18px; height: 18px; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--tq-color-text-mute); font-size: 12px}
.mini:hover {background: var(--tq-color-input); color: var(--tq-color-text)}
.save {display: flex; align-items: center; gap: 4px; margin: 0 4px 4px; padding: 4px 6px; border: 0; border-top: 1px solid var(--tq-color-border); background: transparent; color: var(--tq-color-text-mute); text-align: left}
.save:hover {color: var(--tq-color-text)}
</style>
