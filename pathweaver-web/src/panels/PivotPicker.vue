<script setup lang="ts">
// The reference point locator (Illustrator's 基準点): nine points of the
// selection's box. A point moved freely on the canvas lights none of them.
import {PIVOT_POINTS, type Pivot} from '../canvas/controller'

const props = defineProps<{modelValue: Pivot}>()
const emit = defineEmits<{'update:modelValue': [Pivot]}>()
const NAMES = ['左上', '上', '右上', '左', '中心', '右', '左下', '下', '右下']
const on = (q: Pivot) => Math.abs(q.u - props.modelValue.u) < 1e-6 && Math.abs(q.v - props.modelValue.v) < 1e-6
</script>

<template>
  <div class="pivot-picker" role="radiogroup" aria-label="基準点" data-testid="pivot-picker">
    <button
      v-for="(q, i) in PIVOT_POINTS" :key="i" type="button" role="radio" :aria-checked="on(q)" :aria-label="`基準点：${NAMES[i]}`"
      :title="`基準点：${NAMES[i]}`" :data-pivot="`${q.u} ${q.v}`" :class="{on: on(q)}"
      @click="emit('update:modelValue', q)"
    ><span /></button>
  </div>
</template>

<style scoped>
.pivot-picker {
  display: grid; grid-template-columns: repeat(3, 8px); grid-template-rows: repeat(3, 8px); gap: 0;
  padding: 2px; border-radius: 3px; box-shadow: inset 0 0 0 1px var(--tq-color-border);
}
button {display: flex; align-items: center; justify-content: center; width: 8px; height: 8px; padding: 0; border: 0; background: transparent; cursor: pointer}
span {width: 4px; height: 4px; border-radius: 1px; background: var(--tq-color-text-mute); opacity: .45}
button:hover span {opacity: 1}
button.on span {width: 6px; height: 6px; background: var(--tq-color-accent); opacity: 1}
</style>
