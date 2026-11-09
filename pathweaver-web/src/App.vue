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
import FloatingPanel from './panels/FloatingPanel.vue'
import PanelPin from './panels/PanelPin.vue'
import AreaSplit from './panels/AreaSplit.vue'
import {isFolded, layout, setFolded, toggleFolded, type PanelId} from './core/layout'
import {settings} from './core/settings'
import {createHost, isNative} from './host'
import {call} from './core/wasm'
import type {Document} from './core/types'

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

const ed = createEditor()
const canvas = createCanvas(ed)
const nodeEditor = ref<InstanceType<typeof NodeEditor>>()

const tools: {id: Tool; icon: string; label: string; key: string}[] = [
  {id: 'select', icon: 'pw:select', label: '選択', key: 'V'},
  {id: 'direct', icon: 'pw:direct', label: 'ダイレクト選択（アンカー）', key: 'A'},
  {id: 'pen', icon: 'pw:pen', label: 'ペン', key: 'P'},
  {id: 'convert', icon: 'pw:anchor', label: 'アンカーポイント（角⇄スムーズ）', key: '⇧C'},
]
const toolHint: Record<Tool, string> = {
  select: 'クリックで選択・ドラッグで移動。枠の角で拡大縮小、角の外側で回転（Shiftで比率固定／角度刻み、Optionで複製・中心基準）',
  direct: 'アンカー・ハンドルをドラッグ。線分をOption＋クリック／ダブルクリックでアンカー追加。太さはオレンジの端をドラッグ（Optionで片側）、⌘＋芯クリックで幅点追加、⌘＋端点ドラッグで切り口回転',
  pen: 'クリックで角、ドラッグで曲線。線分上でクリックでアンカー追加。Shift＋パスをクリックでその上から描き始め（次の点は法線方向）。太さはオレンジの端をドラッグ（Optionで片側）。Enter/Escで終了',
  convert: 'アンカーをクリックで角に、ドラッグでスムーズに。ハンドルをドラッグで片側だけ動かす'
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
  export: () => { settle(); host.exportSvg() },
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
}
const blank = JSON.parse(JSON.stringify(ed.doc))
const exampleDoc = () => call<Document>('example')
const host = createHost(ed, commands)

// ---- keyboard routing: text inputs keep their keys; the node editor gets
// keys while the pointer is over it; everything else goes to the canvas. ----
function onKeyDown(e: KeyboardEvent) {
  if (e.isComposing) return
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
  // A Tweeq popover (color picker, dropdown) owns the keyboard while open.
  if (document.querySelector('.Popover:popover-open, [popover]:popover-open')) return
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
  if (mod && k === 'o') { commands.open(); e.preventDefault(); return }
  if (mod && k === 'e') { commands.export(); e.preventDefault(); return }
  if (mod && e.key === ',') { settingsOpen.value = true; e.preventDefault(); return }
  if (mod && k === 'g') { (e.shiftKey ? commands.ungroup : commands.group)(); e.preventDefault(); return }
  if (nodeEditor.value?.isPointerInside() && nodeEditor.value.key(e)) { e.preventDefault(); return }
  if (canvas.key(e)) e.preventDefault()
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
  window.addEventListener('blur', () => { canvas.cancelDrag(); canvas.state.space = false; canvas.state.meta = false; canvas.state.alt = false; if (!typing(document.activeElement)) ed.commit() })
  // Tweeq's color pad/slider gestures end without a confirm event. Releasing
  // the pointer outside a text field ends the gesture: one drag = one undo.
  window.addEventListener('pointerup', () => queueMicrotask(() => {
    if (!typing(document.activeElement) && !canvas.state.drag) ed.commit()
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
;(window as unknown as {__pw: unknown}).__pw = {ed, canvas, call}
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
            <InputButton subtle narrow icon="material-symbols:ios-share" tooltip="SVG書き出し ⌘E" @click="host.exportSvg()" />
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
                <template #first><div class="area" data-area="canvas"><ArtCanvas :ed="ed" :canvas="canvas" v-slot="{bounds}"><ToolPalette :tools="tools" :active="ed.tool" :bounds="bounds" @select="t => canvas.setTool(t)" /></ArtCanvas></div></template>
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
.name {font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis}
.dirty {color: var(--tq-color-accent); font-size: 9px}
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
.tab.active {color: var(--tq-color-text); font-weight: 600; box-shadow: inset 0 -2px 0 var(--tq-color-accent)}
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
