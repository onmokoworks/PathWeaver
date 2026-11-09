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

/** Use an appearance on these paths: they all share it (edits sync). */
export function linkLook(ed: Editor, targets: Stroke[], look: Look) {
  const todo = targets.filter(t => !look.users.includes(t))
  if (!todo.length) return
  ed.edit(d => {
    const src = look.users[0]
    const g = src.appearance!
    if (g.shared == null) {
      g.shared = newSharedId(d)
      if (!g.name.trim()) g.name = freshName(d, lookName(g, src))
    }
    for (const t of todo) t.appearance = copy(g)
  })
  ed.status = `「${look.name}」を共有しました（${look.users.length + todo.length}本のパスで使用・編集はすべてに反映）`
}
/** Put a look made from a set (library, built-in, this file) on these paths.
 *  One shared appearance for all of them; if this document already has an
 *  appearance of that name, the paths join it instead of making 「名前 2」. */
export function applySet(ed: Editor, targets: Stroke[], name: string, make: () => Graph) {
  if (!targets.length) return
  const existing = looks(ed.doc).find(l => l.name === name)
  if (existing) return linkLook(ed, targets, existing)
  ed.edit(d => {
    const g = make()
    g.name = name
    g.shared = newSharedId(d)
    for (const t of targets) t.appearance = copy(g)
  })
  ed.status = targets.length > 1 ? `「${name}」を${targets.length}本のパスにかけました（共有・編集はすべてに反映）` : `「${name}」をかけました`
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
/** New: a fresh Input → Output appearance (or a copy of the first path's),
 *  one for all the given paths. */
export function newLook(ed: Editor, targets: Stroke[]) {
  if (!targets.length) return
  ed.edit(d => {
    const base = targets[0].appearance
    const g = base ? copy(base) : call<Graph>('graphDefault')
    g.name = freshName(d, base ? lookName(base, targets[0]) : '外観')
    g.shared = targets.length > 1 ? newSharedId(d) : null
    for (const t of targets) t.appearance = copy(g)
  })
}
export function unlinkLook(ed: Editor, targets: Stroke[]) {
  ed.edit(d => {
    for (const t of targets) t.appearance = null
    const rest = d.strokes.filter(s => s.appearance?.shared != null)
    for (const s of rest) if (rest.filter(o => o.appearance!.shared === s.appearance!.shared).length === 1) s.appearance!.shared = null
  })
}
/** A set saved in this document. */
export function useSet(ed: Editor, targets: Stroke[], name: string) {
  const p = ed.doc.appearance_presets.find(v => v.name === name)
  if (p) applySet(ed, targets, name, () => copy(p.graph))
}

// ---- sets that come with the app ----
type NodeSetup = [kind: string, set?: (n: Record<string, unknown>) => void]
/** A chain Input → … → Output built from fresh nodes. */
function chainGraph(name: string, steps: NodeSetup[]): Graph {
  let g = call<Graph>('graphDefault')
  g.name = name
  let prev = 1
  steps.forEach(([kind, set], i) => {
    const n = call<Record<string, unknown>>('nodeNew', {kind, id: 10 + i, position: [80 + (i + 1) * 230, 90 + (i % 2) * 40]})
    set?.(n)
    g.nodes.push(n as never)
    g = call<Graph>('graphConnect', {graph: g, from: prev, to: 10 + i, port: 0})
    prev = 10 + i
  })
  const out = g.nodes.find(n => n.kind === 'Output')!
  out.position = [80 + (steps.length + 1) * 230, 90]
  return call<Graph>('graphConnect', {graph: g, from: prev, to: g.output, port: 0})
}
export const BUILTIN_SETS: {name: string; hint: string; make: () => Graph}[] = [
  {
    // wkwkdesign: 光彩（内側）→ メゾティント → スタンプ → 画像トレース → ラフ
    name: 'スタンプ', hint: 'ハンコを押したようなかすれとにじみ（光彩→ノイズ→しきい値→トレース→ラフ）',
    make: () => chainGraph('スタンプ', [
      ['Fill', n => { n.color = [221, 221, 221] }],
      ['Glow', n => { n.inner = true; n.color = [0, 0, 0]; n.opacity = 1; n.amount = 26 }],
      ['Noise', n => { n.by_darkness = true; n.amount = 0.5; n.detail = 1; n.seed = 3 }],
      ['RasterBlur', n => { n.blur = 0.6 }],
      ['Threshold', n => { n.amount = 0.42; n.spread = 0 }],
      ['Trace', n => { n.amount = 0.5; n.detail = 0.5; n.value = 2 }],
      ['Rough', n => { n.amount = 1; n.detail = 30; n.smooth = true }],
      ['Fill', n => { n.color = [200, 40, 40] }],
    ]),
  },
]
export function useBuiltin(ed: Editor, targets: Stroke[], name: string) {
  const b = BUILTIN_SETS.find(v => v.name === name)
  if (b) applySet(ed, targets, b.name, b.make)
}
