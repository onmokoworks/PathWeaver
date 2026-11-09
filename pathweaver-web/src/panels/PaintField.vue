<script setup lang="ts">
// Optional color: "none" is an explicit state, not a checkbox beside a picker.
import InputColor from '../../vendor/tweeq/src/InputColor/InputColor.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import type {RGB} from '../core/types'

const props = defineProps<{value: RGB | null; label: string; fallback?: RGB; mixed?: boolean}>()
const emit = defineEmits<{begin: []; change: [v: RGB | null]; commit: []}>()
const hex = (c: RGB) => '#' + c.map(v => v.toString(16).padStart(2, '0')).join('')
const rgb = (s: string): RGB => [1, 3, 5].map(i => parseInt(s.slice(i, i + 2), 16)) as RGB
function set(v: RGB | null) { emit('begin'); emit('change', v); emit('commit') }
</script>

<template>
  <div class="paint" :data-paint="label">
    <template v-if="props.value">
      <InputColor
        class="color" :model-value="hex(props.value)" :alpha="false" :aria-label="label"
        @focus="emit('begin')" @update:model-value="v => { emit('begin'); emit('change', rgb(v)) }"
        @confirm="emit('commit')" @blur="emit('commit')"
      />
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
