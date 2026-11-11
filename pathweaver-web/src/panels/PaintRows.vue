<script setup lang="ts">
// One paint of the selection (塗り or 線) in the inspector: its colour with
// opacity, the paint type (単色 / 線形 / 円形), and for a gradient its stops
// and angle. Colour, opacity and type apply to every selected path; the
// stops are those of the path being edited (the last selected).
import {computed, ref, watch} from 'vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import InputColor from '../../vendor/tweeq/src/InputColor/InputColor.vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputAngle from '../../vendor/tweeq/src/InputAngle/InputAngle.vue'
import InputRadio from '../../vendor/tweeq/src/InputRadio/InputRadio.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import PaintField from './PaintField.vue'
import GradientBar from './GradientBar.vue'
import type {Editor} from '../core/editor'
import type {Gradient, RGB, Stroke} from '../core/types'
import {gradientAngle, gradientOf, hexAlpha, opacityOf, paintKind, parseHexAlpha, setGradient, setOpacity, setPaintKind, syncFallback, withAngle, type PaintKey, type PaintKind} from '../core/paint'

const props = defineProps<{ed: Editor; paint: PaintKey; label: string; strokes: Stroke[]; active: Stroke}>()
const ed = props.ed
const begin = () => ed.begin()
const commit = () => ed.commit()
const r2 = (v: number) => Math.round(v * 100) / 100

function same<T>(get: (s: Stroke) => T): T | undefined {
  const vals = props.strokes.map(s => JSON.stringify(get(s)))
  return vals.length && vals.every(v => v === vals[0]) ? JSON.parse(vals[0]) : undefined
}
const color = computed(() => { void ed.revision; return same(s => s[props.paint]) })
const opacity = computed(() => { void ed.revision; return same(s => opacityOf(s, props.paint)) ?? 1 })
const kind = computed(() => { void ed.revision; return same(s => (s[props.paint] ? paintKind(s, props.paint) : null)) ?? '' })
/** The edited path's gradient (shown when it has one). */
const gradient = computed(() => { void ed.revision; return props.active[props.paint] ? gradientOf(props.active, props.paint) : null })

function setColor(v: RGB | null, o: number) {
  ed.begin()
  for (const s of props.strokes) {
    s[props.paint] = v ? [...v] as RGB : null
    setOpacity(s, props.paint, o)
    if (!v) setGradient(s, props.paint, null)
  }
  ed.changed()
}
function setKind(k: PaintKind) {
  ed.edit(() => { for (const s of props.strokes) if (s[props.paint]) setPaintKind(s, props.paint, k) })
  stop.value = 0
}

// ---- stops of the edited path's gradient ----
const stop = ref(0)
watch(() => [props.active.id, gradient.value?.stops.length], () => {
  const n = gradient.value?.stops.length ?? 0
  if (stop.value >= n) stop.value = Math.max(0, n - 1)
})
const current = computed(() => gradient.value?.stops[stop.value] ?? null)
function update(fn: (g: Gradient) => void) {
  const g = gradient.value
  if (!g) return
  ed.begin()
  const next = JSON.parse(JSON.stringify(g)) as Gradient
  fn(next)
  setGradient(props.active, props.paint, next)
  syncFallback(props.active, props.paint)
  ed.changed()
}
function setStopColor(v: string) {
  const {color, opacity} = parseHexAlpha(v)
  update(g => { const s = g.stops[stop.value]; if (s) { s.color = color; s.opacity = opacity } })
}
function setStopOffset(v: number) {
  update(g => { const s = g.stops[stop.value]; if (s) s.offset = Math.min(1, Math.max(0, v / 100)) })
}
function deleteStop() {
  if ((gradient.value?.stops.length ?? 0) <= 2) return
  update(g => { g.stops.splice(stop.value, 1) })
  ed.commit()
  stop.value = Math.max(0, stop.value - 1)
}
function setAngle(v: number) {
  update(g => { Object.assign(g, withAngle(g, v)) })
}
function onBar(g: Gradient) {
  setGradient(props.active, props.paint, g)
  syncFallback(props.active, props.paint)
  ed.changed()
}
</script>

<template>
  <Parameter :label="label">
    <div class="paint-rows" :data-paint-rows="paint">
      <div v-if="gradient" class="line">
        <GradientBar :gradient="gradient" :selected="stop" :label="label" @select="i => (stop = i)" @begin="begin" @change="onBar" @commit="commit" />
        <InputButton narrow subtle icon="material-symbols:remove" :tooltip="`${label}をなしにする`" :aria-label="`${label}をなしにする`" @click="() => { setColor(null, 1); commit() }" />
      </div>
      <PaintField
        v-else :label="label" :value="color ?? null" :mixed="color === undefined" :opacity="opacity"
        @begin="begin" @change="setColor" @commit="commit"
      />
      <InputRadio
        v-if="color !== null" :data-field="`${paint}-kind`" :model-value="kind" :options="['Solid', 'Linear', 'Radial']" :labels="['単色', '線形', '円形']"
        @update:model-value="v => setKind(v as PaintKind)"
      />
    </div>
  </Parameter>
  <template v-if="gradient && current">
    <Parameter label="分岐点" :hint="`${label}のグラデーションの、選んでいる分岐点の色と不透明度。バーをクリックで追加、下へドラッグで削除`">
      <div class="line">
        <InputColor
          class="grow" :data-field="`${paint}-stop-color`" :model-value="hexAlpha(current.color, current.opacity)" alpha :aria-label="`${label}の分岐点の色`"
          @focus="begin" @update:model-value="setStopColor" @confirm="commit" @blur="commit"
        />
        <InputButton narrow subtle icon="material-symbols:delete-outline" :disabled="gradient.stops.length <= 2" tooltip="分岐点を削除" aria-label="分岐点を削除" @click="deleteStop" />
      </div>
    </Parameter>
    <Parameter label="位置">
      <InputNumber data-paint-stop-offset :data-field="`${paint}-stop-offset`" :model-value="r2(current.offset * 100)" :min="0" :max="100" suffix="%" @focus="begin" @update:model-value="setStopOffset" @confirm="commit" @blur="commit" />
    </Parameter>
    <Parameter label="角度" hint="グラデーションの向き。ダイレクト選択ツール（A）ならキャンバス上の線をドラッグして位置も変えられます">
      <InputAngle :data-field="`${paint}-angle`" :model-value="r2(gradientAngle(gradient))" @focus="begin" @update:model-value="setAngle" @confirm="commit" @blur="commit" />
    </Parameter>
  </template>
</template>

<style scoped>
.paint-rows {display: grid; gap: var(--tq-gap-related, 2px); min-width: 0}
.line {display: flex; gap: var(--tq-gap-related, 2px); align-items: flex-start; min-width: 0}
.grow {flex: 1; min-width: 0}
</style>
