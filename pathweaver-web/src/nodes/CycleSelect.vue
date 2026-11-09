<script setup lang="ts">
// A dropdown with ‹ › on either side: click the arrows to step through the
// options without opening the list (wraps around). The list still opens on click.
import InputDropdown from '../../vendor/tweeq/src/InputDropdown/InputDropdown.vue'
import {Icon} from '../../vendor/tweeq/src/Icon'

const props = defineProps<{modelValue: string; options: readonly string[]; labels: readonly string[]; ariaLabel?: string}>()
const emit = defineEmits<{'update:modelValue': [value: string]}>()

function step(d: number) {
  const n = props.options.length
  const i = props.options.indexOf(props.modelValue)
  emit('update:modelValue', props.options[((i < 0 ? 0 : i) + d + n) % n])
}
</script>

<template>
  <div class="cycle" :data-value="modelValue">
    <button type="button" class="arrow" :aria-label="`${ariaLabel ?? ''} 前へ`" title="前の選択肢" @pointerdown.stop @click="step(-1)"><Icon icon="material-symbols:chevron-left" /></button>
    <InputDropdown
      class="list" :model-value="modelValue" :options="[...options]" :labels="[...labels]" :aria-label="ariaLabel"
      @update:model-value="v => emit('update:modelValue', v as string)"
    />
    <button type="button" class="arrow" :aria-label="`${ariaLabel ?? ''} 次へ`" title="次の選択肢" @pointerdown.stop @click="step(1)"><Icon icon="material-symbols:chevron-right" /></button>
  </div>
</template>

<style scoped>
.cycle {display: flex; align-items: center; gap: 2px; width: 100%; min-width: 0}
.list {flex: 1; min-width: 0}
.arrow {
  display: grid; place-items: center; flex: none; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 50%;
  background: var(--tq-color-neutral, var(--tq-color-input)); color: var(--tq-color-text); cursor: pointer; font-size: 15px;
}
.arrow:hover {background: var(--tq-color-accent-hover); color: var(--tq-color-on-accent)}
</style>
