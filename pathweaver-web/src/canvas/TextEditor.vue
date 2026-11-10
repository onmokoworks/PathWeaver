<script setup lang="ts">
// Typing on the canvas. A plain-text editable box sits exactly over the text's
// outlines (same font, size, line height and placement as the layout in
// textOutline.ts), with its own glyphs transparent: what you see is the real
// shape updating as you type, with the system caret, selection and IME.
import {computed, nextTick, onMounted, ref} from 'vue'
import type {Editor} from '../core/editor'
import type {Canvas} from './controller'
import {relayout} from '../core/textOutline'

const props = defineProps<{ed: Editor; canvas: Canvas; id: number}>()
const box = ref<HTMLDivElement>()
const s = computed(() => { void props.ed.revision; return props.ed.findStroke(props.id) })
const t = computed(() => s.value?.text)

const style = computed(() => {
  const v = props.canvas.view
  const m = t.value?.matrix ?? [1, 0, 0, 1, 0, 0]
  const z = v.zoom
  return {
    transform: `matrix(${m[0] * z}, ${m[1] * z}, ${m[2] * z}, ${m[3] * z}, ${v.panX + m[4] * z}, ${v.panY + m[5] * z})`,
    fontFamily: /^(sans-serif|serif|monospace)$/.test(t.value?.font ?? '') ? t.value!.font : JSON.stringify(t.value?.font ?? ''),
    fontSize: `${t.value?.size ?? 12}px`,
    lineHeight: String(t.value?.line_height ?? 1.2),
  }
})

let pending: Promise<void> = Promise.resolve()
/** The typed text. A trailing <br> placeholder reads as a newline in
 *  innerText but is not text (textContent leaves it out). */
function read(el: HTMLElement) {
  const v = el.innerText
  return v.endsWith('\n') && !(el.textContent ?? '').endsWith('\n') ? v.slice(0, -1) : v
}
function onInput() {
  const el = box.value, st = s.value
  if (!el || !st?.text) return
  st.text.text = read(el).replace(/\r/g, '')
  props.ed.changed()
  pending = relayout(props.ed, st)
}
async function finish() {
  await pending
  if (props.canvas.state.textEdit?.stroke === props.id) props.canvas.finishText()
}
function onKey(e: KeyboardEvent) {
  e.stopPropagation()
  if (e.isComposing) return
  if (e.key === 'Escape' || (e.key === 'Enter' && (e.metaKey || e.ctrlKey))) { e.preventDefault(); finish() }
}

onMounted(async () => {
  const el = box.value!
  el.innerText = t.value?.text ?? ''
  await nextTick()
  el.focus()
  // Caret at the end (a new text is empty anyway).
  const r = document.createRange()
  r.selectNodeContents(el)
  r.collapse(false)
  const sel = window.getSelection()
  sel?.removeAllRanges()
  sel?.addRange(r)
})
</script>

<template>
  <div
    ref="box" class="text-editor" data-testid="text-editor" contenteditable="plaintext-only" spellcheck="false"
    role="textbox" aria-multiline="true" aria-label="テキスト" :style="style"
    @input="onInput" @keydown="onKey" @blur="finish" @pointerdown.stop @wheel.stop
  />
</template>

<style scoped>
.text-editor {
  position: absolute; left: 0; top: 0; transform-origin: 0 0;
  display: inline-block; min-width: .3em; min-height: 1lh; padding: 0; margin: 0;
  white-space: pre; outline: 1px dashed var(--tq-color-accent); outline-offset: 2px;
  color: transparent; caret-color: var(--tq-color-accent); background: transparent;
  font-kerning: normal; cursor: text; z-index: 5;
}
.text-editor::selection {background: color-mix(in srgb, var(--tq-color-accent) 30%, transparent)}
</style>
