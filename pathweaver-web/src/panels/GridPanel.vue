<script setup lang="ts">
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import ParameterHeading from '../../vendor/tweeq/src/ParameterGrid/ParameterHeading.vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import LengthField from './LengthField.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import {vTooltip} from '../../vendor/tweeq/src/Tooltip'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const ed = props.ed
type Flag = 'snap_grid' | 'snap_guides' | 'snap_tangent' | 'snap_normal'
const flags: [Flag, string, string][] = [
  ['snap_guides', 'ガイド', '定規から引き出したガイドと、その交点に吸着'],
  ['snap_grid', 'グリッド', 'グリッドを表示して交点に吸着'],
  ['snap_tangent', '接線', '他のアンカーの接線方向に吸着'],
  ['snap_normal', '法線', '他のアンカーの法線方向に吸着'],
]
function toggle(f: Flag, v: boolean) { ed.edit(d => { d[f] = v }) }
function setNumber(key: 'grid_spacing' | 'angle_step', v: number) { ed.begin(); ed.doc[key] = v; ed.changed() }
</script>

<template>
  <div class="grid-panel">
    <ParameterGrid>
      <ParameterHeading>吸着</ParameterHeading>
      <Parameter v-for="[f, label, hint] in flags" :key="f" :label="label" :hint="hint">
        <InputCheckbox :model-value="ed.doc[f]" :aria-label="label" @update:model-value="v => toggle(f, v)" />
      </Parameter>
      <Parameter label="グリッド間隔">
        <LengthField data-field="grid-spacing" :model-value="ed.doc.grid_spacing" :min="1" :max="10000" @focus="ed.begin()" @update:model-value="v => setNumber('grid_spacing', v)" @confirm="ed.commit()" @blur="ed.commit()" />
      </Parameter>
      <Parameter label="回転の刻み" hint="Shiftを押しながら回転したときの角度">
        <InputNumber :model-value="ed.doc.angle_step" :min="1" :max="90" suffix="°" :bar="false" @focus="ed.begin()" @update:model-value="v => setNumber('angle_step', v)" @confirm="ed.commit()" @blur="ed.commit()" />
      </Parameter>
    </ParameterGrid>
    <ParameterGrid v-tooltip="'定規からドラッグで作成、定規へ戻すと削除。Controlを押している間は吸着しません'">
      <ParameterHeading>ガイド（{{ ed.doc.guides.length }}本）</ParameterHeading>
    </ParameterGrid>
    <InputButton subtle icon="material-symbols:delete-outline" label="ガイドをすべて削除" :disabled="!ed.doc.guides.length" @click="ed.edit(d => { d.guides = [] })" />
  </div>
</template>

<style scoped>
.grid-panel {padding: 10px; display: flex; flex-direction: column; gap: 10px}
.note {margin: 0; font-size: 11px; line-height: 1.6; color: var(--tq-color-text-mute)}
</style>
