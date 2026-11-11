<script setup lang="ts">
// Properties of the current selection, laid out with Tweeq's ParameterGrid.
// Every field edit is one undo step: focus → previews → confirm/blur.
import {computed, ref} from 'vue'
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import ParameterHeading from '../../vendor/tweeq/src/ParameterGrid/ParameterHeading.vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputAngle from '../../vendor/tweeq/src/InputAngle/InputAngle.vue'
import LengthField from './LengthField.vue'
import LengthPair from './LengthPair.vue'
import InputString from '../../vendor/tweeq/src/InputString/InputString.vue'
import InputRadio from '../../vendor/tweeq/src/InputRadio/InputRadio.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import InputDropdown from '../../vendor/tweeq/src/InputDropdown/InputDropdown.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import {canCarryText, detachText, fontFamilies, relayout} from '../core/textOutline'
import PaintField from './PaintField.vue'
import PaintRows from './PaintRows.vue'
import PivotPicker from './PivotPicker.vue'
import ArtboardPanel from './ArtboardPanel.vue'
import {pivotIn} from '../canvas/controller'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'
import {call} from '../core/wasm'
import type {Cap, CornerWidget, Document, Join, Knockout, RGB, Stroke, TextPath, TextSpec} from '../core/types'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const emit = defineEmits<{openNodes: []}>()
const ed = props.ed

const strokes = computed<Stroke[]>(() => {
  void ed.revision
  return ed.selected.map(id => ed.findStroke(id)).filter((s): s is Stroke => !!s)
})
const active = computed(() => ed.active)
const commit = () => ed.commit()
const begin = () => ed.begin()

// ---- paint (applies to every selected path) ----
function same<T>(get: (s: Stroke) => T): T | undefined {
  const vals = strokes.value.map(s => JSON.stringify(get(s)))
  return vals.length && vals.every(v => v === vals[0]) ? JSON.parse(vals[0]) : undefined
}
const lineWidth = computed(() => {
  void ed.revision
  const s = active.value
  if (!s) return 0
  if (s.contours) return s.edge_width
  const ws = [s.start_width, s.closed ? s.start_width : s.end_width, ...s.widths.filter(k => !k.interpolated).map(k => k.width)]
  return Math.max(...ws.map(w => w.left + w.right))
})
function setLineWidth(v: number) {
  ed.begin()
  for (const s of strokes.value) ed.coreEdit('lineWidth', {stroke: s.id, value: v})
}
const join = computed(() => same(s => s.join) ?? '')  // '' = mixed
function setJoin(v: Join) { ed.edit(() => { for (const s of strokes.value) s.join = v }) }

// ---- text (live: characters and font stay editable) ----
const texts = computed(() => strokes.value.filter(s => s.text))
const text = computed(() => texts.value.at(-1)?.text ?? null)
const fonts = ref<string[]>([])
fontFamilies().then(v => { fonts.value = v })
const fontOptions = computed(() => {
  const f = text.value?.font
  return f && !fonts.value.includes(f) ? [f, ...fonts.value] : fonts.value
})
async function setText(patch: Partial<Omit<TextSpec, 'text' | 'matrix' | 'on_path'>>, once = false) {
  ed.begin()
  for (const s of texts.value) Object.assign(s.text!, patch)
  ed.changed()
  const done = Promise.all(texts.value.map(s => relayout(ed, s)))
  if (once) { await done; ed.commit() }
}
const ALIGNS = ['left', 'center', 'right', 'justify'] as const
// ---- text on a path ----
const onPath = computed(() => { void ed.revision; return text.value?.on_path ?? null })
/** Position and side on the path (every selected text on a path). */
function setOnPath(patch: Partial<Omit<TextPath, 'stroke'>>) {
  ed.begin()
  for (const s of texts.value) if (s.text!.on_path) Object.assign(s.text!.on_path, patch)
  ed.changed()
}
/** One text and one ordinary path selected: the text can go onto the path. */
const carrier = computed(() => {
  if (texts.value.length !== 1 || strokes.value.length !== 2) return null
  const p = strokes.value.find(s => !s.text)
  return canCarryText(p) && texts.value[0].text!.on_path?.stroke !== p.id ? p : null
})
async function attachToPath() {
  const p = carrier.value, s = texts.value[0]
  if (!p || !s) return
  ed.begin()
  s.text!.on_path = {stroke: p.id, offset: 0}
  ed.changed()
  await relayout(ed, s)
  ed.commit()
  ed.selected = [s.id]
  ed.status = 'パスに沿わせました。パスを変えると文字も追従します'
}
async function detachFromPath() {
  const list = texts.value.filter(s => s.text!.on_path)
  ed.begin()
  for (const s of list) detachText(ed.doc, s)
  ed.changed()
  await Promise.all(list.map(s => relayout(ed, s)))
  ed.commit()
}
function editTextOnCanvas() {
  const s = texts.value.at(-1)
  if (!s) return
  props.canvas.setTool('select')
  props.canvas.editText(s.id)
}
function outlineTexts() {
  const list = texts.value
  ed.edit(() => { for (const s of list) { s.text = null; s.name = `${s.name}（アウトライン）` } })
}

// ---- blend ----
const blend = computed(() => {
  void ed.revision
  const id = strokes.value.map(s => ed.blendOf(s.id)).find(Boolean)
  return id ?? null
})
const canBlend = computed(() => strokes.value.length === 2 && !blend.value)

// ---- placed images ----
const images = computed(() => strokes.value.filter(s => s.image))
const onlyImages = computed(() => strokes.value.length > 0 && images.value.length === strokes.value.length)
const image = computed(() => images.value.at(-1)?.image ?? null)
function setImage(patch: {opacity?: number; reference?: boolean; knockout?: Knockout | null}) {
  ed.begin()
  for (const s of images.value) {
    Object.assign(s.image!, patch)
    if (patch.reference === false) delete s.image!.reference
    if (patch.knockout === null) delete s.image!.knockout
  }
  ed.changed()
}
const knockout = computed(() => image.value?.knockout ?? 'none')
function setKnockout(v: Knockout | 'none') {
  setImage({knockout: v === 'none' ? null : v})
  ed.commit()
}
function setReference(v: boolean) {
  setImage({reference: v})
  ed.commit()
  // A 下絵 is not picked on the canvas: let go of it there.
  if (v) { ed.selected = []; ed.status = '下絵にしました（書き出されず、キャンバスでは選択されません。レイヤーから選び直せます）' }
}

/** The path button always deletes paths (Delete key deletes points when points are selected). */
function deletePaths() { ed.anchors = []; ed.deleteSelection() }

// ---- style of the next path (shown when nothing is selected) ----
const style = ed.drawStyle
function setStyle(patch: Partial<typeof style>) { Object.assign(style, patch) }

// ---- pathfinder (2+ paths) ----
const PATHFINDER = [
  {op: 'Unite', icon: 'mdi:vector-union', label: '合体'},
  {op: 'MinusFront', icon: 'mdi:vector-difference-ab', label: '前面で型抜き'},
  {op: 'MinusBack', icon: 'mdi:vector-difference-ba', label: '背面で型抜き'},
  {op: 'Intersect', icon: 'mdi:vector-intersection', label: '交差'},
  {op: 'Exclude', icon: 'mdi:vector-difference', label: '中マド'},
  {op: 'Divide', icon: 'mdi:vector-combine', label: '分割'},
] as const
function runPathfinder(op: string) {
  const ids = ed.coreEdit<number[]>('pathfinder', {ids: ed.selected, pathfinder: op})
  if (ids) { ed.selected = ids; ed.anchors = [] }
}

// ---- transform via bounds (X, Y, W, H, rotation) around the reference point ----
let base: {doc: string; b: {x0: number; y0: number; x1: number; y1: number}; at: {x: number; y: number}} | null = null
const bounds = computed(() => {
  void ed.revision
  return props.canvas.selectionBounds()
})
const pivot = computed(() => props.canvas.state.pivot)
/** The reference point on the page: what X/Y show and what size and rotation keep still. */
const at = computed(() => bounds.value ? pivotIn(bounds.value, pivot.value) : null)
function beginTransform() {
  ed.begin()
  const b = bounds.value
  if (b) base = {doc: JSON.stringify(ed.doc.strokes), b, at: pivotIn(b, pivot.value)}
}
/** Position (X, Y) and size (W, H) as pairs: Tweeq's 2D drag pad / linked size. */
function setPosition(v: [number, number]) {
  if (!base) beginTransform()
  if (!base) return
  transformBase([1, 0, 0, 1, v[0] - base.at.x, v[1] - base.at.y])
}
function setSize(v: [number, number]) {
  if (!base) beginTransform()
  if (!base) return
  const b = base.b, c = base.at
  const sx = Math.max(v[0], 0.01) / Math.max(b.x1 - b.x0, 1e-9), sy = Math.max(v[1], 0.01) / Math.max(b.y1 - b.y0, 1e-9)
  transformBase([sx, 0, 0, sy, c.x - sx * c.x, c.y - sy * c.y])
}
// Rotation is applied, not stored: the dial turns the selection while it is
// held and reads 0 again afterwards (a path has no lasting angle).
const turn = ref(0)
function setTurn(deg: number) {
  if (!base) beginTransform()
  if (!base) return
  turn.value = deg
  const a = (deg * Math.PI) / 180, cos = Math.cos(a), sin = Math.sin(a), c = base.at
  transformBase([cos, sin, -sin, cos, c.x - cos * c.x + sin * c.y, c.y - sin * c.x - cos * c.y])
}
function endTurn() { turn.value = 0; endTransform() }
function transformBase(m: number[]) {
  const d = {...ed.doc, strokes: JSON.parse(base!.doc)} as Document
  const r = call<{doc: Document}>('edit', {doc: d, kind: 'transform', ids: ed.selected, matrix: m})
  ed.doc.strokes = r.doc.strokes
  ed.changed()
}
function endTransform() { base = null; ed.commit() }

// ---- align / distribute (Illustrator's Align panel) ----
const units = computed(() => {
  void ed.revision
  return ed.tool === 'artboard' ? [] : props.canvas.arrangeUnits()
})
const hasBoards = computed(() => (ed.doc.artboards?.length ?? 0) > 0)
/** Two or more things line up with each other; one thing with its artboard. */
const showAlign = computed(() => units.value.length >= 2 || (units.value.length === 1 && hasBoards.value))
const ALIGN = [
  {kind: 'left', icon: 'material-symbols:align-horizontal-left', label: '左揃え'},
  {kind: 'hcenter', icon: 'material-symbols:align-horizontal-center', label: '左右中央揃え'},
  {kind: 'right', icon: 'material-symbols:align-horizontal-right', label: '右揃え'},
  {kind: 'top', icon: 'material-symbols:align-vertical-top', label: '上揃え'},
  {kind: 'vcenter', icon: 'material-symbols:align-vertical-center', label: '上下中央揃え'},
  {kind: 'bottom', icon: 'material-symbols:align-vertical-bottom', label: '下揃え'},
] as const
const DISTRIBUTE = [
  {kind: 'hcenter', icon: 'mdi:distribute-horizontal-center', label: '左右中央で分布：中心を等間隔に'},
  {kind: 'vcenter', icon: 'mdi:distribute-vertical-center', label: '上下中央で分布：中心を等間隔に'},
  {kind: 'hgap', icon: 'material-symbols:horizontal-distribute', label: '横の間隔を揃える：すき間を同じに'},
  {kind: 'vgap', icon: 'material-symbols:vertical-distribute', label: '縦の間隔を揃える：すき間を同じに'},
] as const
const alignTo = computed(() => props.canvas.state.alignTo)
function setAlignTo(v: string) {
  if (v === 'artboard' && !hasBoards.value) { ed.status = 'アートボードがありません（⇧Oで作る）'; return }
  props.canvas.state.alignTo = v as typeof props.canvas.state.alignTo
}

// ---- anchors ----
const anchorNodes = computed(() => {
  void ed.revision
  return ed.anchors.map(a => ({a, n: ed.findStroke(a.stroke)?.nodes.find(n => n.id === a.node)})).filter(v => v.n)
})
const lastAnchor = computed(() => anchorNodes.value[anchorNodes.value.length - 1])
let anchorBase: {x: number; y: number}[] | null = null
function setAnchorXY(v: [number, number]) {
  const last = lastAnchor.value
  if (!last) return
  if (!anchorBase) { ed.begin(); anchorBase = anchorNodes.value.map(({n}) => ({...n!.p})) }
  const ref = anchorBase[anchorBase.length - 1]
  const dx = v[0] - ref.x, dy = v[1] - ref.y
  anchorNodes.value.forEach(({n}, i) => { n!.p = {x: anchorBase![i].x + dx, y: anchorBase![i].y + dy} })
  ed.changed()
}
function endAnchor() { anchorBase = null; ed.commit() }
const anchorKind = computed(() => {
  const kinds = anchorNodes.value.map(({n}) => (Math.hypot(n!.incoming.x, n!.incoming.y) + Math.hypot(n!.outgoing.x, n!.outgoing.y) < 1e-6 ? 'corner' : n!.smooth ? 'smooth' : 'independent'))
  return kinds.every(k => k === kinds[0]) ? kinds[0] : ''
})
function setKind(k: string) { ed.coreEdit('anchorKind', {anchors: ed.anchors, anchorKind: k}) }
const mergeTarget = ref<'center' | 'first' | 'last'>('center')
function anchorOp(kind: string, extra: Record<string, unknown> = {}) {
  const r = ed.coreEdit<unknown>(kind, {anchors: ed.anchors, ...extra})
  if (r === undefined) return
  if (kind === 'separate' && Array.isArray(r)) ed.anchors = r as typeof ed.anchors
  else if ((kind === 'join' || kind === 'merge') && r) ed.anchors = [r as typeof ed.anchors[number]]
  else if (kind === 'deleteAnchors') ed.anchors = []
}
/** The segment between two adjacent selected anchors (for straighten). */
const segment = computed(() => {
  void ed.revision
  if (ed.anchors.length !== 2 || ed.anchors[0].stroke !== ed.anchors[1].stroke) return null
  const s = ed.findStroke(ed.anchors[0].stroke)
  if (!s) return null
  const i = s.nodes.findIndex(n => n.id === ed.anchors[0].node)
  const j = s.nodes.findIndex(n => n.id === ed.anchors[1].node)
  const n = s.nodes.length
  if ((i + 1) % n === j && (j > i || s.closed)) return {stroke: s.id, node: s.nodes[i].id}
  if ((j + 1) % n === i && (i > j || s.closed)) return {stroke: s.id, node: s.nodes[j].id}
  return null
})
function straighten() {
  const seg = segment.value
  if (seg) ed.coreEdit('straighten', {stroke: seg.stroke, segmentNode: seg.node})
}
const r2 = (v: number) => Math.round(v * 100) / 100

// ---- caps (open paths) ----
const openPaths = computed(() => strokes.value.filter(s => !s.closed && !s.contours))
const capStyle = computed(() => {
  const v = openPaths.value.flatMap(s => [s.start_cap ?? 'Butt', s.end_cap ?? 'Butt'])
  return v.length && v.every(x => x === v[0]) ? v[0] : ''
})
function setCaps(c: Cap) { ed.edit(() => { for (const s of openPaths.value) { s.start_cap = c; s.end_cap = c } }) }

// ---- live corners on the selected anchors ----
const cornerTargets = computed(() => {
  void ed.revision
  const out: {stroke: number; w: CornerWidget}[] = []
  for (const a of ed.anchors) {
    const s = ed.findStroke(a.stroke)
    if (!s || s.contours) continue
    const w = call<CornerWidget[]>('cornerWidgets', {stroke: s, zoom: props.canvas.view.zoom}).find(v => v.node === a.node)
    if (w) out.push({stroke: s.id, w})
  }
  return out
})
const cornerRadius = computed(() => cornerTargets.value[cornerTargets.value.length - 1]?.w.radius ?? 0)
const cornerKind = computed(() => {
  const k = cornerTargets.value.map(t => t.w.kind)
  return k.length && k.every(x => x === k[0]) ? k[0] : ''
})
function setCornerRadius(v: number) {
  ed.begin()
  for (const t of cornerTargets.value) ed.coreEdit('setCorner', {stroke: t.stroke, nodes: [t.w.node], radius: v})
}
function setCornerKind(k: CornerWidget['kind']) {
  ed.begin()
  for (const t of cornerTargets.value) ed.coreEdit('setCorner', {stroke: t.stroke, nodes: [t.w.node], radius: t.w.radius || 0, cornerKind: k})
  ed.commit()
}

// ---- width point selected on the canvas (A/P: click a band, ⌘+click adds) ----
const widthPoint = computed(() => {
  void ed.revision
  const t = props.canvas.state.widthTarget
  const s = t ? ed.findStroke(t.stroke) : undefined
  if (!t || !s) return null
  const h = props.canvas.widthHandles(s).find(v => v.target === t.target)
  return h ? {stroke: s.id, target: t.target, h} : null
})
function setWidth(patch: Record<string, number | boolean>) {
  const w = widthPoint.value
  if (!w) return
  ed.begin()
  ed.coreEdit('setWidth', {stroke: w.stroke, target: w.target, ...patch})
}
function setTotal(total: number) {
  const w = widthPoint.value
  if (!w) return
  const delta = (total - (w.h.left + w.h.right)) / 2
  setWidth({left: Math.max(0, w.h.left + delta), right: Math.max(0, w.h.right + delta)})
}
function deleteWidthPoint() {
  const w = widthPoint.value
  if (!w || typeof w.target !== 'number') return
  ed.coreEdit('deleteWidth', {stroke: w.stroke, key: w.target})
  props.canvas.state.widthTarget = null
}

// ---- end cuts (⌘+drag an endpoint on the canvas) ----
type Caps = {start: number; startLocked: boolean; end: number; endLocked: boolean}
const capInfo = computed<Caps | null>(() => {
  void ed.revision
  const s = active.value
  if (!s || s.closed || s.contours || s.nodes.length < 2) return null
  try { return call<{result: Caps | null}>('edit', {doc: ed.doc, kind: 'capAngles', stroke: s.id}).result } catch { return null }
})
function setCap(end: 'start' | 'end', angle: number | null) {
  const s = active.value
  if (!s) return
  ed.begin()
  ed.coreEdit('capAngle', {stroke: s.id, end, angle})
}
</script>

<template>
  <div class="inspector" data-testid="inspector">
    <ArtboardPanel v-if="ed.tool === 'artboard'" :ed="ed" :canvas="canvas" />
    <!-- anchors -->
    <template v-if="ed.anchors.length && lastAnchor">
      <ParameterGrid>
        <ParameterHeading>アンカー（{{ ed.anchors.length }}点）</ParameterHeading>
        <Parameter label="位置" hint="左の点の格子をドラッグすると縦横に一度に動かせます">
          <LengthPair kind="position" data-field="anchor-position" :model-value="[lastAnchor.n!.p.x, lastAnchor.n!.p.y]" @focus="begin" @update:model-value="v => setAnchorXY(v as [number, number])" @confirm="endAnchor" @blur="endAnchor" />
        </Parameter>
        <Parameter label="種類">
          <InputRadio
            :model-value="anchorKind" :options="['corner', 'smooth', 'independent']" :labels="['角', 'スムーズ', '独立']"
            @update:model-value="setKind"
          />
        </Parameter>
      </ParameterGrid>
      <div class="buttons">
        <InputButton subtle icon="material-symbols:content-cut" label="点で分離" :disabled="ed.anchors.length !== 1" @click="anchorOp('separate', {anchor: ed.anchors[0]})" />
        <InputButton subtle icon="material-symbols:link" label="端点を連結" :disabled="ed.anchors.length !== 2" @click="anchorOp('join')" />
        <InputButton subtle icon="material-symbols:horizontal-rule" label="直線にする" :disabled="!segment" :tooltip="'隣り合う2点を選ぶと、その間の線分を直線にします'" @click="straighten" />
        <InputButton subtle icon="material-symbols:delete-outline" :label="`点を削除（${ed.anchors.length}）`" tooltip="Delete" @click="anchorOp('deleteAnchors')" />
      </div>
      <ParameterGrid v-if="cornerTargets.length">
        <Parameter label="角丸" hint="キャンバスの角の内側の丸をドラッグしても変えられます（Option＋クリックで種類）">
          <LengthField data-field="corner-radius" :model-value="cornerRadius" :min="0" @focus="begin" @update:model-value="setCornerRadius" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="角の種類">
          <InputRadio data-field="corner-kind" :model-value="cornerKind" :options="['Circular', 'Clothoid', 'Bevel']" :labels="['円弧', 'クロソイド', '面取り']" @update:model-value="v => setCornerKind(v as CornerWidget['kind'])" />
        </Parameter>
      </ParameterGrid>
      <ParameterGrid v-if="ed.anchors.length >= 2">
        <Parameter label="マージ先">
          <InputDropdown v-model="mergeTarget" :options="['center', 'first', 'last']" :labels="['中心', '最初に選んだ点', '最後に選んだ点']" />
        </Parameter>
      </ParameterGrid>
      <div v-if="ed.anchors.length >= 2" class="buttons">
        <InputButton subtle icon="material-symbols:join-inner" label="点をマージ" @click="anchorOp('merge', {target: mergeTarget})" />
      </div>
    </template>

    <!-- width point -->
    <template v-if="widthPoint">
      <ParameterGrid>
        <ParameterHeading>{{ widthPoint.target === 'start' ? '始点の太さ' : widthPoint.target === 'end' ? '終点の太さ' : '幅点' }}</ParameterHeading>
        <Parameter label="太さ" hint="左右に同じ量を足します（左右差は保つ）。キャンバスで端を普通にドラッグするのと同じ">
          <LengthField data-field="width-total" :model-value="widthPoint.h.left + widthPoint.h.right" :min="0" :max="2000" @focus="begin" @update:model-value="setTotal" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="左だけ" hint="Option＋左の端をドラッグと同じ"><LengthField data-field="width-left" :model-value="widthPoint.h.left" :min="0" :max="1000" @focus="begin" @update:model-value="v => setWidth({left: v})" @confirm="commit" @blur="commit" /></Parameter>
        <Parameter label="右だけ" hint="Option＋右の端をドラッグと同じ"><LengthField data-field="width-right" :model-value="widthPoint.h.right" :min="0" :max="1000" @focus="begin" @update:model-value="v => setWidth({right: v})" @confirm="commit" @blur="commit" /></Parameter>
        <Parameter label="方向" hint="幅を測る向きを芯の法線から傾けます。つまみを回すか数字をドラッグ"><InputAngle data-field="width-angle" :model-value="r2(widthPoint.h.angle)" :min="-89" :max="89" @focus="begin" @update:model-value="v => setWidth({angle: v})" @confirm="commit" @blur="commit" /></Parameter>
      </ParameterGrid>
      <div v-if="typeof widthPoint.target === 'number'" class="buttons">
        <InputButton subtle icon="material-symbols:delete-outline" label="幅点を削除" tooltip="Delete" @click="deleteWidthPoint" />
      </div>
    </template>

    <!-- paths -->
    <template v-if="strokes.length && active">
      <ParameterGrid>
        <ParameterHeading>{{ strokes.length > 1 ? `パス（${strokes.length}本）` : 'パス' }}</ParameterHeading>
        <Parameter label="名前" :hint="strokes.length > 1 ? '編集中のパス（最後に選択）の名前' : undefined">
          <InputString data-field="name" :model-value="active.name" @focus="begin" @update:model-value="v => { ed.begin(); active!.name = v; ed.changed() }" @confirm="commit" @blur="commit" />
        </Parameter>
        <template v-if="!onlyImages">
        <PaintRows :ed="ed" paint="fill" label="塗り" :strokes="strokes" :active="active" />
        <PaintRows :ed="ed" paint="edge" label="線" :strokes="strokes" :active="active" />
        <Parameter label="線幅" :hint="strokes.length > 1 ? `選択中の${strokes.length}本すべてを、それぞれの幅点の比率を保ったまま同じ最大幅にします（表示は編集中のパス）` : '幅点の比率を保ったまま、いちばん太い所の幅を変えます'">
          <LengthField data-field="line-width" :model-value="lineWidth" :min="0" :max="1000" @focus="begin" @update:model-value="setLineWidth" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="角">
          <InputRadio data-field="join" :model-value="join" :options="['Miter', 'Round', 'Bevel']" :labels="['鋭角', '丸', '面取り']" @update:model-value="v => setJoin(v as Join)" />
        </Parameter>
        <Parameter v-if="openPaths.length" label="線端" hint="開いたパスの始点・終点の形">
          <InputRadio data-field="caps" :model-value="capStyle" :options="['Butt', 'Round', 'Square']" :labels="['平ら', '丸', '四角']" @update:model-value="v => setCaps(v as Cap)" />
        </Parameter>
        </template>
        <template v-if="capInfo">
          <Parameter v-for="end in (['start', 'end'] as const)" :key="end" :label="end === 'start' ? '始点の切り口' : '終点の切り口'" hint="⌘を押しながら端点をドラッグして回転（Shiftで刻み）。−で芯に直角へ戻す">
            <div class="cap">
              <InputAngle :data-field="`cap-${end}`" :model-value="r2(end === 'start' ? capInfo.start : capInfo.end)" :min="0" :max="180" @focus="begin" @update:model-value="v => setCap(end, v)" @confirm="commit" @blur="commit" />
              <InputButton v-if="end === 'start' ? capInfo.startLocked : capInfo.endLocked" narrow subtle icon="material-symbols:remove" tooltip="芯に直角へ戻す" aria-label="芯に直角へ戻す" @click="() => { setCap(end, null); commit() }" />
            </div>
          </Parameter>
        </template>
      </ParameterGrid>
      <ParameterGrid v-if="text" data-testid="text-props">
        <ParameterHeading>文字</ParameterHeading>
        <Parameter label="フォント">
          <InputDropdown data-field="font" :model-value="text.font" :options="fontOptions" @update:model-value="v => setText({font: v as string}, true)" />
        </Parameter>
        <Parameter label="サイズ">
          <InputNumber data-field="font-size" :model-value="r2(text.size)" :min="1" :max="2000" :bar="false" suffix=" pt" @focus="begin" @update:model-value="v => setText({size: v})" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="行送り" hint="文字の大きさに対する行の高さ">
          <InputNumber data-field="line-height" :model-value="r2(text.line_height)" :min="0.5" :max="5" :step="0.05" :bar="false" @focus="begin" @update:model-value="v => setText({line_height: v})" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="揃え" :hint="onPath ? 'パス上の位置を基準に揃える（両端：パスの終わりまで広げる）' : '文字の基準点を動かさずに揃える（両端：最後の行以外を最も長い行に合わせる）'">
          <InputRadio data-field="text-align" :model-value="text.align ?? 'left'" :options="[...ALIGNS]" :labels="['左', '中央', '右', '両端']" :icons="['material-symbols:format-align-left', 'material-symbols:format-align-center', 'material-symbols:format-align-right', 'material-symbols:format-align-justify']" @update:model-value="v => setText({align: v as TextSpec['align']}, true)" />
        </Parameter>
        <Parameter label="字送り" hint="文字の間隔。1/1000 em 単位で、＋で広げ −で詰める">
          <InputNumber data-field="tracking" :model-value="Math.round(text.tracking ?? 0)" :min="-1000" :max="10000" :step="10" :bar="false" @focus="begin" @update:model-value="v => setText({tracking: v})" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="ベースライン" hint="文字を上下にずらす（＋で上）">
          <InputNumber data-field="baseline-shift" :model-value="r2(text.baseline_shift ?? 0)" :min="-1000" :max="1000" :bar="false" suffix=" pt" @focus="begin" @update:model-value="v => setText({baseline_shift: v})" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="カーニング" hint="フォントに入っている、文字の組み合わせごとの詰め">
          <InputCheckbox data-field="kerning" :model-value="text.kerning !== false" label="フォントのカーニング" @update:model-value="v => setText({kerning: v}, true)" />
        </Parameter>
        <template v-if="onPath">
          <Parameter label="パス上の位置" hint="パスの長さに対する位置。揃えの基準になる（左揃えなら書き出し）">
            <InputNumber data-field="path-offset" :model-value="r2(onPath.offset * 100)" :min="0" :max="100" suffix="%" @focus="begin" @update:model-value="v => setOnPath({offset: v / 100})" @confirm="commit" @blur="commit" />
          </Parameter>
          <Parameter label="反対側" hint="パスの反対側に、逆向きに並べる">
            <InputCheckbox data-field="path-flip" :model-value="!!onPath.flip" label="反対側に並べる" @update:model-value="v => { setOnPath({flip: v}); commit() }" />
          </Parameter>
        </template>
      </ParameterGrid>
      <div v-if="text" class="buttons">
        <InputButton subtle icon="material-symbols:edit-outline" label="文字を打ち直す" tooltip="キャンバスで文字をダブルクリックしても同じ" @click="editTextOnCanvas" />
        <InputButton subtle icon="material-symbols:format-shapes-outline" label="アウトライン化" tooltip="⇧⌘O：文字をただの形にする（打ち直せなくなる）" data-testid="outline-text" @click="outlineTexts" />
        <InputButton v-if="carrier" subtle class="wide" icon="material-symbols:route-outline" label="パスに沿わせる" tooltip="選んだパスに沿って文字を並べる（パスを変えると文字も追従）" data-testid="attach-path" @click="attachToPath" />
        <InputButton v-if="onPath" subtle class="wide" icon="material-symbols:link-off" label="パスから外す" tooltip="ふつうの文字に戻す（パスはそのまま残る）" data-testid="detach-path" @click="detachFromPath" />
      </div>
      <ParameterGrid v-if="image" data-testid="image-props">
        <ParameterHeading>配置画像</ParameterHeading>
        <Parameter label="不透明度">
          <InputNumber data-field="image-opacity" :model-value="Math.round(image.opacity * 100)" :min="0" :max="100" suffix="%" @focus="begin" @update:model-value="v => setImage({opacity: v / 100})" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="抜き" hint="黒：暗いところほど透ける（光や炎の素材を黒背景ごと重ねる・Unmult）／白：白い紙が透けて線だけ残る（スキャンした線画）。元の画素はそのまま">
          <InputRadio data-field="knockout" :model-value="knockout" :options="['none', 'black', 'white']" :labels="['なし', '黒', '白']" @update:model-value="v => setKnockout(v as Knockout | 'none')" />
        </Parameter>
        <Parameter label="下絵" hint="なぞるための画像にする：書き出されず、キャンバスではクリックしても選ばれない（レイヤーから選ぶ）">
          <InputCheckbox data-field="reference" :model-value="!!image.reference" label="下絵にする" @update:model-value="setReference" />
        </Parameter>
      </ParameterGrid>
      <ParameterGrid v-if="blend" data-testid="blend-props">
        <ParameterHeading>ブレンド</ParameterHeading>
        <Parameter label="段数" hint="2つのパスの間に作る形の数。端の形・色・位置を変えると間も追従します">
          <InputNumber data-field="blend-steps" :model-value="blend.steps" :min="1" :max="200" :step="1" :bar="false" @focus="begin" @update:model-value="v => ed.setBlendSteps(blend!.id, v)" @confirm="commit" @blur="commit" />
        </Parameter>
      </ParameterGrid>
      <div v-if="blend" class="buttons">
        <InputButton subtle icon="material-symbols:link-off" label="ブレンドを解除" tooltip="⌥⇧⌘B：間の形を消して2つに戻す" @click="ed.releaseBlend(blend.id)" />
        <InputButton subtle icon="material-symbols:call-split" label="拡張" tooltip="間の形を普通のパスにする（フォルダにまとまる）" data-testid="blend-expand" @click="ed.expandBlend(blend.id)" />
      </div>
      <div v-if="canBlend" class="buttons">
        <InputButton subtle class="wide" icon="material-symbols:blur-linear" label="ブレンド（2つの間をつなぐ）" tooltip="⌥⌘B" data-testid="make-blend" @click="ed.makeBlend()" />
      </div>
      <ParameterGrid v-if="strokes.length >= 2">
        <Parameter label="パスファインダー" hint="選んだパスの見えている輪郭どうしを合成します（効果は焼き込み）。前後は重なり順">
          <div class="pathfinder" data-testid="pathfinder">
            <InputButton
              v-for="p in PATHFINDER" :key="p.op" subtle narrow :icon="p.icon" :aria-label="p.label" :tooltip="p.label" :data-op="p.op"
              @click="runPathfinder(p.op)"
            />
          </div>
        </Parameter>
      </ParameterGrid>
      <ParameterGrid v-if="bounds && at">
        <ParameterHeading>
          位置とサイズ
          <template #right><PivotPicker v-tooltip="'基準点：位置はこの点の座標、サイズと回転はこの点を動かさずに変わる。キャンバスの十字をドラッグしても動かせる（パスや角のハンドルと重なっているときは⌘を押しながら。ダブルクリックで中心）'" :model-value="pivot" @update:model-value="canvas.setPivot" /></template>
        </ParameterHeading>
        <Parameter label="位置" hint="基準点の位置。左の点の格子をドラッグすると縦横に一度に動かせます">
          <LengthPair kind="position" data-field="position" :model-value="[at.x, at.y]" @focus="beginTransform" @update:model-value="v => setPosition(v as [number, number])" @confirm="endTransform" @blur="endTransform" />
        </Parameter>
        <Parameter label="サイズ" hint="鎖のマークで縦横比の固定を切り替えます">
          <LengthPair kind="size" data-field="size" :model-value="[bounds.x1 - bounds.x0, bounds.y1 - bounds.y0]" @focus="beginTransform" @update:model-value="v => setSize(v as [number, number])" @confirm="endTransform" @blur="endTransform" />
        </Parameter>
        <Parameter label="回転" hint="基準点を中心に回す（回し終わると0に戻る）">
          <InputAngle data-field="rotate" :model-value="turn" @focus="beginTransform" @update:model-value="setTurn" @confirm="endTurn" @blur="endTurn" />
        </Parameter>
      </ParameterGrid>
      <ParameterGrid v-if="showAlign" data-testid="align">
        <Parameter label="整列" :hint="units.length < 2 ? 'ひとつだけのときはアートボードに揃えます' : undefined">
          <div class="icon-row">
            <InputButton
              v-for="a in ALIGN" :key="a.kind" subtle narrow :icon="a.icon" :aria-label="a.label" :tooltip="a.label" :data-align="a.kind"
              @click="canvas.align(a.kind)"
            />
          </div>
        </Parameter>
        <template v-if="units.length >= 2">
          <Parameter label="分布">
            <div class="icon-row">
              <InputButton
                v-for="a in DISTRIBUTE" :key="a.kind" subtle narrow :icon="a.icon" :aria-label="a.label" :tooltip="a.label" :data-distribute="a.kind"
                @click="canvas.distribute(a.kind)"
              />
            </div>
          </Parameter>
          <Parameter label="基準" hint="キー：最後にクリックしたもの（太枠）を動かさずに揃える。選択中のものをクリックし直すとキーが変わります">
            <InputDropdown data-field="align-to" :model-value="alignTo" :options="['selection', 'key', 'artboard']" :labels="['選択範囲', 'キー', 'アートボード']" @update:model-value="v => setAlignTo(v as string)" />
          </Parameter>
          <Parameter v-if="alignTo === 'key'" label="間隔" hint="キーを基準に、すき間をこの幅にそろえる（間隔を揃えるボタン）">
            <InputNumber data-field="align-gap" :model-value="canvas.state.alignGap" :bar="false" @update:model-value="v => { canvas.state.alignGap = v }" />
          </Parameter>
        </template>
      </ParameterGrid>
      <div class="buttons">
        <InputButton v-if="!onlyImages" subtle class="wide" icon="material-symbols:account-tree-outline" :label="active.appearance ? '外観ノードを編集' : '外観ノードを開く'" @click="emit('openNodes')" />
        <InputButton subtle icon="material-symbols:content-copy-outline" label="複製" tooltip="⌘D" @click="ed.duplicateSelection()" />
        <InputButton subtle icon="material-symbols:delete-outline" :label="strokes.length > 1 ? `パスを削除（${strokes.length}本）` : 'パスを削除'" @click="deletePaths" />
      </div>
    </template>

    <template v-if="!strokes.length">
      <!-- Nothing selected: the look the next path gets (no prose; tooltip explains). -->
      <ParameterGrid data-testid="draw-style" v-tooltip="'次に描くパスの見た目（最後に選んだパスから引き継ぎ）'">
        <Parameter label="塗り"><PaintField label="塗り" :value="style.fill" :mixed="false" :opacity="style.fillOpacity" @begin="() => {}" @change="(v, o) => setStyle({fill: v, fillOpacity: o})" @commit="() => {}" /></Parameter>
        <Parameter label="線"><PaintField label="線" :value="style.edge" :mixed="false" :opacity="style.edgeOpacity" @begin="() => {}" @change="(v, o) => setStyle({edge: v, edgeOpacity: o})" @commit="() => {}" /></Parameter>
        <Parameter label="線幅"><LengthField data-field="draw-width" :model-value="style.width" :min="0.1" :max="1000" @update:model-value="v => setStyle({width: v})" /></Parameter>
      </ParameterGrid>
    </template>
  </div>
</template>

<style scoped>
.inspector {padding: 10px 10px 16px; display: flex; flex-direction: column; gap: 12px}
.buttons {display: grid; grid-template-columns: 1fr 1fr; gap: var(--tq-gap-group, 4px)}
.buttons :deep(.TqInputButton) {justify-content: flex-start}
.buttons .wide {grid-column: 1 / -1}
.pathfinder {display: flex; gap: 2px; width: 100%}
.pathfinder :deep(.TqInputButton) {flex: 1; font-size: 16px}
.icon-row {display: flex; gap: 2px; width: 100%}
.icon-row :deep(.TqInputButton) {flex: 1; min-width: 0; font-size: 16px}
.cap {display: flex; gap: var(--tq-gap-related, 2px); width: 100%}
.cap > :first-child {flex: 1; min-width: 0}
.empty {color: var(--tq-color-text-mute); font-size: 12px; line-height: 1.7; margin: 8px 2px}
</style>
