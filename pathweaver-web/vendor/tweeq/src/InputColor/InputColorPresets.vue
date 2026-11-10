<script setup lang="ts">
import {computedInject} from '@vueuse/core'

import {InputColorPresetsKey} from './useInputColor'

interface Props {
	presets?: string[]
}

const props = withDefaults(defineProps<Props>(), {
	presets: () => [],
})

const emit = defineEmits<{
	'update:modelValue': [string]
}>()

const presetsMerged = computedInject(InputColorPresetsKey, injectedPresets => {
	return [...(injectedPresets ?? []), ...props.presets]
})
</script>

<template>
	<div class="TqInputColorPresets">
		<button
			v-for="preset in presetsMerged"
			:key="preset"
			:style="{background: preset}"
			@click="emit('update:modelValue', preset)"
		/>
	</div>
</template>

<style lang="stylus" scoped>
@import './common.styl'

.TqInputColorPresets
	display flex
	gap var(--tq-gap-related)

button
	width var(--tq-input-height)
	height var(--tq-input-height)
	border-radius var(--tq-radius-input)
	// PathWeaver: a swatch the colour of the background (white) stays visible.
	box-shadow inset 0 0 0 1px var(--tq-color-border)
</style>
./useInputColor
