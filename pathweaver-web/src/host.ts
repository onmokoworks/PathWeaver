// Bridge to the macOS host (WKWebView). In a plain browser the same commands
// fall back to file input / downloads so the UI can be developed and tested.
import {watch} from 'vue'
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
  function save(saveAs = false) {
    let text: string
    // Only pictures still placed somewhere are written (deleted ones drop out).
    const used = new Set(ed.doc.strokes.map(s => s.image?.asset).filter(Boolean))
    const assets = Object.fromEntries(Object.entries(ed.doc.assets ?? {}).filter(([k]) => used.has(k)))
    try { text = call<string>('save', {doc: {...ed.doc, assets}}) } catch (e) { ed.error = String(e instanceof Error ? e.message : e); return }
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
    placed: ((name: string, kind: 'svg' | 'image', data: string) => placeHandler?.(name, kind, data)) as never,
  }
  initTextHost(isNative() ? post as never : null)
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
  return {open, save, saveFiles, opened, place}
}
