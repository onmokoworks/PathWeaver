<script setup lang="ts">
import {computed, onBeforeUnmount, onMounted, ref, watch} from 'vue'
import {TooltipRoot} from '../vendor/tweeq/src/Tooltip'
import {useInputColor} from '../vendor/tweeq/src/InputColor/useInputColor'
import InputButton from '../vendor/tweeq/src/InputButton/InputButton.vue'
import {useThemeStore} from '../vendor/tweeq/src/stores/theme'
import {useAppConfigStore} from '../vendor/tweeq/src/stores/appConfig'
import {createEditor, type Tool} from './core/editor'
import {createCanvas} from './canvas/controller'
import ArtCanvas from './canvas/ArtCanvas.vue'
import ToolPalette from './canvas/ToolPalette.vue'
import NodeEditor from './nodes/NodeEditor.vue'
import PathList from './panels/PathList.vue'
import Inspector from './panels/Inspector.vue'
import GridPanel from './panels/GridPanel.vue'
import SettingsDialog from './panels/SettingsDialog.vue'
import ExportDialog from './panels/ExportDialog.vue'
import FloatingPanel from './panels/FloatingPanel.vue'
import PanelPin from './panels/PanelPin.vue'
import AreaSplit from './panels/AreaSplit.vue'
import {isFolded, layout, setFolded, toggleFolded, type PanelId} from './core/layout'
import {settings} from './core/settings'
import {createHost, isNative} from './host'
import {call} from './core/wasm'
import type {Document, Vec2} from './core/types'
import {placeData, placeFiles, type PlaceTarget} from './core/place'
import {SHAPES, shapeInfo, type ShapeKind} from './core/shapes'
import {DEFAULT_FONT, outlineText, relayout} from './core/textOutline'
import {exportAssets} from './core/knockout'

useAppConfigStore().appId = 'pathweaver'
useInputColor(['#202124', '#ffffff', '#e66b5b', '#e8b655', '#70a774', '#5c88c5', '#9b7ac3'])
const theme = useThemeStore()
theme.accentColor = '#3d74e0'
// Theme comes from 設定 (システム follows macOS).
const darkQuery = window.matchMedia?.('(prefers-color-scheme: dark)')
function applyTheme() {
  theme.colorMode = settings.theme === 'system' ? (darkQuery?.matches ? 'dark' : 'light') : settings.theme
}
watch(() => settings.theme, applyTheme, {immediate: true})
darkQuery?.addEventListener?.('change', applyTheme)
const settingsOpen = ref(false)
const exportOpen = ref(false)

const ed = createEditor()
const canvas = createCanvas(ed)
const nodeEditor = ref<InstanceType<typeof NodeEditor>>()

const toolList: {id: Tool; icon: string; label: string; key: string}[] = [
  {id: 'select', icon: 'pw:select', label: '選択', key: 'V'},
  {id: 'direct', icon: 'pw:direct', label: 'ダイレクト選択（アンカー）', key: 'A'},
  {id: 'pen', icon: 'pw:pen', label: 'ペン', key: 'P'},
  {id: 'convert', icon: 'pw:anchor', label: 'アンカーポイント（角⇄スムーズ）', key: '⇧C'},
  {id: 'text', icon: 'material-symbols:title', label: '文字', key: 'T'},
  {id: 'artboard', icon: 'mdi:artboard', label: 'アートボード', key: '⇧O'},
]
// The shape button shows the shape it will draw (Illustrator's tool group).
const tools = computed(() => {
  const sh = shapeInfo(canvas.state.shapeKind)
  const list = [...toolList]
  list.splice(3, 0, {id: 'shape', icon: sh.icon, label: sh.label, key: sh.key ?? 'M・L・\\'})
  return list
})
const toolHint: Record<Tool, string> = {
  select: 'クリックで選択・ドラッグで移動。枠の角で拡大縮小、角の外側で基準点（十字）まわりに回転（Shiftで比率固定／角度刻み、Optionで複製・基準点から拡大縮小）',
  direct: 'アンカー・ハンドルをドラッグ。線分をOption＋クリック／ダブルクリックでアンカー追加。太さはオレンジの端をドラッグ（Optionで片側）、⌘＋芯クリックで幅点追加、⌘＋端点ドラッグで切り口回転',
  pen: 'クリックで角、ドラッグで曲線。線分上でクリックでアンカー追加。Shift＋パスをクリックでその上から描き始め（次の点は法線方向）。太さはオレンジの端をドラッグ（Optionで片側）。Enter/Escで終了',
  convert: 'アンカーをクリックで角に、ドラッグでスムーズに。ハンドルをドラッグで片側だけ動かす',
  shape: 'ドラッグで描く（Shiftで縦横同じ・角度刻み、Optionで中心から）。多角形・スターはドラッグ中に↑↓で辺の数。クリックで標準の大きさ。作った直後は左下で寸法や数を変えられる',
  artboard: '空いた所をドラッグでアートボードを作る。アートボードをドラッグで移動、枠の四角で大きさ。Deleteで削除。名前と寸法は右のパネル',
  text: 'クリックした所に文字を打つ。文字をクリックで打ち直し。Esc／ほかをクリックで確定。フォントと大きさは右のパネル',
}

const typing = (t: EventTarget | null) =>
  t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && t.isContentEditable)
/** App commands confirm a field being edited first (its blur commits it). */
function settle() { if (typing(document.activeElement)) (document.activeElement as HTMLElement).blur() }
const commands: Record<string, () => void> = {
  // Menu commands act on a focused text field first, like any Mac app.
  undo: () => (typing(document.activeElement) ? document.execCommand('undo') : ed.undo()),
  redo: () => (typing(document.activeElement) ? document.execCommand('redo') : ed.redo()),
  delete: () => { if (!typing(document.activeElement)) ed.deleteSelection() },
  toggleNodes: () => toggleFolded('nodes'),
  makeGuides: () => ed.makeGuides(true),
  openNodes: () => openNodes(),
  releaseGuides: () => ed.makeGuides(false),
  copy: () => { const t = ed.copySelection(); if (t) navigator.clipboard?.writeText(t).catch(() => {}) },
  cut: () => { const t = ed.cutSelection(); if (t) navigator.clipboard?.writeText(t).catch(() => {}) },
  paste: () => { ed.paste() },
  duplicate: () => ed.duplicateSelection(),
  selectAll: () => { if (typing(document.activeElement)) { (document.activeElement as HTMLInputElement).select?.(); return } ed.selected = ed.doc.strokes.filter(s => ed.shown(s)).map(s => s.id); ed.anchors = [] },
  fit: () => canvas.fit(),
  zoomIn: () => canvas.zoomAt({x: canvas.view.width / 2, y: canvas.view.height / 2}, 1.25),
  zoomOut: () => canvas.zoomAt({x: canvas.view.width / 2, y: canvas.view.height / 2}, 0.8),
  actualSize: () => canvas.zoomAt({x: canvas.view.width / 2, y: canvas.view.height / 2}, 1 / canvas.view.zoom),
  toggleTheme: () => { settings.theme = theme.colorMode === 'dark' ? 'light' : 'dark' },
  settings: () => { settingsOpen.value = true },
  new: () => { if (!ed.dirty || confirm('保存されていない変更を破棄しますか？')) ed.load(JSON.parse(JSON.stringify(blank)), null) },
  example: () => { ed.load(exampleDoc(), null, 'サンプル'); requestAnimationFrame(() => canvas.fit()) },
  open: () => { settle(); host.open() },
  save: () => { settle(); host.save(false) },
  saveAs: () => { settle(); host.save(true) },
  export: () => { settle(); exportOpen.value = true },
  group: () => ed.groupSelection(),
  ungroup: () => { const f = ed.active?.folder; if (f != null) ed.ungroup(f) },
  snapGrid: () => ed.edit(d => { d.snap_grid = !d.snap_grid }),
  snapGuides: () => ed.edit(d => { d.snap_guides = !d.snap_guides }),
  snapTangent: () => ed.edit(d => { d.snap_tangent = !d.snap_tangent }),
  snapNormal: () => ed.edit(d => { d.snap_normal = !d.snap_normal }),
  toolSelect: () => canvas.setTool('select'),
  toolDirect: () => canvas.setTool('direct'),
  toolPen: () => canvas.setTool('pen'),
  toolConvert: () => canvas.setTool('convert'),
  toolText: () => canvas.setTool('text'),
  toolShape: () => canvas.setTool('shape'),
  toolArtboard: () => canvas.setTool('artboard'),
  place: () => { settle(); host.place((name, kind, data) => placeData(placeTarget(), name, kind, data), files => placeFiles(placeTarget(), files)) },
  outline: () => createOutlines(),
  makeBlend: () => ed.makeBlend(),
  releaseBlend: () => { const b = ed.selected.map(id => ed.blendOf(id)).find(Boolean); if (b) ed.releaseBlend(b.id) },
  expandBlend: () => { const b = ed.selected.map(id => ed.blendOf(id)).find(Boolean); if (b) ed.expandBlend(b.id) },
}
/** Where placed material goes: the given world point, else the middle of the view. */
function placeTarget(at?: Vec2): PlaceTarget {
  // Placed material arrives selected, ready to move (Illustrator's Place).
  canvas.setTool('select')
  const v = canvas.view
  return {ed, at: at ?? canvas.toWorld({x: v.width / 2, y: v.height / 2}), viewSize: {w: v.width / v.zoom, h: v.height / v.zoom}}
}
/** Type → Create Outlines: the selected text becomes plain shapes. */
function createOutlines() {
  const texts = ed.selected.map(id => ed.findStroke(id)).filter(s => s?.text)
  if (!texts.length) { ed.status = 'アウトライン化する文字を選んでください'; return }
  ed.edit(() => { for (const s of texts) { s!.text = null; s!.name = `${s!.name}（アウトライン）` } })
  ed.status = `${texts.length}個の文字をアウトライン化しました`
}
function onDrop(e: DragEvent) {
  const files = [...(e.dataTransfer?.files ?? [])]
  if (!files.length) return
  e.preventDefault()
  const el = document.querySelector('[data-testid="canvas"]')!.getBoundingClientRect()
  placeFiles(placeTarget(canvas.toWorld({x: e.clientX - el.left, y: e.clientY - el.top})), files)
}
const blank = JSON.parse(JSON.stringify(ed.doc))
const exampleDoc = () => call<Document>('example')
const host = createHost(ed, commands)

// ---- keyboard routing: text inputs keep their keys; the node editor gets
// keys while the pointer is over it; everything else goes to the canvas. ----
function onKeyDown(e: KeyboardEvent) {
  if (e.isComposing) return
  // a dialog is open: its own keys only (no tool switching underneath)
  if (exportOpen.value || settingsOpen.value) return
  if (typing(e.target)) {
    // In a field: text keys stay with it, but file commands still work
    // (the field is confirmed first, like any Mac app).
    const mod = e.metaKey || e.ctrlKey
    const k = e.key.toLowerCase()
    if (mod && !isNative() && (k === 's' || k === 'o' || k === 'e')) {
      e.preventDefault()
      ;(k === 's' ? (e.shiftKey ? commands.saveAs : commands.save) : k === 'o' ? commands.open : commands.export)()
    }
    return
  }
  // A Tweeq drag in progress (colour swatch, number, 2D pad, dial) owns the
  // keys: H/S/V/R/G/B/A pick the channel, Shift snaps. None of them may also
  // switch tools or nudge paths.
  if (document.pointerLockElement || document.querySelector('.TqTweakOverlay')) return
  // A Tweeq popover (color picker, dropdown) owns the keyboard while open.
  // (A tooltip is a popover too, but it never takes the keyboard.)
  const owner = [...document.querySelectorAll('.Popover:popover-open, [popover]:popover-open')].some(p => !p.querySelector('.TqTooltipContent'))
  if (owner) return
  if (document.activeElement instanceof HTMLElement && document.activeElement.closest('.Popover, [popover]')) return
  const mod = e.metaKey || e.ctrlKey
  const k = e.key.toLowerCase()
  if (e.key === 'Meta') canvas.state.meta = true
  if (e.key === 'Alt') canvas.state.alt = true
  // Space belongs to the canvas (pan, Ctrl+Space): a button clicked earlier
  // must not keep focus and get pressed by it.
  if (k === ' ' && document.activeElement instanceof HTMLButtonElement) document.activeElement.blur()
  if (e.ctrlKey && k === ' ') { toggleMaximize(); e.preventDefault(); return }
  if (k === ' ') { canvas.state.space = true; nodeEditor.value?.setSpace(true); e.preventDefault(); return }
  if (mod && k === 'z') { e.shiftKey ? ed.redo() : ed.undo(); e.preventDefault(); return }
  if (mod && k === 's') { (e.shiftKey ? commands.saveAs : commands.save)(); e.preventDefault(); return }
  if (mod && e.altKey && e.code === 'KeyB') { (e.shiftKey ? commands.releaseBlend : commands.makeBlend)(); e.preventDefault(); return }
  if (mod && e.shiftKey && k === 'p') { commands.place(); e.preventDefault(); return }
  if (mod && e.shiftKey && k === 'o') { commands.outline(); e.preventDefault(); return }
  if (mod && k === 'o') { commands.open(); e.preventDefault(); return }
  if (mod && k === 'e') { commands.export(); e.preventDefault(); return }
  if (mod && e.key === ',') { settingsOpen.value = true; e.preventDefault(); return }
  if (mod && k === 'g') { (e.shiftKey ? commands.ungroup : commands.group)(); e.preventDefault(); return }
  if (mod && e.code === 'Digit5') { ed.makeGuides(!e.altKey); e.preventDefault(); return }
  // Paths on the clipboard (text fields keep their own copy/paste above).
  // ⌘ only: Ctrl+X belongs to the node editor (dissolve).
  if (e.metaKey && !e.shiftKey && (k === 'c' || k === 'x')) {
    const text = k === 'c' ? ed.copySelection() : ed.cutSelection()
    if (text) { navigator.clipboard?.writeText(text).catch(() => {}); e.preventDefault() }
    return
  }
  // ⌘V: the browser's paste event comes first (it can carry an image copied
  // elsewhere); only when none arrives are PathWeaver's own paths pasted.
  if (e.metaKey && !e.shiftKey && k === 'v') {
    pasteSeen = false
    setTimeout(() => { if (!pasteSeen) ed.paste() }, 0)
    return
  }
  if (nodeEditor.value?.isPointerInside() && nodeEditor.value.key(e)) { e.preventDefault(); return }
  if (canvas.key(e)) e.preventDefault()
}
function onClipboard(e: ClipboardEvent) {
  if (typing(document.activeElement)) return
  const text = e.type === 'cut' ? ed.cutSelection() : ed.copySelection()
  if (!text) return
  e.clipboardData?.setData('text/plain', text)
  e.preventDefault()
}
let pasteSeen = false
function onPaste(e: ClipboardEvent) {
  if (typing(document.activeElement)) return
  pasteSeen = true
  // An image (or SVG file) on the clipboard is placed in the view.
  const files = [...(e.clipboardData?.files ?? [])].filter(f => f.type.startsWith('image/'))
  const svgText = e.clipboardData?.getData('image/svg+xml') || (/^\s*(<\?xml[^>]*>\s*)?<svg[\s>]/.test(e.clipboardData?.getData('text/plain') ?? '') ? e.clipboardData!.getData('text/plain') : '')
  if (files.length || svgText) {
    e.preventDefault()
    if (svgText) placeData(placeTarget(), 'ペーストしたSVG', 'svg', svgText)
    else placeFiles(placeTarget(), files)
    return
  }
  if (ed.paste(e.clipboardData?.getData('text/plain') || undefined) || ed.paste()) e.preventDefault()
}
function onKeyUp(e: KeyboardEvent) {
  if (e.key === 'Meta') canvas.state.meta = false
  if (e.key === 'Alt') canvas.state.alt = false
  if (e.key === ' ') { canvas.state.space = false; nodeEditor.value?.setSpace(false) }
}
onMounted(() => {
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('keyup', onKeyUp)
  window.addEventListener('pointermove', trackArea, true)
  // Edit menu (Mac app) and other copy/paste routes arrive as DOM events.
  document.addEventListener('copy', onClipboard)
  document.addEventListener('cut', onClipboard)
  document.addEventListener('paste', onPaste)
  window.addEventListener('blur', () => { canvas.cancelDrag(); canvas.state.space = false; canvas.state.meta = false; canvas.state.alt = false; if (!typing(document.activeElement) && !canvas.state.textEdit) ed.commit() })
  // Tweeq's color pad/slider gestures end without a confirm event. Releasing
  // the pointer outside a text field ends the gesture: one drag = one undo.
  window.addEventListener('pointerup', () => queueMicrotask(() => {
    if (!typing(document.activeElement) && !canvas.state.drag && !canvas.state.textEdit) ed.commit()
  }), true)
  requestAnimationFrame(() => canvas.fit())
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeyDown)
  window.removeEventListener('keyup', onKeyUp)
})

const message = computed(() => ed.error || ed.status || toolHint[ed.tool])
watch(() => ed.status, v => { if (v) setTimeout(() => { if (ed.status === v) ed.status = '' }, 4000) })
function openNodes() { setFolded('nodes', false) }

// ---- top bar = title bar (drag to move the window, double-click = full screen) ----
let dragRegion = false
function setDragRegion(v: boolean) {
  if (v === dragRegion) return
  dragRegion = v
  window.webkit?.messageHandlers?.pathweaver?.postMessage(JSON.stringify({type: 'dragRegion', value: v}))
}
const emptyTopbar = (t: EventTarget | null) => !(t instanceof Element && t.closest('button, input, .actions, .TqInputButton'))
function onTopbarMove(e: PointerEvent) { setDragRegion(emptyTopbar(e.target)) }
function onTopbarDouble(e: MouseEvent) {
  // In the Mac app the host handles this natively; in a browser use the Fullscreen API.
  if (isNative() || !emptyTopbar(e.target)) return
  if (document.fullscreenElement) document.exitFullscreen()
  else document.documentElement.requestFullscreen?.()
}

// ---- panels: docked in place, or floating over the workspace ----
const layersDocked = computed(() => !layout.layers.floating)
const propsDocked = computed(() => !layout.props.floating)
const nodesDocked = computed(() => !layout.nodes.floating)
const nodesFolded = computed(() => isFolded('nodes'))

// ---- Ctrl+Space: the area under the pointer fills the workspace (Blender) ----
type AreaId = 'canvas' | PanelId
const maximized = ref<AreaId | null>(null)
let pointerAt = {x: -1, y: -1}
function trackArea(e: PointerEvent) { pointerAt = {x: e.clientX, y: e.clientY} }
function toggleMaximize() {
  if (maximized.value) { maximized.value = null; return }
  // the area under the pointer right now (not where an event last landed)
  const el = document.elementFromPoint(pointerAt.x, pointerAt.y)?.closest('[data-area]') as HTMLElement | null
  const a = (el?.dataset.area as AreaId | undefined) ?? 'canvas'
  if (a !== 'canvas' && layout[a].floating) return
  maximized.value = a
  ed.status = '最大化しました（Ctrl＋Space で戻す）'
}
const maxRight = computed(() => (!maximized.value ? null : maximized.value === 'canvas' || maximized.value === 'nodes' ? 'first' : 'second'))
const maxNodes = computed(() => (maximized.value === 'canvas' ? 'first' : maximized.value === 'nodes' ? 'second' : null))
const maxSide = computed(() => (maximized.value === 'layers' ? 'first' : maximized.value === 'props' ? 'second' : null))
const workspace = ref<HTMLElement>()
const wsBounds = ref({width: 800, height: 600})
let wsObserver: ResizeObserver | null = null
onMounted(() => {
  wsObserver = new ResizeObserver(() => { if (workspace.value) wsBounds.value = {width: workspace.value.clientWidth, height: workspace.value.clientHeight} })
  wsObserver.observe(workspace.value!)
})
onBeforeUnmount(() => wsObserver?.disconnect())

// ---- right panel tabs (plain text tabs) ----
const sideTabs = [{id: 'props', label: 'プロパティ'}, {id: 'grid', label: 'グリッド・ガイド'}] as const
const sideTab = ref<string>((() => { try { return localStorage.getItem('pw.sideTab') ?? 'props' } catch { return 'props' } })())
function setSideTab(id: string) { sideTab.value = id; try { localStorage.setItem('pw.sideTab', id) } catch { /* preference only */ } }

// Test hook: lets end-to-end tests read the document without poking internals.
;(window as unknown as {__pw: unknown}).__pw = {
  ed, canvas, call, exportAssets,
  // test hook: lay out the active text again and finish its gesture
  textProbe: async (t: string) => {
    try { (window as unknown as {__pwTextProbe: unknown}).__pwTextProbe = (await outlineText({text: t, font: DEFAULT_FONT, size: 48, line_height: 1.2})).length } catch (e) { (window as unknown as {__pwTextProbe: unknown}).__pwTextProbe = String(e) }
  },
  relayoutActive: async () => { const s = ed.active; if (s) await relayout(ed, s); ed.commit() },
}
</script>

<template>
    <div class="app TqViewport" :class="{native: isNative()}">
      <header class="topbar" data-testid="topbar" @pointermove="onTopbarMove" @pointerleave="setDragRegion(false)" @dblclick="onTopbarDouble">
        <div class="doc" :title="ed.filePath ?? ed.fileName">
          <span class="name">{{ ed.fileName }}</span>
          <span v-if="ed.dirty" class="dirty" title="未保存の変更があります">●</span>
        </div>
        <div class="actions">
          <template v-if="!isNative()">
            <InputButton subtle narrow icon="material-symbols:folder-open-outline" tooltip="開く ⌘O" @click="host.open()" />
            <InputButton subtle narrow icon="material-symbols:save-outline" tooltip="保存 ⌘S" @click="host.save()" />
            <InputButton subtle narrow icon="material-symbols:ios-share" tooltip="書き出し（SVG・PNG・JPEG）⌘E" data-testid="export-open" @click="commands.export()" />
            <span class="sep" />
          </template>
          <InputButton subtle narrow icon="material-symbols:undo" tooltip="取り消す ⌘Z" :disabled="!ed.history.undo" @click="ed.undo()" />
          <InputButton subtle narrow icon="material-symbols:redo" tooltip="やり直す ⇧⌘Z" :disabled="!ed.history.redo" @click="ed.redo()" />
          <span class="sep" />
          <InputButton subtle narrow :icon="nodesFolded ? 'material-symbols:bottom-panel-open-outline' : 'material-symbols:bottom-panel-close-outline'" :tooltip="nodesFolded ? 'ノードを開く' : 'ノードをたたむ'" data-testid="toggle-nodes" @click="toggleFolded('nodes')" />
          <InputButton v-if="!isNative()" subtle narrow icon="material-symbols:settings-outline" tooltip="設定 ⌘," aria-label="設定" @click="settingsOpen = true" />
        </div>
      </header>

      <div class="body">

        <div ref="workspace" class="workspace">
          <AreaSplit name="pw-right" direction="horizontal" fixed="second" :titles="['キャンバス', 'レイヤー・プロパティ']" :min="200" :max="maxRight" :hide="!layersDocked && !propsDocked ? 'second' : null">
            <template #first>
              <AreaSplit name="pw-nodes" direction="vertical" fixed="second" :titles="['キャンバス', 'ノード']" :max="maxNodes" :hide="!nodesDocked ? 'second' : null">
                <template #first><div class="area" data-area="canvas" @dragover.prevent @drop="onDrop"><ArtCanvas :ed="ed" :canvas="canvas" v-slot="{bounds}" @command="n => commands[n]?.()"><ToolPalette :tools="tools" :active="ed.tool" :bounds="bounds" :shapes="SHAPES" :shape-kind="canvas.state.shapeKind" @select="t => canvas.setTool(t)" @shape="k => canvas.setShapeKind(k as ShapeKind)" /></ArtCanvas></div></template>
                <template #second><div class="area" data-area="nodes"><NodeEditor v-if="nodesDocked" ref="nodeEditor" :ed="ed" /></div></template>
              </AreaSplit>
            </template>
            <template #second>
              <AreaSplit name="pw-side" direction="vertical" fixed="first" :titles="['レイヤー', 'プロパティ']" :min="80" :max="maxSide" :hide="!layersDocked ? 'first' : !propsDocked ? 'second' : null">
                <template #first><div class="area" data-area="layers"><PathList v-if="layersDocked" :ed="ed" /></div></template>
                <template #second>
                  <div v-if="propsDocked" class="side-tabs area" data-area="props">
                    <div class="tab-bar" role="tablist">
                      <button v-for="t in sideTabs" :key="t.id" type="button" role="tab" class="tab" :class="{active: sideTab === t.id}" :aria-selected="sideTab === t.id" @click="setSideTab(t.id)">{{ t.label }}</button>
                      <span class="tab-spacer" />
                      <PanelPin id="props" />
                    </div>
                    <div class="tab-panel">
                      <Inspector v-if="sideTab === 'props'" :ed="ed" :canvas="canvas" @open-nodes="openNodes" />
                      <GridPanel v-else :ed="ed" :canvas="canvas" />
                    </div>
                  </div>
                </template>
              </AreaSplit>
            </template>
          </AreaSplit>

          <!-- floating panels -->
          <FloatingPanel v-if="!layersDocked" id="layers" :bounds="wsBounds"><PathList :ed="ed" /></FloatingPanel>
          <FloatingPanel v-if="!propsDocked" id="props" :bounds="wsBounds">
            <div class="side-tabs">
              <div class="tab-bar" role="tablist">
                <button v-for="t in sideTabs" :key="t.id" type="button" role="tab" class="tab" :class="{active: sideTab === t.id}" :aria-selected="sideTab === t.id" @click="setSideTab(t.id)">{{ t.label }}</button>
              </div>
              <div class="tab-panel">
                <Inspector v-if="sideTab === 'props'" :ed="ed" :canvas="canvas" @open-nodes="openNodes" />
                <GridPanel v-else :ed="ed" :canvas="canvas" />
              </div>
            </div>
          </FloatingPanel>
          <FloatingPanel v-if="!nodesDocked" id="nodes" :bounds="wsBounds"><NodeEditor ref="nodeEditor" :ed="ed" /></FloatingPanel>
        </div>
      </div>

      <footer class="status" :class="{error: !!ed.error}" role="status" data-testid="status">{{ message }}</footer>
    </div>
  <SettingsDialog v-if="settingsOpen" @close="settingsOpen = false" />
  <ExportDialog v-if="exportOpen" :ed="ed" :canvas="canvas" @close="exportOpen = false" @files="host.saveFiles" />
  <TooltipRoot />
</template>

<style lang="stylus">
@import '../vendor/tweeq/src/common.styl'
reset-viewport('.TqViewport')
scroll-fade-globals()
</style>

<style>
:root {
  font-family: -apple-system, BlinkMacSystemFont, 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', sans-serif;
  --pw-paper: #ffffff; --pw-ink: #202124; --pw-grid: rgba(0,0,0,.07); --pw-guide: #19b4d6; --pw-snap: #e0457b; --pw-width: #e07b22;
}
html, body, #app {margin: 0; height: 100%; overflow: hidden}
body {background: var(--tq-color-background); color: var(--tq-color-text)}
/* App chrome is not text: dragging anywhere must never start a text selection. */
body {-webkit-user-select: none; user-select: none}
input, textarea, [contenteditable] {-webkit-user-select: text; user-select: text}
/* In full screen the traffic lights are hidden: no inset needed. */
html.fullscreen .app.native .topbar {padding-left: 12px}
/* Pressable vs not: a disabled button loses its fill and most of its ink, so
   it never reads like the live ones beside it. Labels are never filled. */
.TqInputButton:disabled, .TqInputButtonToggle:disabled {
  opacity: .3 !important; background: transparent !important; box-shadow: inset 0 0 0 1px var(--tq-color-border) !important;
}
/* Quieter, flatter corners than Tweeq's defaults (popups 13px, panes 12px). */
:root {--tq-radius-input: 3px !important; --tq-radius-popup: 6px !important; --tq-radius-pane: 6px !important}
</style>

<style scoped>
.app {position: fixed; inset: 0; display: flex; flex-direction: column; font-size: 12px}
.topbar {
  display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 8px 0 12px; flex: none;
  border-bottom: 1px solid var(--tq-color-border); -webkit-app-region: drag;
}
.native .topbar {padding-left: 80px}
.doc {display: flex; align-items: center; gap: 6px; min-width: 0; flex: 1}
.name {font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis}
.dirty {color: var(--tq-color-text-mute); font-size: 9px}
.actions {display: flex; align-items: center; gap: 2px; -webkit-app-region: no-drag}
.sep {width: 1px; height: 16px; background: var(--tq-color-border); margin: 0 4px}
.body {flex: 1; display: flex; min-height: 0}
.workspace {flex: 1; min-width: 0; position: relative}
.area {width: 100%; height: 100%; position: relative}
.side-tabs {display: flex; flex-direction: column; height: 100%}
.tab-bar {display: flex; gap: 2px; padding: 0 8px; border-bottom: 1px solid var(--tq-color-border); flex: none}
.tab {
  height: 28px; padding: 0 8px; border: 0; background: transparent; font: inherit; font-size: 11px; cursor: pointer;
  color: var(--tq-color-text-mute); box-shadow: inset 0 -2px 0 transparent;
}
.tab:hover {color: var(--tq-color-text)}
.tab.active {color: var(--tq-color-text); font-weight: 500; box-shadow: inset 0 -2px 0 var(--tq-color-text)}
.tab-panel {flex: 1; min-height: 0; overflow-y: auto}
.tab-spacer {flex: 1}
.tab-bar {align-items: center}
.status {
  height: 24px; line-height: 24px; padding: 0 12px; flex: none; font-size: 11px;
  color: var(--tq-color-text-mute); border-top: 1px solid var(--tq-color-border);
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.status.error {color: var(--tq-color-error, #d24a3c)}
</style>
