<script setup lang="ts">
// A docked panel's own controls: fold it to a strip (same as dragging its
// divider shut) and float it. A floating panel has both in its title bar.
import {layout, PANEL_TITLES, setFloating, setFolded, type PanelId} from '../core/layout'
import {Icon} from '../../vendor/tweeq/src/Icon'

defineProps<{id: PanelId}>()
</script>

<template>
  <span v-if="!layout[id].floating" class="pins">
    <button
      type="button" class="pin" :data-fold="id" :aria-label="`${PANEL_TITLES[id]}をたたむ`"
      title="たたむ（仕切りを端まで寄せても同じ）" @pointerdown.stop @click="setFolded(id, true)"
    >
      <Icon :icon="id === 'nodes' ? 'material-symbols:expand-more' : 'material-symbols:expand-less'" />
    </button>
    <button
      type="button" class="pin" :data-pin="id"
      :aria-label="`${PANEL_TITLES[id]}を切り離す`" title="切り離して浮かせる" @pointerdown.stop @click="setFloating(id, true)"
    >
      <Icon icon="material-symbols:open-in-new" />
    </button>
  </span>
</template>

<style scoped>
.pins {display: inline-flex; gap: 2px; flex: none}
.pin {
  display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 50%; flex: none;
  background: transparent; color: var(--tq-color-text-mute); cursor: pointer; font-size: 13px;
}
.pin:hover {background: var(--tq-color-input-hover); color: var(--tq-color-text)}
</style>
