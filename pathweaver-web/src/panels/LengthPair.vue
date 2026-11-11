<script setup lang="ts">
// Two lengths as Tweeq's position (X, Y with the 2D drag pad) or linked size
// (W, H), in the current unit; values in and out stay in document units (pt).
import {computed, provide} from 'vue'
import InputPosition from '../../vendor/tweeq/src/InputPosition/InputPosition.vue'
import InputSize from '../../vendor/tweeq/src/InputSize/InputSize.vue'
import {InputNumberContextKey} from '../../vendor/tweeq/src/InputNumber/context'
import {display, fromUnit, roundTo, toUnit, unitContext, UNIT_DIGITS, type Unit} from '../core/units'

const props = defineProps<{modelValue: [number, number]; kind: 'position' | 'size'; unit?: Unit}>()
const emit = defineEmits<{'update:modelValue': [v: [number, number]]; focus: []; blur: []; confirm: []}>()
const unit = computed(() => props.unit ?? display.unit)
provide(InputNumberContextKey, computed(() => unitContext(unit.value)))
const shown = computed(() => props.modelValue.map(v => roundTo(toUnit(v, unit.value), UNIT_DIGITS[unit.value])) as [number, number])
// a value left as shown passes through exactly (no rounding drift on the other axis)
const back = (v: number, i: 0 | 1) => (v === shown.value[i] ? props.modelValue[i] : fromUnit(v, unit.value))
const out = (v: readonly number[]) => emit('update:modelValue', [back(v[0], 0), back(v[1], 1)])
</script>

<template>
  <InputPosition
    v-if="kind === 'position'" :model-value="shown" :data-unit="unit"
    @update:model-value="out" @focus="emit('focus')" @blur="emit('blur')" @confirm="emit('confirm')"
  />
  <InputSize
    v-else :model-value="shown" :data-unit="unit"
    @update:model-value="out" @focus="emit('focus')" @blur="emit('blur')" @confirm="emit('confirm')"
  />
</template>
