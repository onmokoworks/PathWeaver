<script setup lang="ts">
// A length: Tweeq's number field showing and taking the current unit, while
// the value in and out stays in document units (pt). Typed text may carry its
// own unit (`10mm`, `0.5in + 2pt`) — see core/units.ts.
import {computed, provide} from 'vue'
import InputNumber from '../../vendor/tweeq/src/InputNumber/InputNumber.vue'
import {InputNumberContextKey} from '../../vendor/tweeq/src/InputNumber/context'
import {display, fromUnit, roundTo, toUnit, unitContext, UNIT_DIGITS, type Unit} from '../core/units'

const props = withDefaults(defineProps<{modelValue: number; min?: number; max?: number; bar?: boolean; unit?: Unit}>(), {bar: false})
const emit = defineEmits<{'update:modelValue': [v: number]; focus: []; blur: []; confirm: []}>()
const unit = computed(() => props.unit ?? display.unit)
provide(InputNumberContextKey, computed(() => unitContext(unit.value)))
const shown = computed(() => roundTo(toUnit(props.modelValue, unit.value), UNIT_DIGITS[unit.value]))
const lim = (v: number | undefined) => (v === undefined ? undefined : toUnit(v, unit.value))
</script>

<template>
  <InputNumber
    :model-value="shown" :min="lim(min)" :max="lim(max)" :bar="bar" :data-unit="unit"
    @update:model-value="v => emit('update:modelValue', fromUnit(v, unit))"
    @focus="emit('focus')" @blur="emit('blur')" @confirm="emit('confirm')"
  />
</template>
