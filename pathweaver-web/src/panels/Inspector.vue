<script setup lang="ts">
// Properties of the current selection, laid out with Tweeq's ParameterGrid.
// Every field edit is one undo step: focus → previews → confirm/blur.
import {computed, ref} from 'vue'
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import ParameterHeading from '../../vendor/tweeq/src/ParameterGrid/ParameterHeading.vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputString from '../../vendor/tweeq/src/InputString/InputString.vue'
import InputRadio from '../../vendor/tweeq/src/InputRadio/InputRadio.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import InputDropdown from '../../vendor/tweeq/src/InputDropdown/InputDropdown.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import {fontFamilies, relayout} from '../core/textOutline'
import PaintField from './PaintField.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'
import {call} from '../core/wasm'
import type {Cap, CornerWidget, Document, Join, RGB, Stroke} from '../core/types'

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
const fill = computed(() => same(s => s.fill))
const edge = computed(() => same(s => s.edge))
function setPaint(key: 'fill' | 'edge', v: RGB | null) {
  ed.begin()
  for (const s of strokes.value) s[key] = v ? [...v] as RGB : null
  ed.changed()
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
async function setText(patch: {font?: string; size?: number; line_height?: number}, once = false) {
  ed.begin()
  for (const s of texts.value) Object.assign(s.text!, patch)
  ed.changed()
  const done = Promise.all(texts.value.map(s => relayout(ed, s)))
  if (once) { await done; ed.commit() }
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
function setImage(patch: {opacity?: number; reference?: boolean}) {
  ed.begin()
  for (const s of images.value) {
    Object.assign(s.image!, patch)
    if (patch.reference === false) delete s.image!.reference
  }
  ed.changed()
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

// ---- transform via bounds (X, Y, W, H) ----
let base: {doc: string; b: {x0: number; y0: number; x1: number; y1: number}} | null = null
const bounds = computed(() => {
  void ed.revision
  return props.canvas.selectionBounds()
})
function beginTransform() {
  ed.begin()
  const b = bounds.value
  if (b) base = {doc: JSON.stringify(ed.doc.strokes), b}
}
function setBounds(key: 'x' | 'y' | 'w' | 'h', v: number) {
  if (!base) beginTransform()
  if (!base) return
  const b = base.b
  const w = Math.max(b.x1 - b.x0, 1e-9), h = Math.max(b.y1 - b.y0, 1e-9)
  let sx = 1, sy = 1, tx = 0, ty = 0
  if (key === 'x') tx = v - b.x0
  if (key === 'y') ty = v - b.y0
  if (key === 'w') sx = Math.max(v, 0.01) / w
  if (key === 'h') sy = Math.max(v, 0.01) / h
  const m = [sx, 0, 0, sy, b.x0 - sx * b.x0 + tx, b.y0 - sy * b.y0 + ty]
  const d = {...ed.doc, strokes: JSON.parse(base.doc)} as Document
  const r = call<{doc: Document}>('edit', {doc: d, kind: 'transform', ids: ed.selected, matrix: m})
  ed.doc.strokes = r.doc.strokes
  ed.changed()
}
function endTransform() { base = null; ed.commit() }

// ---- anchors ----
const anchorNodes = computed(() => {
  void ed.revision
  return ed.anchors.map(a => ({a, n: ed.findStroke(a.stroke)?.nodes.find(n => n.id === a.node)})).filter(v => v.n)
})
const lastAnchor = computed(() => anchorNodes.value[anchorNodes.value.length - 1])
let anchorBase: {x: number; y: number}[] | null = null
function setAnchor(axis: 'x' | 'y', v: number) {
  const last = lastAnchor.value
  if (!last) return
  if (!anchorBase) { ed.begin(); anchorBase = anchorNodes.value.map(({n}) => ({...n!.p})) }
  const ref = anchorBase[anchorBase.length - 1]
  const delta = v - ref[axis]
  anchorNodes.value.forEach(({n}, i) => { n!.p = {...anchorBase![i], [axis]: anchorBase![i][axis] + delta} })
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
    <!-- anchors -->
    <template v-if="ed.anchors.length && lastAnchor">
      <ParameterGrid>
        <ParameterHeading>アンカー（{{ ed.anchors.length }}点）</ParameterHeading>
        <Parameter label="X"><InputNumber :model-value="r2(lastAnchor.n!.p.x)" :bar="false" @focus="begin" @update:model-value="v => setAnchor('x', v)" @confirm="endAnchor" @blur="endAnchor" /></Parameter>
        <Parameter label="Y"><InputNumber :model-value="r2(lastAnchor.n!.p.y)" :bar="false" @focus="begin" @update:model-value="v => setAnchor('y', v)" @confirm="endAnchor" @blur="endAnchor" /></Parameter>
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
          <InputNumber data-field="corner-radius" :model-value="r2(cornerRadius)" :min="0" :bar="false" @focus="begin" @update:model-value="setCornerRadius" @confirm="commit" @blur="commit" />
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
          <InputNumber :model-value="r2(widthPoint.h.left + widthPoint.h.right)" :min="0" :max="2000" :bar="false" @focus="begin" @update:model-value="setTotal" @confirm="commit" @blur="commit" />
        </Parameter>
        <Parameter label="左だけ" hint="Option＋左の端をドラッグと同じ"><InputNumber :model-value="r2(widthPoint.h.left)" :min="0" :max="1000" :bar="false" @focus="begin" @update:model-value="v => setWidth({left: v})" @confirm="commit" @blur="commit" /></Parameter>
        <Parameter label="右だけ" hint="Option＋右の端をドラッグと同じ"><InputNumber :model-value="r2(widthPoint.h.right)" :min="0" :max="1000" :bar="false" @focus="begin" @update:model-value="v => setWidth({right: v})" @confirm="commit" @blur="commit" /></Parameter>
        <Parameter label="方向" hint="幅を測る向きを芯の法線から傾けます"><InputNumber :model-value="r2(widthPoint.h.angle)" :min="-89" :max="89" suffix="°" :bar="false" @focus="begin" @update:model-value="v => setWidth({angle: v})" @confirm="commit" @blur="commit" /></Parameter>
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
        <Parameter label="塗り"><PaintField label="塗り" :value="fill ?? null" :mixed="fill === undefined" @begin="begin" @change="v => setPaint('fill', v)" @commit="commit" /></Parameter>
        <Parameter label="線"><PaintField label="線" :value="edge ?? null" :mixed="edge === undefined" @begin="begin" @change="v => setPaint('edge', v)" @commit="commit" /></Parameter>
        <Parameter label="線幅" :hint="strokes.length > 1 ? `選択中の${strokes.length}本すべてを、それぞれの幅点の比率を保ったまま同じ最大幅にします（表示は編集中のパス）` : '幅点の比率を保ったまま、いちばん太い所の幅を変えます'">
          <InputNumber :model-value="r2(lineWidth)" :min="0" :max="1000" :bar="false" suffix=" pt" @focus="begin" @update:model-value="setLineWidth" @confirm="commit" @blur="commit" />
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
              <InputNumber :model-value="r2(end === 'start' ? capInfo.start : capInfo.end)" :min="0" :max="180" suffix="°" :bar="false" @focus="begin" @update:model-value="v => setCap(end, v)" @confirm="commit" @blur="commit" />
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
      </ParameterGrid>
      <div v-if="text" class="buttons">
        <InputButton subtle icon="material-symbols:edit-outline" label="文字を打ち直す" tooltip="キャンバスで文字をダブルクリックしても同じ" @click="editTextOnCanvas" />
        <InputButton subtle icon="material-symbols:format-shapes-outline" label="アウトライン化" tooltip="⇧⌘O：文字をただの形にする（打ち直せなくなる）" data-testid="outline-text" @click="outlineTexts" />
      </div>
      <ParameterGrid v-if="image" data-testid="image-props">
        <ParameterHeading>配置画像</ParameterHeading>
        <Parameter label="不透明度">
          <InputNumber data-field="image-opacity" :model-value="Math.round(image.opacity * 100)" :min="0" :max="100" suffix="%" @focus="begin" @update:model-value="v => setImage({opacity: v / 100})" @confirm="commit" @blur="commit" />
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
      <ParameterGrid v-if="bounds">
        <ParameterHeading>位置とサイズ</ParameterHeading>
        <Parameter label="X"><InputNumber data-field="x" :model-value="r2(bounds.x0)" :bar="false" @focus="beginTransform" @update:model-value="v => setBounds('x', v)" @confirm="endTransform" @blur="endTransform" /></Parameter>
        <Parameter label="Y"><InputNumber :model-value="r2(bounds.y0)" :bar="false" @focus="beginTransform" @update:model-value="v => setBounds('y', v)" @confirm="endTransform" @blur="endTransform" /></Parameter>
        <Parameter label="W"><InputNumber :model-value="r2(bounds.x1 - bounds.x0)" :min="0.01" :bar="false" @focus="beginTransform" @update:model-value="v => setBounds('w', v)" @confirm="endTransform" @blur="endTransform" /></Parameter>
        <Parameter label="H"><InputNumber :model-value="r2(bounds.y1 - bounds.y0)" :min="0.01" :bar="false" @focus="beginTransform" @update:model-value="v => setBounds('h', v)" @confirm="endTransform" @blur="endTransform" /></Parameter>
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
        <Parameter label="塗り"><PaintField label="塗り" :value="style.fill" :mixed="false" @begin="() => {}" @change="v => setStyle({fill: v})" @commit="() => {}" /></Parameter>
        <Parameter label="線"><PaintField label="線" :value="style.edge" :mixed="false" @begin="() => {}" @change="v => setStyle({edge: v})" @commit="() => {}" /></Parameter>
        <Parameter label="線幅"><InputNumber data-field="draw-width" :model-value="r2(style.width)" :min="0.1" :max="1000" :bar="false" suffix=" pt" @update:model-value="v => setStyle({width: v})" /></Parameter>
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
.cap {display: flex; gap: var(--tq-gap-related, 2px); width: 100%}
.cap > :first-child {flex: 1; min-width: 0}
.empty {color: var(--tq-color-text-mute); font-size: 12px; line-height: 1.7; margin: 8px 2px}
</style>
