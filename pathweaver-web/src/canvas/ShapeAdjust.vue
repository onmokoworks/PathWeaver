<script setup lang="ts">
// Adjust Last Operation (Blender's redo panel, F9): right after a shape is
// drawn, its numbers stay open here — size, sides, corners, spiral decay.
// Changing them rebuilds the same shape in the same undo step. It closes as
// soon as anything else changes the document (the shape is then just a path).
import {computed} from 'vue'
import type {Canvas} from './controller'
import type {Editor} from '../core/editor'
import {shapeInfo, type ShapeParams} from '../core/shapes'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputAngle from '../../vendor/tweeq/src/InputAngle/InputAngle.vue'
import InputSize from '../../vendor/tweeq/src/InputSize/InputSize.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const p = computed(() => props.canvas.state.lastShape!.params)
const set = (patch: Partial<ShapeParams>) => props.canvas.adjustShape(patch)
const r2 = (v: number) => Math.round(v * 100) / 100

type Field = {key: string; label: string; value: number; min?: number; max?: number; step?: number; suffix?: string; angle?: boolean; bar?: boolean; apply: (v: number) => Partial<ShapeParams>}
type Toggle = {key: string; label: string; value: boolean; apply: (v: boolean) => Partial<ShapeParams>}
// W and H travel together (Tweeq's linked size): see `pair` below.
const size = (_q: ShapeParams): Field[] => []
const pair = computed(() => {
  const q = p.value
  return ['rect', 'rounded', 'ellipse', 'arc', 'grid', 'polar'].includes(q.kind) ? [Math.abs(q.w), Math.abs(q.h)] as [number, number] : null
})
function setPair(v: [number, number]) {
  const q = p.value
  set({w: Math.sign(q.w || 1) * Math.max(0.1, v[0]), h: Math.sign(q.h || 1) * Math.max(0.1, v[1])})
}
const fields = computed<Field[]>(() => {
  const q = p.value
  switch (q.kind) {
    case 'rect': case 'rounded': return [{key: 'corner', label: '角丸', value: q.corner, min: 0, apply: v => ({corner: v})}]
    case 'ellipse': case 'arc': return []
    case 'polygon': return [
      {key: 'radius', label: '半径', value: q.radius, min: 0.1, apply: v => ({radius: v})},
      {key: 'sides', label: '辺の数', value: q.sides, min: 3, max: 100, step: 1, apply: v => ({sides: Math.round(v)})},
      {key: 'angle', label: '回転', value: q.angle, angle: true, apply: v => ({angle: v})},
      {key: 'corner', label: '角丸', value: q.corner, min: 0, apply: v => ({corner: v})},
    ]
    case 'star': return [
      {key: 'radius', label: '外の半径', value: q.radius, min: 0.1, apply: v => ({radius: v})},
      {key: 'radius2', label: '内の半径', value: q.radius2, min: 0, apply: v => ({radius2: v})},
      {key: 'sides', label: '頂点の数', value: q.sides, min: 2, max: 100, step: 1, apply: v => ({sides: Math.round(v)})},
      {key: 'angle', label: '回転', value: q.angle, angle: true, apply: v => ({angle: v})},
    ]
    case 'line': {
      const len = Math.hypot(q.w, q.h), ang = Math.atan2(-q.h, q.w) * 180 / Math.PI
      return [
        {key: 'length', label: '長さ', value: len, min: 0.1, apply: v => ({w: Math.cos(-ang * Math.PI / 180) * v, h: Math.sin(-ang * Math.PI / 180) * v})},
        {key: 'angle', label: '角度', value: ang, angle: true, apply: v => ({w: Math.cos(-v * Math.PI / 180) * len, h: Math.sin(-v * Math.PI / 180) * len})},
      ]
    }
    case 'grid': case 'polar': return [

      {key: 'rows', label: q.kind === 'grid' ? '横の分割線' : '同心円の分割', value: q.rows, min: 0, max: 100, step: 1, apply: v => ({rows: Math.round(v)})},
      {key: 'cols', label: q.kind === 'grid' ? '縦の分割線' : '放射の分割', value: q.cols, min: 0, max: 100, step: 1, apply: v => ({cols: Math.round(v)})},
    ]
    case 'spiral': return [
      {key: 'radius', label: '半径', value: q.radius, min: 0.1, apply: v => ({radius: v})},
      {key: 'decay', label: '減衰率', value: q.decay, min: 5, max: 99, suffix: '%', bar: true, apply: v => ({decay: v})},
      {key: 'sides', label: 'セグメント数', value: q.sides, min: 2, max: 200, step: 1, apply: v => ({sides: Math.round(v)})},
    ]
  }
  return []
})
const toggles = computed<Toggle[]>(() => {
  const q = p.value
  if (q.kind === 'arc') return [
    {key: 'concave', label: '凹にする', value: q.concave, apply: v => ({concave: v})},
    {key: 'closed', label: '閉じる', value: q.closed, apply: v => ({closed: v})},
  ]
  if (q.kind === 'spiral') return [{key: 'clockwise', label: '時計回り', value: q.clockwise, apply: v => ({clockwise: v})}]
  return []
})
</script>

<template>
  <div class="adjust" data-testid="shape-adjust" @pointerdown.stop @wheel.stop @keydown.stop>
    <div class="title">{{ shapeInfo(p.kind).label }}</div>
    <div v-if="pair" class="row" data-adjust="size">
      <span>サイズ</span>
      <InputSize :model-value="[r2(pair[0]), r2(pair[1])]" @update:model-value="v => setPair(v as [number, number])" />
    </div>
    <label v-for="f in fields" :key="f.key" class="row" :data-adjust="f.key">
      <span>{{ f.label }}</span>
      <InputAngle v-if="f.angle" :model-value="r2(f.value)" @update:model-value="v => set(f.apply(v))" />
      <InputNumber v-else :model-value="r2(f.value)" :min="f.min" :max="f.max" :step="f.step" :suffix="f.suffix" :bar="!!f.bar" @update:model-value="v => set(f.apply(v))" />
    </label>
    <div v-for="t in toggles" :key="t.key" class="row" :data-adjust="t.key">
      <InputCheckbox :model-value="t.value" :label="t.label" @update:model-value="v => set(t.apply(v))" />
    </div>
  </div>
</template>

<style scoped>
.adjust {
  position: absolute; left: 28px; bottom: 48px; z-index: 6; width: 220px; padding: 6px 8px 8px;
  display: flex; flex-direction: column; gap: 4px; font-size: 11px;
  background: color-mix(in srgb, var(--tq-color-background) 92%, transparent);
  -webkit-backdrop-filter: blur(12px); backdrop-filter: blur(12px);
  border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, .12);
}
.title {font-weight: 600; color: var(--tq-color-text-mute); padding-bottom: 2px}
.row {display: grid; grid-template-columns: 70px 1fr; align-items: center; gap: 6px}
.row:has(.TqInputCheckbox) {grid-template-columns: 1fr}
</style>
