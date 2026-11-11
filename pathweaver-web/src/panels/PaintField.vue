<script setup lang="ts">
// Optional color: "none" is an explicit state, not a checkbox beside a picker.
// With `opacity` the same control edits the paint's opacity (Tweeq's alpha:
// the % beside the colour code, or the A slider in the picker).
import InputColor from '../../vendor/tweeq/src/InputColor/InputColor.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import type {RGB} from '../core/types'
import {hexAlpha, parseHexAlpha} from '../core/paint'

const props = defineProps<{value: RGB | null; label: string; fallback?: RGB; mixed?: boolean; opacity?: number}>()
const emit = defineEmits<{begin: []; change: [v: RGB | null, opacity: number]; commit: []}>()
function set(v: RGB | null) { emit('begin'); emit('change', v, props.opacity ?? 1); emit('commit') }
function input(v: string) {
  const {color, opacity} = parseHexAlpha(v)
  emit('begin')
  emit('change', color, props.opacity === undefined ? 1 : opacity)
}
</script>

<template>
  <div class="paint" :data-paint="label">
    <template v-if="props.value">
      <InputColor
        class="color" :model-value="hexAlpha(props.value, props.opacity ?? 1)" :alpha="props.opacity !== undefined" :aria-label="label"
        @focus="emit('begin')" @update:model-value="input"
        @confirm="emit('commit')" @blur="emit('commit')"
      />
      <slot />
      <InputButton narrow subtle icon="material-symbols:remove" :tooltip="`${label}をなしにする`" :aria-label="`${label}をなしにする`" @click="set(null)" />
    </template>
    <InputButton
      v-else class="none" subtle icon="material-symbols:add" :label="mixed ? '混在 — 追加' : 'なし — 追加'"
      :tooltip="`${label}を追加`" :aria-label="`${label}を追加`" @click="set(props.fallback ?? [32, 33, 36])"
    />
  </div>
</template>

<style scoped>
.paint {display: flex; gap: var(--tq-gap-related, 2px); width: 100%; min-width: 0}
.color {flex: 1; min-width: 0}
.none {flex: 1; justify-content: flex-start}
</style>
