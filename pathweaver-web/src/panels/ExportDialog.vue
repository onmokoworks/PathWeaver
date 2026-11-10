<script setup lang="ts">
// 書き出し（⌘E）: what (artboards / selection / whole drawing), as what
// (SVG / PNG / JPEG), how big and on what background. The choices are kept
// for next time; the artboards to export default to all of them.
import {computed, onBeforeUnmount, onMounted, reactive, ref, watch} from 'vue'
import ParameterGrid from '../../vendor/tweeq/src/ParameterGrid/ParameterGrid.vue'
import Parameter from '../../vendor/tweeq/src/ParameterGrid/Parameter.vue'
import InputRadio from '../../vendor/tweeq/src/InputRadio/InputRadio.vue'
import InputCheckbox from '../../vendor/tweeq/src/InputCheckbox/InputCheckbox.vue'
import InputButton from '../../vendor/tweeq/src/InputButton/InputButton.vue'
import type {Editor} from '../core/editor'
import type {Canvas} from '../canvas/controller'
import {exportFiles, frames, pixelSize, type ExportFile, type ExportFormat, type ExportOptions, type ExportTarget} from '../core/exporter'

const props = defineProps<{ed: Editor; canvas: Canvas}>()
const emit = defineEmits<{close: []; files: [ExportFile[]]}>()
const ed = props.ed

const KEY = 'pw.export'
const saved = (() => { try { return JSON.parse(localStorage.getItem(KEY) ?? '{}') } catch { return {} } })()
const boards = computed(() => ed.doc.artboards ?? [])
const hasSelection = ed.selected.length > 0
const opts = reactive({
  target: (saved.target as ExportTarget) ?? 'artboards',
  format: (saved.format as ExportFormat) ?? 'png',
  scale: Number(saved.scale) || 2,
  background: (saved.background as 'none' | 'white') ?? 'none',
})
// a target that is not available falls back to one that is
if (opts.target === 'artboards' && !boards.value.length) opts.target = hasSelection ? 'selection' : 'all'
if (opts.target === 'selection' && !hasSelection) opts.target = boards.value.length ? 'artboards' : 'all'
watch(opts, v => { try { localStorage.setItem(KEY, JSON.stringify(v)) } catch { /* preference only */ } }, {deep: true})
const picked = ref<number[]>(boards.value.map(a => a.id))

const targets = computed(() => {
  const t: ExportTarget[] = []
  if (boards.value.length) t.push('artboards')
  if (hasSelection) t.push('selection')
  t.push('all')
  return t
})
const TARGET_LABEL: Record<ExportTarget, string> = {artboards: 'アートボード', selection: '選択', all: '絵全体'}

function drawingBounds() {
  const keep = ed.selected
  ed.selected = ed.doc.strokes.filter(s => !s.guide && !s.image?.reference).map(s => s.id)
  const b = props.canvas.selectionBounds()
  ed.selected = keep
  return b
}
const options = computed<ExportOptions>(() => ({
  target: opts.target, artboards: picked.value, selection: [...ed.selected],
  bounds: opts.target === 'selection' ? props.canvas.selectionBounds() : opts.target === 'all' ? drawingBounds() : null,
  format: opts.format, scale: opts.scale,
  background: opts.background === 'white' || opts.format === 'jpeg' ? [255, 255, 255] : null,
  base: ed.fileName.replace(/\.pathweaver$/, '') || '名称未設定',
}))
const list = computed(() => frames({...ed.doc}, options.value))
const summary = computed(() => {
  if (!list.value.length) return '書き出すものがありません'
  const f = list.value[0].frame
  if (opts.format === 'svg') return list.value.length > 1 ? `${list.value.length} ファイル` : `${Math.round(f[2])}×${Math.round(f[3])}`
  const sizes = list.value.map(v => pixelSize(v.frame[2], v.frame[3], opts.scale))
  const first = `${sizes[0].w}×${sizes[0].h} px`
  return (list.value.length > 1 ? `${list.value.length} ファイル（1枚目 ${first}）` : first) + (sizes.some(s => s.clamped) ? '・大きすぎるので縮めます' : '')
})
const busy = ref(false)
async function run() {
  if (!list.value.length || busy.value) return
  busy.value = true
  try {
    emit('files', await exportFiles(ed.doc, options.value))
    emit('close')
  } catch (e) {
    ed.error = String(e instanceof Error ? e.message : e)
  } finally {
    busy.value = false
  }
}
const toggle = (id: number, on: boolean) => { picked.value = on ? [...picked.value, id] : picked.value.filter(v => v !== id) }

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && !document.querySelector('.Popover:popover-open')) { e.stopPropagation(); emit('close') }
  if (e.key === 'Enter' && !(e.target instanceof HTMLInputElement)) { e.preventDefault(); e.stopPropagation(); void run() }
}
onMounted(() => window.addEventListener('keydown', onKey, true))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <div class="backdrop" @pointerdown.self="emit('close')">
    <div class="dialog" role="dialog" aria-label="書き出し" data-testid="export-dialog">
      <header>
        <span class="title">書き出し</span>
        <InputButton subtle narrow icon="material-symbols:close" aria-label="閉じる" @click="emit('close')" />
      </header>
      <div class="body">
        <ParameterGrid>
          <Parameter label="範囲">
            <InputRadio data-field="export-target" :model-value="opts.target" :options="targets" :labels="targets.map(t => TARGET_LABEL[t])" @update:model-value="v => { opts.target = v as ExportTarget }" />
          </Parameter>
          <li v-if="opts.target === 'artboards'" class="boards">
            <InputCheckbox
              v-for="a in boards" :key="a.id" :data-export-board="a.id" :model-value="picked.includes(a.id)"
              :label="`${a.name}（${Math.round(a.w)}×${Math.round(a.h)}）`" @update:model-value="v => toggle(a.id, v)"
            />
          </li>
          <Parameter label="形式">
            <InputRadio data-field="export-format" :model-value="opts.format" :options="['svg', 'png', 'jpeg']" :labels="['SVG', 'PNG', 'JPEG']" @update:model-value="v => { opts.format = v as ExportFormat }" />
          </Parameter>
          <Parameter v-if="opts.format !== 'svg'" label="倍率" hint="1単位を何ピクセルにするか。画面用なら2倍がくっきり">
            <InputRadio data-field="export-scale" :model-value="opts.scale" :options="[1, 2, 3, 4]" :labels="['1×', '2×', '3×', '4×']" @update:model-value="v => { opts.scale = v as number }" />
          </Parameter>
          <Parameter v-if="opts.format !== 'jpeg'" label="背景">
            <InputRadio data-field="export-background" :model-value="opts.background" :options="['none', 'white']" :labels="['透明', '白']" @update:model-value="v => { opts.background = v as 'none' | 'white' }" />
          </Parameter>
        </ParameterGrid>
      </div>
      <footer>
        <span class="summary" data-testid="export-summary">{{ summary }}</span>
        <InputButton label="書き出す" data-testid="export-run" :disabled="!list.length || busy" @click="run" />
      </footer>
    </div>
  </div>
</template>

<style scoped>
.backdrop {position: fixed; inset: 0; z-index: 100; display: grid; place-items: center; background: rgba(0, 0, 0, .18)}
.dialog {
  width: min(440px, calc(100vw - 32px)); max-height: calc(100vh - 64px); display: flex; flex-direction: column;
  background: var(--tq-color-background); border: 1px solid var(--tq-color-border); border-radius: 6px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, .25);
}
header {display: flex; align-items: center; height: 36px; padding: 0 6px 0 14px; border-bottom: 1px solid var(--tq-color-border)}
.title {flex: 1; font-weight: 500}
.body {padding: 12px 14px; overflow-y: auto}
.boards {grid-column: 1 / 3; display: flex; flex-direction: column; gap: 4px; padding-left: 4px; list-style: none}
footer {display: flex; align-items: center; gap: 12px; padding: 8px 14px; border-top: 1px solid var(--tq-color-border)}
.summary {flex: 1; color: var(--tq-color-text-mute); font-variant-numeric: tabular-nums}
</style>
