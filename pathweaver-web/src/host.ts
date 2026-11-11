// Bridge to the macOS host (WKWebView). In a plain browser the same commands
// fall back to file input / downloads so the UI can be developed and tested.
import {ref, watch} from 'vue'
import {settings} from './core/settings'
import {call} from './core/wasm'
import type {Editor} from './core/editor'
import type {Document} from './core/types'
import {installNativePointerLock} from './nativePointerLock'
import {initLibrary} from './core/library'
import {initTextHost} from './core/textOutline'
import type {ExportFile} from './core/exporter'

interface NativeMessage {type: string; [k: string]: unknown}
declare global {
  interface Window {
    webkit?: {messageHandlers?: {pathweaver?: {postMessage(m: string): void}}}
    pathweaver?: Record<string, (...args: never[]) => unknown>
  }
}
const native = () => window.webkit?.messageHandlers?.pathweaver
export const isNative = () => !!native()
const post = (m: NativeMessage) => native()?.postMessage(JSON.stringify(m))

export function createHost(ed: Editor, commands: Record<string, () => void>) {
  const autosavedAt = ref<Date | null>(null)
  if (isNative()) installNativePointerLock(post)
  function open() {
    if (isNative()) return post({type: 'open'})
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.pathweaver,.json'
    input.onchange = async () => {
      const f = input.files?.[0]
      if (f) opened(null, await f.text(), f.name)
    }
    input.click()
  }
  /** Place (⇧⌘P): SVG becomes paths, bitmaps become placed images. */
  function place(onData: (name: string, kind: 'svg' | 'image', data: string) => void, onFiles: (files: File[]) => void) {
    placeHandler = onData
    if (isNative()) return post({type: 'place'})
    const input = document.createElement('input')
    input.type = 'file'
    input.multiple = true
    input.accept = '.svg,image/svg+xml,image/png,image/jpeg,image/gif,image/webp'
    input.onchange = () => onFiles([...(input.files ?? [])])
    input.click()
  }
  let placeHandler: ((name: string, kind: 'svg' | 'image', data: string) => void) | null = null
  function opened(path: string | null, text: string, name?: string) {
    try {
      ed.load(call<Document>('load', {text}), path, name)
      commands.fit?.()
    } catch (e) {
      ed.error = e instanceof Error ? e.message : String(e)
    }
  }
  /** The document as file text. Only pictures still placed somewhere are written. */
  function fileText(): string {
    const used = new Set(ed.doc.strokes.map(s => s.image?.asset).filter(Boolean))
    const assets = Object.fromEntries(Object.entries(ed.doc.assets ?? {}).filter(([k]) => used.has(k)))
    return call<string>('save', {doc: {...ed.doc, assets}})
  }
  function save(saveAs = false) {
    let text: string
    try { text = fileText() } catch (e) { ed.error = String(e instanceof Error ? e.message : e); return }
    if (isNative()) return post({type: 'save', text, path: saveAs ? null : ed.filePath, name: ed.fileName})
    download(text, ed.fileName.endsWith('.pathweaver') ? ed.fileName : `${ed.fileName}.pathweaver`, 'application/json')
    ed.markSaved(null)
  }
  /** Written files from the export dialog: the Mac asks where (one file: a
   *  save panel, several: a folder); the browser downloads each. */
  async function saveFiles(files: ExportFile[]) {
    if (!files.length) return
    if (isNative()) return post({type: 'exportFiles', files})
    for (const [i, f] of files.entries()) {
      // browsers drop downloads started in the same instant: one at a time
      if (i) await new Promise(r => setTimeout(r, 250))
      if (f.text != null) download(f.text, f.name, f.mime)
      else download(Uint8Array.from(atob(f.base64!), c => c.charCodeAt(0)), f.name, f.mime)
    }
    ed.status = files.length > 1 ? `${files.length} ファイルを書き出しました` : `${files[0].name} を書き出しました`
  }
  function download(data: string | Uint8Array, name: string, type: string) {
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([data as BlobPart], {type}))
    a.download = name
    a.click()
    setTimeout(() => URL.revokeObjectURL(a.href), 1000)
  }

  window.pathweaver = {
    command: ((name: string) => commands[name]?.()) as never,
    opened: ((path: string, text: string) => opened(path, text)) as never,
    saved: ((path: string) => { ed.markSaved(path); ed.status = `保存しました：${path}` }) as never,
    exported: ((path: string) => { ed.status = `書き出しました：${path}` }) as never,
    failed: ((message: string) => { ed.error = message }) as never,
    isDirty: (() => ed.dirty) as never,
    recovered: ((data: string) => recovered(data)) as never,
    placed: ((name: string, kind: 'svg' | 'image', data: string) => placeHandler?.(name, kind, data)) as never,
  }
  initTextHost(isNative() ? post as never : null)

  // ---- autosave (設定 → 自動保存) ----
  // While there are unsaved changes, every N minutes: a recovery copy (Mac:
  // Application Support/PathWeaver/Recovery, browser: localStorage) that the
  // next launch offers to restore after a crash or force quit; optionally also
  // the document's own file. Never in the middle of a gesture.
  const RECOVERY = 'pw.recovery'
  let lastAutosave = Date.now()
  function writeRecovery(text: string) {
    const data = JSON.stringify({text, name: ed.fileName, path: ed.filePath, at: new Date().toISOString()})
    if (isNative()) return post({type: 'recovery', data})
    try { localStorage.setItem(RECOVERY, data) } catch { /* too big for the browser's storage: skipped */ }
  }
  function clearRecovery() {
    if (isNative()) return post({type: 'recovery', clear: true})
    try { localStorage.removeItem(RECOVERY) } catch { /* nothing kept */ }
  }
  function autosave() {
    lastAutosave = Date.now()
    let text: string
    try { text = fileText() } catch { return }
    writeRecovery(text)
    if (settings.autosaveFile && ed.filePath && isNative()) post({type: 'save', text, path: ed.filePath, name: ed.fileName})
    autosavedAt.value = new Date()
  }
  setInterval(() => {
    const min = settings.autosave
    if (!min || !ed.dirty || ed.inGesture() || Date.now() - lastAutosave < min * 60_000) return
    autosave()
  }, 5_000)
  // saved, discarded or a new document: the recovery copy has done its job
  watch(() => ed.dirty, d => { if (!d) { clearRecovery(); lastAutosave = Date.now() } })
  /** A recovery copy from a session that ended without saving. */
  function recovered(data: string) {
    let r: {text: string; name?: string; path?: string | null; at?: string}
    try { r = JSON.parse(data) } catch { return clearRecovery() }
    const when = r.at ? new Date(r.at).toLocaleString('ja-JP') : ''
    const ok = window.confirm(`前回、保存されないまま終わった「${r.name ?? '名称未設定'}」の変更が残っています（${when}）。\n復元しますか？（「キャンセル」で破棄）`)
    if (!ok) return clearRecovery()
    try {
      ed.load(call<Document>('load', {text: r.text}), r.path ?? null, r.name)
      ed.markUnsaved()
      ed.status = '復元しました。保存するまで元のファイルは変わっていません'
      commands.fit?.()
    } catch (e) {
      ed.error = `復元できませんでした：${e instanceof Error ? e.message : e}`
    }
  }
  watch(() => [ed.dirty, ed.fileName, ed.filePath], () => {
    document.title = `${ed.dirty ? '• ' : ''}${ed.fileName} — PathWeaver`
    post({type: 'state', dirty: ed.dirty, title: ed.fileName, path: ed.filePath})
  }, {immediate: true})
  watch(() => [ed.history.undo, ed.history.redo, ed.selected.length, ed.anchors.length, ed.revision], () => {
    const d = ed.doc
    post({
      type: 'menu', undo: ed.history.undo > 0, redo: ed.history.redo > 0, selection: ed.selected.length > 0 || ed.anchors.length > 0,
      snap: {snapGrid: d.snap_grid, snapGuides: d.snap_guides, snapTangent: d.snap_tangent, snapNormal: d.snap_normal},
    })
  }, {immediate: true})
  initLibrary(post as never, isNative())
  if (!isNative()) {
    window.addEventListener('beforeunload', e => { if (ed.dirty) e.preventDefault() })
  }
  post({type: 'ready'})
  if (!isNative()) {
    let data: string | null = null
    try { data = localStorage.getItem(RECOVERY) } catch { /* none */ }
    if (data) setTimeout(() => recovered(data!), 300)
  }
  return {open, save, saveFiles, opened, place, autosavedAt, autosave}
}
