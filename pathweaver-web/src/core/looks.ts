// Appearances as data-blocks (Blender's materials): a path uses one, several
// paths can use the same one (edits sync, see editor.syncShared), and any
// path can pick, copy or drop one. Saved sets (appearance_presets) are
// data-blocks with no user: picking one makes a fresh copy.
import type {Editor} from './editor'
import type {Document, Graph, Stroke} from './types'
import {call} from './wasm'

export interface Look {
  key: string          // 's:<shared id>' or 'p:<path id>' (used by one path only)
  name: string
  graph: Graph
  users: Stroke[]
}

const copy = (g: Graph): Graph => JSON.parse(JSON.stringify(g))
export const lookName = (g: Graph, owner?: Stroke) => g.name.trim() || (owner ? `${owner.name}の外観` : '外観')

/** Every appearance in the document, one row per data-block. */
export function looks(d: Document): Look[] {
  const out: Look[] = []
  const byShared = new Map<number, Look>()
  for (const s of d.strokes) {
    const g = s.appearance
    if (!g) continue
    if (g.shared != null) {
      const l = byShared.get(g.shared)
      if (l) { l.users.push(s); continue }
      const n: Look = {key: `s:${g.shared}`, name: lookName(g, s), graph: g, users: [s]}
      byShared.set(g.shared, n)
      out.push(n)
    } else out.push({key: `p:${s.id}`, name: lookName(g, s), graph: g, users: [s]})
  }
  return out
}
export const lookOf = (d: Document, s: Stroke) => looks(d).find(l => l.users.includes(s))

function newSharedId(d: Document) {
  return Math.max(0, ...d.strokes.map(s => s.appearance?.shared ?? 0)) + 1
}
/** A name not used by another appearance: "外観 2", "光彩 3"… */
function freshName(d: Document, base: string) {
  const used = new Set(looks(d).map(l => l.name))
  const stem = base.replace(/ \d+$/, '')
  if (!used.has(stem)) return stem
  for (let i = 2; ; i++) if (!used.has(`${stem} ${i}`)) return `${stem} ${i}`
}

/** Use another path's appearance on this path: both now share it. */
export function linkLook(ed: Editor, target: Stroke, look: Look) {
  if (look.users.includes(target)) return
  ed.edit(d => {
    const src = look.users[0]
    const g = src.appearance!
    if (g.shared == null) {
      g.shared = newSharedId(d)
      if (!g.name.trim()) g.name = freshName(d, lookName(g, src))
    }
    target.appearance = copy(g)
  })
  ed.status = `「${look.name}」を共有しました（${look.users.length + 1}つのパスで使用）`
}
/** Blender's user-count button: this path gets its own copy. */
export function makeSingleUser(ed: Editor, target: Stroke) {
  ed.edit(d => {
    const g = target.appearance
    if (!g) return
    const name = freshName(d, lookName(g, target))
    g.shared = null
    g.name = name
    // a group left with one user is no longer shared
    const rest = d.strokes.filter(s => s !== target && s.appearance?.shared != null)
    for (const s of rest) if (rest.filter(o => o.appearance!.shared === s.appearance!.shared).length === 1) s.appearance!.shared = null
  })
  ed.status = 'このパス専用の外観にしました'
}
/** New: a fresh Input → Output appearance, or a copy of the current one. */
export function newLook(ed: Editor, target: Stroke) {
  ed.edit(d => {
    const g = target.appearance ? copy(target.appearance) : call<Graph>('graphDefault')
    g.shared = null
    g.name = freshName(d, target.appearance ? lookName(target.appearance, target) : '外観')
    target.appearance = g
  })
}
export function unlinkLook(ed: Editor, target: Stroke) {
  ed.edit(d => {
    target.appearance = null
    const rest = d.strokes.filter(s => s.appearance?.shared != null)
    for (const s of rest) if (rest.filter(o => o.appearance!.shared === s.appearance!.shared).length === 1) s.appearance!.shared = null
  })
}
/** A saved set: always a fresh copy for this path. */
export function useSet(ed: Editor, target: Stroke, name: string) {
  const p = ed.doc.appearance_presets.find(v => v.name === name)
  if (!p) return
  ed.edit(() => { target.appearance = {...copy(p.graph), shared: null, name} })
}
