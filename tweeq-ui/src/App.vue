<script setup lang="ts">
import {iconExists} from '@iconify/vue'
import {computed, nextTick, onMounted, ref, watch} from 'vue'
import InputNumber from '../vendor/tweeq/src/InputNumber/InputNumber.vue'
import InputColor from '../vendor/tweeq/src/InputColor/InputColor.vue'
import InputString from '../vendor/tweeq/src/InputString/InputString.vue'
import InputCheckbox from '../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import InputDropdown from '../vendor/tweeq/src/InputDropdown/InputDropdown.vue'
import InputButton from '../vendor/tweeq/src/InputButton/InputButton.vue'
import ParameterGrid from '../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import TooltipRoot from '../vendor/tweeq/src/Tooltip/TooltipRoot.vue'
import {useThemeStore} from '../vendor/tweeq/src/stores/theme'
import {useAppConfigStore} from '../vendor/tweeq/src/stores/appConfig'
import {useInputColor} from '../vendor/tweeq/src/InputColor/useInputColor'
import {changeFromControl, numericStep} from './controlBridge'
import {send, Transactions, type Snapshot, type Field, type Value} from './protocol'

useAppConfigStore().appId = 'pathweaver-tweeq'
const theme = useThemeStore()
theme.accentColor = '#567DD0'
useInputColor(['#202124', '#ffffff', '#e66b5b', '#e8b655', '#70a774', '#5c88c5', '#9b7ac3'])
const state = ref<Snapshot>({revision: '', theme: 'light', title: '', sections: []})
const generation = ref(0)
const fieldGenerations = ref<Record<string, number>>({})
let focusedNumeric: string | null = null
const tx = new Transactions(send)
const fields = computed(() => state.value.sections.flatMap(section => section.fields))
window.pathweaverUpdate = snapshot => {
  if (snapshot.revision !== state.value.revision) {
    generation.value++
    focusedNumeric = null
  }
  tx.receive(snapshot)
  for (const field of snapshot.sections.flatMap(section => section.fields)) {
    const identity = field.id === 'path.rotation' ? 0 : field.id === 'path.scale' ? 1 : undefined
    if (identity !== undefined && field.value === identity && tx.active?.id !== field.id) {
      const previous = fields.value.find(old => old.id === field.id)
      if (previous && previous.value !== identity) {
        // These controls are relative commands. The native acknowledgement
        // resets their model to identity; reset the focused display as well.
        fieldGenerations.value[field.id] = (fieldGenerations.value[field.id] ?? 0) + 1
        if (focusedNumeric === field.id) focusedNumeric = null
      }
    }
  }
  // Snapshot echoes must not overwrite an in-progress text expression or scrub.
  const active = tx.active
  if (active) {
    const next = snapshot.sections.flatMap(section => section.fields).find(field => field.id === active.id)
    if (next) next.value = active.value
  }
  state.value = snapshot
}
window.pathweaverFocusField = id => nextTick(() => {
  const field = document.querySelector(`[data-field="${CSS.escape(id)}"]`)
  const input = field?.querySelector('input')
  if (input instanceof HTMLInputElement) { input.focus(); input.select() }
})
window.pathweaverCommand = (command, text) => {
  const input = document.activeElement
  const editing = input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement || (input instanceof HTMLElement && input.isContentEditable)
  if (command === 'insertText' && editing && text !== undefined) {
    document.execCommand('insertText', false, text)
  } else if (command === 'blur') {
    if (input instanceof HTMLElement) input.blur()
    tx.commit()
  } else if (editing && ['undo', 'redo', 'selectAll', 'cut', 'copy', 'paste'].includes(command)) {
    if (command === 'selectAll' && (input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement)) input.select()
    else document.execCommand(command)
  } else if (['undo', 'redo', 'selectAll', 'save', 'saveAs', 'open', 'new', 'close', 'export'].includes(command)) {
    if (input instanceof HTMLElement) input.blur()
    tx.commit()
    send({type: 'action', id: command, revision: state.value.revision})
  }
}
watch(() => state.value.theme, mode => { theme.colorMode = mode }, {immediate: true})
function change(field: Field, value: Value) { changeFromControl(tx, state.value.revision, field, value, focusedNumeric) }
function discrete(field: Field, value: Value) { change(field, value); tx.commit() }
function begin(field: Field) {
  if (field.disabled) return
  focusedNumeric = field.type === 'number' ? field.id : null
  tx.begin(state.value.revision, field)
}
function blur(field: Field) {
  if (focusedNumeric === field.id) focusedNumeric = null
  queueMicrotask(() => {
  if (!(document.activeElement instanceof HTMLInputElement)) tx.commit()
}) }
function cancel(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  const reverted = tx.cancel()
  if (!reverted) return
  event.preventDefault()
  event.stopImmediatePropagation()
  // Remount clears Tweeq's locally held text expression and temporary color state.
  const field = fields.value.find(field => field.id === reverted.id)
  if (field) field.value = reverted.value
  focusedNumeric = null
  generation.value++
}
function act(id: string) { tx.commit(); send({type: 'action', revision: state.value.revision, id}) }
function actionIcon(id: string, icon?: string) {
  const aliases: Record<string, string> = {
    copy: 'mdi:content-copy', trash: 'mdi:delete-outline', nodes: 'mdi:graph-outline',
    anchor: 'mdi:vector-point', split: 'mdi:content-cut', join: 'mdi:vector-polyline',
    merge: 'mdi:arrow-collapse-all', line: 'mdi:vector-line', union: 'mdi:vector-union',
  }
  if (icon && aliases[icon]) return aliases[icon]
  if (icon && iconExists(icon)) return icon
  if (id.includes('delete')) return 'mdi:delete-outline'
  if (id.includes('duplicate')) return 'mdi:content-copy'
  if (id === 'edit_nodes' || id === 'path_nodes') return 'mdi:graph-outline'
  return 'mdi:arrow-right'
}
onMounted(() => {
  window.addEventListener('keydown', cancel, true)
  window.addEventListener('pointerup', () => queueMicrotask(() => {
    // Color channel pads do not forward confirm in upstream Tweeq. End their
    // gesture here while leaving a focused text editor's transaction intact.
    if (!(document.activeElement instanceof HTMLInputElement)) tx.commit()
  }))
  window.addEventListener('blur', () => tx.commit())
  document.addEventListener('focusin', () => send({type: 'focus', value: true}))
  document.addEventListener('focusout', () => nextTick(() => {
    if (!document.hasFocus()) send({type: 'focus', value: false})
  }))
  send({type: 'ready'})
})
</script>

<template>
  <main class="inspector TqViewport" :key="generation">
    <header v-if="state.title || state.subtitle">
      <h1 :title="state.title">{{ state.title }}</h1>
      <p v-if="state.subtitle">{{ state.subtitle }}</p>
    </header>
    <section v-for="(section, index) in state.sections" :key="index">
      <h2 v-if="section.title">{{ section.title }}</h2>
      <ParameterGrid>
        <Parameter v-for="field in section.fields" :key="`${field.id}:${fieldGenerations[field.id] ?? 0}`" :data-field="field.id" :label="field.label" :hint="field.hint || field.label">
          <span v-if="field.type === 'number' && field.value === null" class="missing" aria-label="値なし">—</span>
          <InputNumber v-else-if="field.type === 'number'" :model-value="Number(field.value)" :disabled="field.disabled" :min="field.min" :max="field.max" :step="numericStep(field)" :suffix="field.suffix" :bar="false" :aria-label="field.label" @update:model-value="change(field, $event)" @focus="begin(field)" @confirm="tx.commit()" @blur="blur(field)" />
          <InputString v-else-if="field.type === 'text'" :model-value="String(field.value ?? '')" :disabled="field.disabled" :aria-label="field.label" @update:model-value="change(field, $event)" @focus="begin(field)" @confirm="tx.commit()" @blur="blur(field)" />
          <div v-else-if="field.type === 'color'" class="paint-field">
            <InputCheckbox v-if="field.nullable !== false" :model-value="field.value !== null" :disabled="field.disabled" :aria-label="`${field.label}を有効にする`" :title="`${field.label}を有効にする`" @update:model-value="discrete(field, $event ? '#202124' : null)" />
            <div class="paint-value" :inert="field.disabled || field.value === null"><InputColor :model-value="String(field.value ?? '#202124')" :alpha="false" :aria-label="field.label" @update:model-value="change(field, $event)" @focus="begin(field)" @confirm="tx.commit()" @blur="blur(field)" /></div>
          </div>
          <InputCheckbox v-else-if="field.type === 'boolean'" :model-value="Boolean(field.value)" :disabled="field.disabled" :aria-label="field.label" @update:model-value="discrete(field, $event)" />
          <InputDropdown v-else-if="field.type === 'select'" :model-value="String(field.value ?? '')" :options="field.options ?? []" :labels="field.labels" :disabled="field.disabled" :aria-label="field.label" @update:model-value="discrete(field, $event)" />
        </Parameter>
      </ParameterGrid>
    </section>
    <div v-if="state.actions?.length" class="actions">
      <InputButton v-for="action in state.actions" :key="action.id" :icon="actionIcon(action.id, action.icon)" :tooltip="action.label" :aria-label="action.label" :disabled="action.disabled" @click="act(action.id)" />
    </div>
    <p v-if="state.error" role="alert" class="error">{{ state.error }}</p>
  </main>
  <TooltipRoot />
</template>

<style lang="stylus">
@import '../vendor/tweeq/src/common.styl'
reset-viewport('.TqViewport')
scroll-fade-globals()
</style>
<style>
:root {font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans', sans-serif; font-size: 12px; color-scheme: light dark}
* {box-sizing: border-box}
html, body, #app {margin: 0; min-height: 100%; width: 100%}
body {background: var(--tq-color-background); color: var(--tq-color-text); --tq-popup-width: min(220px, calc(100vw - 36px)) !important; --tq-pane-margin: 8px !important}
.inspector {padding: 12px; min-height: 100vh; overflow-x: hidden; font-family: inherit}
header {margin-bottom: 16px}
h1 {font-size: 13px; font-weight: 600; margin: 0; line-height: 1.5; overflow-wrap: anywhere}
header p {margin: 4px 0 0; color: var(--tq-color-text-mute); font-size: 11px; line-height: 1.5}
section + section {margin-top: 20px}
h2 {font-size: 11px; font-weight: 600; color: var(--tq-color-text-mute); margin: 0 0 10px}
/* Native child views cannot paint outside their frame. Keep the real Tweeq
   color popover inside this inspector, with its full contents scrollable. */
.Popover:has(.TqInputColorPicker) {
  top: 8px !important; bottom: auto !important;
  left: 8px !important; right: 8px !important;
  width: fit-content; max-width: calc(100vw - 16px);
  max-height: calc(100dvh - 16px); margin: 0 auto !important;
  transform: none !important; translate: none !important;
  overflow: auto;
}
.Popover .floating {max-width: calc(100vw - 32px)}
.paint-field {display: flex; align-items: center; gap: 6px; min-width: 0}
.paint-value {flex: 1; min-width: 0}
.paint-value[inert] {opacity: .45}
.actions {display: flex; flex-wrap: wrap; gap: 6px; margin-top: 18px}
.error {color: var(--tq-color-error, #c3453b); line-height: 1.5; margin-top: 12px}
.TqParameter > .label {max-width: 96px; overflow: hidden; text-overflow: ellipsis; font-size: 11px}
</style>
