<script setup lang="ts">
// One node. Every value is edited in place with the real Tweeq inputs.
import {computed} from 'vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputAngle from '../../vendor/tweeq/src/InputAngle/InputAngle.vue'
import InputColor from '../../vendor/tweeq/src/InputColor/InputColor.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import CycleSelect from './CycleSelect.vue'
import {Icon} from '../../vendor/tweeq/src/Icon'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import {pictureUrl} from './picture'
import type {GraphNode, ParamKey, RGB} from '../core/types'
import {BAND, BOOLEAN, GRAIN, categoryOf, hasOutput, inputLabels, JOINS, MATH, nodeInfo, outputsNumber} from './catalog'

const props = defineProps<{
  node: GraphNode
  /** Document revision: the node object is edited in place. */
  rev: number
  selected: boolean
  active: boolean
  isOutput: boolean
  effective: Map<string, number>
  connectedOutput: boolean
}>()
const emit = defineEmits<{
  param: [key: ParamKey, value: number]
  set: [patch: (n: GraphNode) => void]
  begin: []
  commit: []
  bypass: []
  choosePicture: []
}>()

const info = computed(() => { void props.rev; return nodeInfo(props.node) })
const category = computed(() => categoryOf(props.node.kind))
const glowSide = computed(() => { void props.rev; return props.node.both ? 'both' : props.node.inner ? 'inner' : 'outer' })
const title = computed(() => { void props.rev; return props.node.label || info.value.label })
const bound = (key: ParamKey) => props.node.bindings.find(b => b[0] === key)
const hex = (c: RGB) => '#' + c.map(v => v.toString(16).padStart(2, '0')).join('')
const rgb = (s: string): RGB => [1, 3, 5].map(i => parseInt(s.slice(i, i + 2), 16)) as RGB
const isInt = (key: ParamKey) => key === 'Seed' || (props.node.kind === 'Trace' && key === 'Spread')
const precision = (min: number, max: number) => (max - min <= 1 ? 2 : max - min <= 100 ? 1 : 0)
function fmt(v: number | undefined) {
  if (v === undefined) return '計算不可'
  return Number.isInteger(v) ? String(v) : v.toFixed(2)
}
</script>

<template>
  <div
    class="node" :data-node="node.id"
    :class="{selected, active, bypass: node.bypass, output: isOutput}"
    :style="{left: node.position[0] + 'px', top: node.position[1] + 'px', '--cat': category.color}"
  >
    <header class="head" data-drag-handle>
      <span class="cat-dot" :title="category.label" />
      <span class="title" :title="title">{{ title }}</span>
      <button
        v-if="node.kind !== 'Input' && node.kind !== 'Output'" class="bypass-btn" type="button"
        :class="{on: node.bypass}" :title="node.bypass ? 'バイパス中（クリックで有効）' : 'バイパス（M）'"
        @pointerdown.stop @click.stop="emit('bypass')"
      ><Icon icon="material-symbols:power-settings-new" /></button>
      <span
        v-if="hasOutput(node.kind)" class="socket out" :class="{number: outputsNumber(node.kind), linked: connectedOutput}"
        :data-socket="`out:${node.id}`" :title="outputsNumber(node.kind) ? '数値出力' : '形の出力'"
      />
    </header>

    <div v-if="node.kind === 'Image'" class="picture" data-nodrag>
      <img v-if="node.picture" :src="pictureUrl(node.picture)" :alt="node.picture.name" class="thumb" draggable="false">
      <div v-else class="drop">画像ファイルをここへドロップ</div>
      <div class="pic-row">
        <span class="pic-name" :title="node.picture ? `${node.picture.name}（${node.picture.width}×${node.picture.height}px）` : ''">{{ node.picture ? node.picture.name : '未選択' }}</span>
        <InputButton subtle narrow icon="material-symbols:image-outline" :label="node.picture ? '差し替え' : '画像を選ぶ…'" @click="emit('choosePicture')" />
      </div>
    </div>
    <div v-for="(src, i) in node.inputs" :key="`in${i}`" class="row input-row">
      <span class="socket in" :class="{linked: src !== null}" :data-socket="`in:${node.id}:${i}`" title="形の入力" />
      <span class="label">{{ inputLabels(node)[i] }}</span>
    </div>

    <div v-for="p in info.params" :key="p.key" class="row param-row" data-nodrag>
      <span class="socket in number" :class="{linked: !!bound(p.key)}" :data-socket="`param:${node.id}:${p.key}`" title="数値をつなぐ" />
      <template v-if="bound(p.key)">
        <span class="label">{{ p.label }}</span>
        <span class="linked-value" :title="'接続元で調整'">↗ {{ fmt(effective.get(`${node.id}:${p.key}`)) }}</span>
      </template>
      <template v-else-if="p.key === 'Angle' || p.key === 'Rotation'">
        <span class="label">{{ p.label }}</span>
        <InputAngle
          class="num" :model-value="p.value" :min="p.min" :max="p.max" :aria-label="p.label" :data-param="p.key"
          @focus="emit('begin')" @update:model-value="v => emit('param', p.key, v)"
          @confirm="emit('commit')" @blur="emit('commit')"
        />
      </template>
      <!-- A bounded value shows Tweeq's bar: how far along its range it is. -->
      <InputNumber
        v-else class="num" :model-value="p.value" :min="p.min" :max="p.max"
        :step="isInt(p.key) ? 1 : undefined" :precision="precision(p.min, p.max)"
        :prefix="p.label + ' '" :aria-label="p.label"
        :data-param="p.key"
        @focus="emit('begin')" @update:model-value="v => emit('param', p.key, v)"
        @confirm="emit('commit')" @blur="emit('commit')"
      />
    </div>

    <div v-if="node.kind === 'Fill' || node.kind === 'Shadow' || node.kind === 'Glow'" class="row" data-nodrag>
      <span class="label">色</span>
      <InputColor
        class="grow" :model-value="hex(node.color)" :alpha="false" aria-label="色"
        @focus="emit('begin')" @update:model-value="v => emit('set', n => { n.color = rgb(v) })"
        @confirm="emit('commit')" @blur="emit('commit')"
      />
    </div>
    <label v-if="node.kind === 'Rough' || node.kind === 'Zigzag'" class="row check" data-nodrag>
      <InputCheckbox :model-value="node.smooth" @update:model-value="v => { emit('set', n => { n.smooth = v }); emit('commit') }" />
      <span>滑らか</span>
    </label>
    <div v-if="node.kind === 'Glow'" class="row" data-nodrag>
      <span class="label">向き</span>
      <CycleSelect
        class="grow" :model-value="glowSide" :options="['outer', 'inner', 'both']" :labels="['外側', '内側', '両方']" aria-label="光彩の向き"
        @update:model-value="v => { emit('set', n => { n.both = v === 'both'; n.inner = v === 'inner' }); emit('commit') }"
      />
    </div>
    <label v-if="node.kind === 'Glow'" class="row check" data-nodrag>
      <InputCheckbox :model-value="node.keep_source !== false" @update:model-value="v => { emit('set', n => { n.keep_source = v }); emit('commit') }" />
      <span>元の形を残す</span>
    </label>
    <div v-if="node.kind === 'Noise' || node.kind === 'Threshold' || node.kind === 'Trace'" class="row" data-nodrag>
      <span class="label" title="不透明度：色に関係なく、描かれている所すべて。暗さ：白い紙に置いたときの暗さ（上に重ねた白はインクを消す）">判定</span>
      <CycleSelect
        class="grow" :model-value="node.by_darkness ? 'dark' : 'alpha'" :options="['alpha', 'dark']" :labels="['不透明度', '暗さ']" aria-label="判定"
        @update:model-value="v => { emit('set', n => { n.by_darkness = v === 'dark' }); emit('commit') }"
      />
    </div>
    <label v-if="node.kind === 'Measure'" class="row check" data-nodrag>
      <InputCheckbox :model-value="node.measure_height" @update:model-value="v => { emit('set', n => { n.measure_height = v }); emit('commit') }" />
      <span>高さを計測</span>
    </label>
    <div v-if="node.kind === 'Boolean'" class="row" data-nodrag>
      <CycleSelect class="grow" :model-value="node.operation" :options="BOOLEAN.map(o => o[0])" :labels="BOOLEAN.map(o => o[1])" @update:model-value="v => { emit('set', n => { n.operation = v as GraphNode['operation'] }); emit('commit') }" />
    </div>
    <div v-if="node.kind === 'Math'" class="row" data-nodrag>
      <CycleSelect class="grow" :model-value="node.math" :options="MATH.map(o => o[0])" :labels="MATH.map(o => o[1])" @update:model-value="v => { emit('set', n => { n.math = v }); emit('commit') }" />
    </div>
    <div v-if="node.kind === 'Offset' || node.kind === 'Round' || node.kind === 'Band'" class="row" data-nodrag>
      <CycleSelect class="grow" :model-value="node.effect.join" :options="JOINS.map(o => o[0])" :labels="JOINS.map(o => o[1])" @update:model-value="v => { emit('set', n => { n.effect.join = v as GraphNode['effect']['join'] }); emit('commit') }" />
    </div>
    <div v-if="node.kind === 'Mezzotint'" class="row" data-nodrag>
      <span class="label">粒</span>
      <CycleSelect class="grow" :model-value="node.effect.grain ?? 'Dots'" :options="GRAIN.map(o => o[0])" :labels="GRAIN.map(o => o[1])" aria-label="粒の形" @update:model-value="v => { emit('set', n => { n.effect.grain = v as 'Dots' | 'Lines' }); emit('commit') }" />
    </div>
    <div v-if="node.kind === 'Band'" class="row" data-nodrag>
      <CycleSelect class="grow" :model-value="node.effect.position" :options="BAND.map(o => o[0])" :labels="BAND.map(o => o[1])" @update:model-value="v => { emit('set', n => { n.effect.position = v as GraphNode['effect']['position'] }); emit('commit') }" />
    </div>
  </div>
</template>

<style scoped>
.node {
  position: absolute; width: 200px; border-radius: 4px;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border);
  box-shadow: 0 1px 3px var(--tq-color-shadow, rgba(0,0,0,.12));
  font-size: 11px; color: var(--tq-color-text); padding-bottom: 4px;
}
.node.selected {border-color: var(--tq-color-text-mute)}
.node.active {border-color: var(--tq-color-accent); box-shadow: 0 0 0 1px var(--tq-color-accent), 0 2px 8px var(--tq-color-shadow, rgba(0,0,0,.15))}
.node.bypass {opacity: .55}
.head {
  position: relative; display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 6px 0 8px;
  border-radius: 3px 3px 0 0; background: var(--tq-color-background);
  border-bottom: 1px solid var(--tq-color-border);
  cursor: grab;
}
.cat-dot {width: 6px; height: 6px; border-radius: 1px; background: var(--cat); flex: none}
.title {flex: 1; min-width: 0; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis}
.bypass-btn {
  display: grid; place-items: center; width: 18px; height: 18px; border: 0; border-radius: 50%;
  background: transparent; color: var(--tq-color-text-mute); cursor: pointer; padding: 0; font-size: 13px;
}
.bypass-btn:hover {background: var(--tq-color-input-hover)}
.bypass-btn.on {color: var(--tq-color-error, #d24a3c)}
.row {position: relative; display: flex; align-items: center; gap: 6px; min-height: 26px; padding: 2px 10px}
.row .label {color: var(--tq-color-text-mute); white-space: nowrap}
.row .num, .row .grow {flex: 1; min-width: 0}
.check {cursor: pointer}
.picture {padding: 6px 10px 2px; display: flex; flex-direction: column; gap: 4px}
.thumb {display: block; width: 100%; max-height: 120px; object-fit: contain; border-radius: 2px;
  background: repeating-conic-gradient(#e4e4e4 0 25%, #fff 0 50%) 0 0 / 12px 12px}
.drop {height: 56px; display: grid; place-items: center; border: 1px dashed var(--tq-color-border); border-radius: 3px; color: var(--tq-color-text-mute); font-size: 10px}
.pic-row {display: flex; align-items: center; gap: 4px}
.pic-name {flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--tq-color-text-mute)}
.linked-value {margin-left: auto; color: var(--tq-color-accent); font-variant-numeric: tabular-nums}
.socket {
  position: absolute; width: 10px; height: 10px; border-radius: 50%;
  background: var(--tq-color-background); border: 2px solid #4f9a6b; cursor: crosshair; box-sizing: border-box;
}
.socket.number {border-color: #4f7fc4; border-radius: 2px}
.socket.linked {background: currentColor}
.socket.linked:not(.number) {background: #4f9a6b}
.socket.linked.number {background: #4f7fc4}
.socket.in {left: -6px; top: 50%; margin-top: -5px}
.socket.out {right: -6px; top: 50%; margin-top: -5px}
.socket:hover {transform: scale(1.3)}
</style>
