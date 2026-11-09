<script setup lang="ts">
// "切り離す" for a docked panel (a floating panel has 戻す in its own bar).
import {layout, PANEL_TITLES, setFloating, type PanelId} from '../core/layout'
import {Icon} from '../../vendor/tweeq/src/Icon'

defineProps<{id: PanelId}>()
</script>

<template>
  <button
    v-if="!layout[id].floating" type="button" class="pin" :data-pin="id"
    :aria-label="`${PANEL_TITLES[id]}を切り離す`" title="切り離して浮かせる" @pointerdown.stop @click="setFloating(id, true)"
  >
    <Icon icon="material-symbols:open-in-new" />
  </button>
</template>

<style scoped>
.pin {
  display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 50%; flex: none;
  background: transparent; color: var(--tq-color-text-mute); cursor: pointer; font-size: 13px;
}
.pin:hover {background: var(--tq-color-input-hover); color: var(--tq-color-text)}
</style>
