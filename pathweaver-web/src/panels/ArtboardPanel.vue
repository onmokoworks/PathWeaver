<script setup lang="ts">
// Artboards (shown while the artboard tool is active): the list, and the
// edited one's name, position and size. Every change is one undo step.
import {computed} from 'vue'
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import ParameterHeading from '../../vendor/tweeq/src/ParameterGrid/ParameterHeading.vue'
import InputString from '../../vendor/tweeq/src/InputString/InputString.vue'
import InputDropdown from '../../vendor/tweeq/src/InputDropdown/InputDropdown.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import LengthPair from './LengthPair.vue'
import {display, formatSize, fromUnit} from '../core/units'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'
import type {Artboard} from '../core/types'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const ed = props.ed
// copies, so the numbers refresh on every document change (the document itself is not deeply reactive)
const list = computed(() => { void ed.revision; return (ed.doc.artboards ?? []).map(a => ({...a})) })
const board = computed(() => list.value.find(a => a.id === props.canvas.state.artboard) ?? null)
const live = () => (ed.doc.artboards ?? []).find(a => a.id === props.canvas.state.artboard)

/** Common page sizes, defined in their own unit (paper in mm / in, screens in px = pt). */
const mm = (v: number) => fromUnit(v, 'mm')
const PRESETS: {label: string; w: number; h: number; screen?: boolean}[] = [
  {label: 'A4 縦', w: mm(210), h: mm(297)}, {label: 'A4 横', w: mm(297), h: mm(210)},
  {label: 'A3 縦', w: mm(297), h: mm(420)}, {label: 'はがき', w: mm(100), h: mm(148)},
  {label: 'レター', w: fromUnit(8.5, 'in'), h: fromUnit(11, 'in')},
  {label: '1920×1080', w: 1920, h: 1080, screen: true}, {label: '1080×1080', w: 1080, h: 1080, screen: true},
  {label: '1080×1920', w: 1080, h: 1920, screen: true}, {label: '1280×720', w: 1280, h: 720, screen: true},
]
// Paper sizes read in the document's unit; screen sizes are pixel counts.
const presetLabels = computed(() => PRESETS.map(p => (p.screen ? `${p.label} px` : `${p.label}（${formatSize(p.w, p.h, display.unit)}）`)))

let editing = false
function begin() { if (!editing) { ed.begin(); editing = true } }
function commit() { if (editing) { editing = false; ed.commit() } }
function patch(p: Partial<Artboard>) {
  const a = live()
  if (!a) return
  begin()
  Object.assign(a, p)
  ed.changed()
}
function choosePreset(label: string) {
  const p = PRESETS[presetLabels.value.indexOf(label)]
  if (!p) return
  patch({w: p.w, h: p.h})
  commit()
}
/** A new artboard: around the selection, else the whole drawing, else A4 at the view's centre. */
function add() {
  const b = props.canvas.selectionBounds() ?? drawingBounds()
  const v = props.canvas.view
  const c = props.canvas.toWorld({x: v.width / 2, y: v.height / 2})
  const frame = b ? {x: b.x0, y: b.y0, w: b.x1 - b.x0, h: b.y1 - b.y0} : {x: c.x - 297, y: c.y - 421, w: 595, h: 842}
  const id = ed.edit(d => {
    const id = ed.nextId()
    ;(d.artboards ??= []).push({id, name: `アートボード ${d.artboards.length + 1}`, ...frame})
    return id
  })
  if (id != null) props.canvas.state.artboard = id
}
function fitSelection() {
  const b = props.canvas.selectionBounds()
  if (!b || !board.value) return
  patch({x: b.x0, y: b.y0, w: b.x1 - b.x0, h: b.y1 - b.y0})
  commit()
}
function drawingBounds() {
  let out: {x0: number; y0: number; x1: number; y1: number} | null = null
  const keep = ed.selected
  ed.selected = ed.doc.strokes.filter(s => !s.guide && !s.image?.reference).map(s => s.id)
  out = props.canvas.selectionBounds()
  ed.selected = keep
  return out
}
</script>

<template>
  <ParameterGrid data-testid="artboard-panel">
    <ParameterHeading>アートボード</ParameterHeading>
    <li v-if="!board" class="empty">キャンバスでアートボードを押すか、レイヤーの「アートボード」タブで選ぶと、ここで名前と大きさを変えられます</li>
    <template v-if="board">
      <Parameter label="名前">
        <InputString data-field="artboard-name" :model-value="board.name" @update:model-value="v => { patch({name: v}); commit() }" />
      </Parameter>
      <Parameter label="位置" hint="左上の位置">
        <LengthPair kind="position" data-field="artboard-position" :model-value="[board.x, board.y]" @focus="begin" @update:model-value="v => patch({x: v[0], y: v[1]})" @confirm="commit" @blur="commit" />
      </Parameter>
      <Parameter label="サイズ">
        <LengthPair kind="size" data-field="artboard-size" :model-value="[board.w, board.h]" @focus="begin" @update:model-value="v => patch({w: Math.max(1, v[0]), h: Math.max(1, v[1])})" @confirm="commit" @blur="commit" />
      </Parameter>
      <Parameter label="定型">
        <InputDropdown data-field="artboard-preset" :model-value="''" :options="presetLabels" @update:model-value="choosePreset" />
      </Parameter>
    </template>
  </ParameterGrid>
  <div class="buttons">
    <InputButton subtle icon="material-symbols:add" label="アートボードを追加" tooltip="選択（なければ絵全体）を囲むように作る" data-testid="artboard-add" @click="add" />
    <InputButton v-if="board && ed.selected.length" subtle icon="mdi:crop-free" label="選択に合わせる" @click="fitSelection" />
    <InputButton v-if="board" subtle icon="material-symbols:delete-outline" label="削除" tooltip="Delete" data-testid="artboard-delete" @click="canvas.deleteArtboard(board.id)" />
  </div>
</template>

<style scoped>
.empty {grid-column: 1 / 3; list-style: none; padding: 4px 0; color: var(--tq-color-text-mute); line-height: 1.6}
.row {
  display: flex; align-items: center; gap: 8px; height: 24px; padding: 0 8px; border: 0; border-radius: 3px;
  background: transparent; font: inherit; color: var(--tq-color-text); cursor: pointer; text-align: left;
}
.row:hover {background: var(--tq-color-input-hover)}
.row.on {background: color-mix(in srgb, var(--tq-color-text) 9%, transparent)}
.name {flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap}
.size {color: var(--tq-color-text-mute); font-variant-numeric: tabular-nums}
.empty {margin: 0; padding: 4px 8px; color: var(--tq-color-text-mute)}
.buttons {display: flex; flex-wrap: wrap; gap: 4px; padding: 8px 12px}
</style>
