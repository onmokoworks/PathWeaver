// The look library: appearances saved outside any document, usable from every
// document (After Effects' .ffx presets, Blender's asset library). In the Mac
// app each look is one .pwn file in ~/Library/Application Support/
// PathWeaver/Looks; in a browser it lives in localStorage. Files can also be
// exported and imported to share them.
import {reactive} from 'vue'
import type {Graph} from './types'

export interface LibraryLook {name: string; graph: Graph}
type Post = (m: {type: string; [k: string]: unknown}) => void

const KIND = 'pathweaver/look'
export const library = reactive({items: [] as LibraryLook[], native: false})
let post: Post = () => {}

export function lookText(name: string, graph: Graph) {
  return JSON.stringify({kind: KIND, version: 1, name, graph: {...graph, shared: null, name}}, null, 2)
}
function parse(name: string, text: string): LibraryLook | null {
  try {
    const d = JSON.parse(text)
    if (d?.kind !== KIND || !d.graph?.nodes) return null
    return {name: d.name || name, graph: d.graph}
  } catch { return null }
}
const LS = 'pw.library'
function loadLocal() {
  try {
    const m = JSON.parse(localStorage.getItem(LS) ?? '{}') as Record<string, string>
    library.items = Object.entries(m).map(([n, t]) => parse(n, t)).filter((v): v is LibraryLook => !!v)
  } catch { library.items = [] }
}
function storeLocal(update: (m: Record<string, string>) => void) {
  try {
    const m = JSON.parse(localStorage.getItem(LS) ?? '{}') as Record<string, string>
    update(m)
    localStorage.setItem(LS, JSON.stringify(m))
  } catch { /* preference only */ }
  loadLocal()
}

/** Called by the host bridge at startup. */
export function initLibrary(p: Post, native: boolean) {
  post = p
  library.native = native
  const w = window as unknown as {pathweaver?: Record<string, unknown>}
  if (w.pathweaver) w.pathweaver.library = (list: {name: string; text: string}[]) => {
    library.items = list.map(f => parse(f.name, f.text)).filter((v): v is LibraryLook => !!v)
  }
  if (native) post({type: 'libraryList'})
  else loadLocal()
}
export function saveToLibrary(name: string, graph: Graph) {
  const text = lookText(name, graph)
  if (library.native) post({type: 'librarySave', name, text})
  else storeLocal(m => { m[name] = text })
}
export function deleteFromLibrary(name: string) {
  if (library.native) post({type: 'libraryDelete', name})
  else storeLocal(m => { delete m[name] })
}
export function exportLook(name: string, graph: Graph) {
  const text = lookText(name, graph)
  if (library.native) return post({type: 'lookExport', name, text})
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], {type: 'application/json'}))
  a.download = `${name}.pwn`
  a.click()
  setTimeout(() => URL.revokeObjectURL(a.href), 1000)
}
export function importLooks() {
  if (library.native) return post({type: 'lookImport'})
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = '.pwn,.json'
  input.multiple = true
  input.onchange = async () => {
    for (const f of [...(input.files ?? [])]) {
      const l = parse(f.name.replace(/\.pwn$/, ''), await f.text())
      if (l) storeLocal(m => { m[l.name] = lookText(l.name, l.graph) })
    }
  }
  input.click()
}
export function revealLibrary() { post({type: 'libraryReveal'}) }
