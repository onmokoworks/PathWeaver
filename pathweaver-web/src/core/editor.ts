// Editor state: document, selection, undo history and render cache.
// One user gesture = begin() … commit() = one undo step. Everything geometric
// goes through the Rust core (call()).
import {settings} from './settings'
import {computed, reactive, ref, shallowRef, triggerRef, watch} from 'vue'
import {call, CoreError} from './wasm'
import {createRenderQueue} from './renderQueue'
import {blendProblem, syncBlends} from './blend'
import {compoundGroups, gather, isHead, remapCompounds, SHARED_LOOK} from './compound'
import type {Anchor, Cap, Document, Folder, Graph, Join, Render, RGB, Stroke, Vec2} from './types'

export interface DrawStyle {fill: RGB | null; edge: RGB | null; width: number; join: Join; cap: Cap}

export type Tool = 'select' | 'direct' | 'pen' | 'convert' | 'text' | 'shape' | 'artboard'

interface Snapshot {doc: string; selected: number[]; anchors: Anchor[]; nodes: number[]}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v))
/** The document as text for undo / dirty checks: placed images by key only
 *  (their pixels never change once added, and they are megabytes). */
const docText = (d: Document) => JSON.stringify(d, (k, v) => (k === 'assets' && v && typeof v === 'object' ? Object.keys(v).sort() : v))

export function createEditor() {
  const doc = shallowRef<Document>(call<Document>('newDocument'))
  const selected = ref<number[]>([])          // path ids; last = active
  const anchors = ref<Anchor[]>([])           // selection order matters (merge first/last)
  const selectedNodes = ref<number[]>([])     // graph node ids of the active path
  const tool = ref<Tool>('pen')
  const status = ref('')
  const error = ref('')
  const filePath = ref<string | null>(null)
  const fileName = ref('名称未設定')
  const savedDoc = ref(docText(doc.value))
  const revision = ref(0)                     // bumps on every doc change
  const generation = ref(0)                   // bumps when the document is replaced (new/open)

  const undoStack: Snapshot[] = []
  const redoStack: Snapshot[] = []
  const history = reactive({undo: 0, redo: 0})
  let pending: Snapshot | null = null

  const snapshot = (): Snapshot => ({
    doc: docText(doc.value),
    selected: [...selected.value],
    anchors: clone(anchors.value),
    nodes: [...selectedNodes.value],
  })
  function restore(s: Snapshot) {
    const assets = doc.value.assets
    const next = JSON.parse(s.doc) as Document & {assets?: unknown}
    // Snapshots list asset keys only; the pixels stay in the live store.
    const keys = Array.isArray(next.assets) ? next.assets as string[] : []
    next.assets = Object.fromEntries(keys.filter(k => assets?.[k]).map(k => [k, assets![k]]))
    if (!keys.length) delete next.assets
    doc.value = next
    selected.value = s.selected.filter(id => doc.value.strokes.some(st => st.id === id))
    anchors.value = s.anchors.filter(a => findStroke(a.stroke)?.nodes.some(n => n.id === a.node))
    selectedNodes.value = s.nodes
    changed()
  }

  const blendSeen = new Map<number, string>()
  function changed() {
    syncCompounds()
    syncShared()
    syncBlends(doc.value, () => nextId(), blendSeen)
    revision.value++
    triggerRef(doc)
  }
  // A shared appearance (Blender's linked data-block) is one thing used by
  // several paths: an edit to any user's copy goes to every user. The copy
  // that changed since the last sync is the source — not simply the active
  // path, or an edit made elsewhere (the layer list's eye on another path)
  // would be overwritten by the active path's older copy.
  // Embedded pictures are compared by a short signature, not megabytes.
  const sharedSeen = new Map<number, string>()
  const sig = (g: Graph) => JSON.stringify(g, (k, v) => (k === 'png' && typeof v === 'string' ? `${v.length}:${v.slice(-48)}` : v))
  function syncShared() {
    const groups = new Map<number, Stroke[]>()
    for (const o of doc.value.strokes) {
      const id = o.appearance?.shared
      if (id != null) (groups.get(id) ?? groups.set(id, []).get(id)!).push(o)
    }
    for (const id of [...sharedSeen.keys()]) if (!groups.has(id)) sharedSeen.delete(id)
    for (const [id, users] of groups) {
      const sigs = users.map(o => sig(o.appearance!))
      if (sigs.every(v => v === sigs[0])) { sharedSeen.set(id, sigs[0]); continue }
      const seen = sharedSeen.get(id)
      const changed = users.filter((_, i) => sigs[i] !== seen)
      const source = changed.find(o => o === active.value) ?? changed[0] ?? users[0]
      const g = source.appearance!
      for (const o of users) if (o !== source) o.appearance = JSON.parse(JSON.stringify(g)) as Graph
      sharedSeen.set(id, sig(g))
    }
  }

  // A compound path is one shape: its members share one look (paint, line,
  // appearance, fill rule, visibility, folder) and sit next to each other in
  // the stacking order. An edit made on any member (the direct tool selects
  // just one) goes to all — the member that changed since the last sync is
  // the source, like the shared appearance above.
  const compoundSeen = new Map<number, string>()
  const lookSig = (o: Stroke) => JSON.stringify(SHARED_LOOK.map(k => o[k] ?? null), (k, v) => (k === 'png' && typeof v === 'string' ? `${v.length}:${v.slice(-48)}` : v))
  function syncCompounds() {
    const d = doc.value
    gather(d)
    const groups = compoundGroups(d.strokes)
    for (const id of [...compoundSeen.keys()]) if (!groups.has(id)) compoundSeen.delete(id)
    for (const [id, users] of groups) {
      const sigs = users.map(lookSig)
      if (sigs.every(v => v === sigs[0])) { compoundSeen.set(id, sigs[0]); continue }
      const seen = compoundSeen.get(id)
      const changedOnes = users.filter((_, i) => sigs[i] !== seen)
      const source = changedOnes.find(o => o === active.value) ?? changedOnes[0] ?? users[0]
      for (const o of users) {
        if (o === source) continue
        for (const k of SHARED_LOOK) {
          const v = source[k]
          if (v === undefined) delete (o as unknown as Record<string, unknown>)[k]
          else (o as unknown as Record<string, unknown>)[k] = typeof v === 'object' ? JSON.parse(JSON.stringify(v)) : v
        }
      }
      compoundSeen.set(id, lookSig(source))
    }
  }
  const groups = computed(() => { void revision.value; return compoundGroups(doc.value.strokes) })
  /** The paths that make up this one's shape: its compound's members, or itself. */
  function membersOf(id: number): Stroke[] {
    const s = findStroke(id)
    if (!s) return []
    return s.compound != null ? groups.value.get(s.compound) ?? [s] : [s]
  }
  /** Ids with every compound they touch completed (the selection tool takes a compound whole). */
  function withCompounds(ids: number[]): number[] {
    const out: number[] = []
    for (const id of ids) for (const m of membersOf(id)) if (!out.includes(m.id)) out.push(m.id)
    // keep the given active (last) path active
    const last = ids[ids.length - 1]
    return last !== undefined && out.includes(last) ? [...out.filter(v => v !== last), last] : out
  }

  /** Start a gesture. Nested begins are ignored; the outermost gesture wins. */
  function begin() {
    if (!pending) pending = snapshot()
  }
  function commit() {
    if (!pending) return
    const before = pending
    pending = null
    if (before.doc === docText(doc.value)) return
    undoStack.push(before)
    if (undoStack.length > 300) undoStack.shift()
    redoStack.length = 0
    syncHistory()
  }
  function cancel() {
    if (!pending) return
    restore(pending)
    pending = null
  }
  const inGesture = () => pending !== null
  /** Run a mutation as one undo step. Errors from the core roll back and show. */
  function edit<T>(fn: (d: Document) => T): T | undefined {
    const outer = inGesture()
    begin()
    try {
      const result = fn(doc.value)
      changed()
      if (!outer) commit()
      error.value = ''
      return result
    } catch (e) {
      if (!outer) cancel()
      error.value = e instanceof Error ? e.message : String(e)
      return undefined
    }
  }
  /** Replace the document with the result of a core edit operation. */
  function coreEdit<T = unknown>(kind: string, args: Record<string, unknown>): T | undefined {
    try {
      const r = call<{doc: Document; result: T}>('edit', {doc: doc.value, kind, ...args})
      const outer = inGesture()
      begin()
      if (doc.value.assets) r.doc.assets = doc.value.assets // the core got them stripped
      doc.value = r.doc
      changed()
      if (!outer) commit()
      error.value = ''
      return r.result
    } catch (e) {
      error.value = e instanceof CoreError ? e.message : String(e)
      return undefined
    }
  }
  function syncHistory() {
    history.undo = undoStack.length
    history.redo = redoStack.length
  }
  function undo() {
    if (pending) cancel()
    const s = undoStack.pop()
    if (!s) return
    redoStack.push(snapshot())
    restore(s)
    syncHistory()
  }
  function redo() {
    if (pending) cancel()
    const s = redoStack.pop()
    if (!s) return
    undoStack.push(snapshot())
    restore(s)
    syncHistory()
  }

  function load(next: Document, path: string | null, name?: string) {
    pending = null
    undoStack.length = 0
    redoStack.length = 0
    syncHistory()
    doc.value = next
    selected.value = []
    anchors.value = []
    selectedNodes.value = []
    filePath.value = path
    fileName.value = name ?? (path ? path.split('/').pop()! : '名称未設定')
    savedDoc.value = docText(next)
    error.value = ''
    status.value = ''
    generation.value++
    viewer.value = null
    changed()
  }
  const dirty = computed(() => {
    void revision.value
    return docText(doc.value) !== savedDoc.value
  })
  /** A restored recovery copy: its changes are not in any file yet. */
  function markUnsaved() {
    savedDoc.value = ''
    revision.value++
  }
  function markSaved(path: string | null) {
    savedDoc.value = docText(doc.value)
    if (path) {
      filePath.value = path
      fileName.value = path.split('/').pop()!
    }
    revision.value++
  }

  // ---- lookup ----
  function findStroke(id: number): Stroke | undefined {
    return doc.value.strokes.find(s => s.id === id)
  }
  const active = computed<Stroke | undefined>(() => {
    void revision.value
    const id = selected.value[selected.value.length - 1]
    return id === undefined ? undefined : findStroke(id)
  })
  function nextId(d: Document = doc.value) {
    return d.next_id++
  }

  /** Shown on the canvas: the path and its folder are visible. */
  // ---- layer folders (they nest: Folder.parent) ----
  const folderOf = (id: number | null | undefined) => (id == null ? undefined : doc.value.folders?.find(f => f.id === id))
  /** The folder and all folders inside it. */
  function folderTree(id: number, d: Document = doc.value): Set<number> {
    const out = new Set([id])
    let grew = true
    while (grew) {
      grew = false
      for (const f of d.folders ?? []) if (f.parent != null && out.has(f.parent) && !out.has(f.id)) { out.add(f.id); grew = true }
    }
    return out
  }
  /** Is the path inside this folder (directly or in a folder within it)? */
  const inFolder = (s: Stroke, id: number, d: Document = doc.value) => s.folder != null && folderTree(id, d).has(s.folder)
  function shown(s: Stroke): boolean {
    if (!s.visible) return false
    let f = folderOf(s.folder)
    for (let depth = 0; f && depth < 64; depth++) {
      if (!f.visible) return false
      f = folderOf(f.parent)
    }
    return true
  }
  const folderDepth = (f: Folder, d: Document = doc.value) => {
    let n = 0
    for (let p = f.parent; p != null && n < 64; n++) p = d.folders?.find(x => x.id === p)?.parent
    return n
  }

  // Each folder's paths (including nested ones) stay contiguous in the
  // stacking order, so the list shows a folder as one block. Deepest first, so
  // inner blocks are already whole when their parents are gathered.
  function regroup(d: Document) {
    const folders = [...(d.folders ?? [])].sort((a, b) => folderDepth(b, d) - folderDepth(a, d))
    for (const f of folders) {
      const tree = folderTree(f.id, d)
      const isIn = (s: Stroke) => s.folder != null && tree.has(s.folder)
      const idx = d.strokes.map((s, i) => (isIn(s) ? i : -1)).filter(i => i >= 0)
      if (idx.length < 2 || idx[idx.length - 1] - idx[0] === idx.length - 1) continue
      const members = idx.map(i => d.strokes[i])
      const top = idx[idx.length - 1]
      const rest = d.strokes.filter(s => !isIn(s))
      const at = d.strokes.slice(0, top + 1).filter(s => !isIn(s)).length
      rest.splice(at, 0, ...members)
      d.strokes = rest
    }
  }
  /**
   * New folder. With paths selected they move into it (inside the folder they
   * shared, if any); with nothing selected an empty folder is made (inside the
   * active path's folder, if any).
   */
  function groupSelection(name?: string): number | undefined {
    const ids = selected.value.filter(id => findStroke(id))
    return edit(d => {
      d.folders ??= []
      const id = nextId(d)
      const parents = new Set(ids.map(v => findStroke(v)!.folder ?? null))
      const parent = ids.length ? (parents.size === 1 ? [...parents][0] : null) : active.value?.folder ?? null
      d.folders.push({id, name: name ?? `フォルダ ${d.folders.length + 1}`, visible: true, open: true, ...(parent != null ? {parent} : {})})
      for (const s of d.strokes) if (ids.includes(s.id)) s.folder = id
      regroup(d)
      return id
    })
  }
  /** Remove a folder; its paths and folders move up one level. */
  function ungroup(folderId: number) {
    edit(d => {
      const f = d.folders?.find(x => x.id === folderId)
      const up = f?.parent ?? null
      for (const s of d.strokes) if (s.folder === folderId) s.folder = up
      for (const g of d.folders ?? []) if (g.parent === folderId) { if (up == null) delete g.parent; else g.parent = up }
      d.folders = (d.folders ?? []).filter(x => x.id !== folderId)
    })
  }
  /**
   * Move a path or a whole folder in the list. `beforeIndex` counts list
   * positions (paths, top first) among the paths not being moved; `into` is the
   * folder the moved item belongs to afterwards (null = top level).
   */
  function moveInList(what: {stroke?: number; folder?: number}, beforeIndex: number, into: number | null) {
    if (what.folder !== undefined && into != null && folderTree(what.folder).has(into)) return // not into itself
    edit(d => {
      const moving = what.folder !== undefined
        ? d.strokes.filter(s => inFolder(s, what.folder!, d))
        : (() => { const s = d.strokes.find(v => v.id === what.stroke); return s?.compound != null ? d.strokes.filter(v => v.compound === s.compound) : d.strokes.filter(v => v.id === what.stroke) })()
      if (!moving.length && what.folder === undefined) return
      const rest = d.strokes.filter(s => !moving.includes(s))
      // Positions count list rows: a compound path is one row (its first member).
      const g = compoundGroups(rest)
      const rows = rest.filter(s => s.blend_of == null && isHead(s, g))
      const fromTop = Math.max(0, Math.min(rows.length, beforeIndex))
      const above = rows[rows.length - fromTop]
      const at = above ? rest.indexOf(above) : rest.length
      if (what.stroke !== undefined) for (const s of moving) s.folder = into
      else {
        const f = d.folders?.find(x => x.id === what.folder)
        if (f) { if (into == null) delete f.parent; else f.parent = into }
      }
      rest.splice(at, 0, ...moving)
      d.strokes = rest
      regroup(d)
    })
  }

  // ---- selection ----
  function selectPath(id: number, add = false) {
    const target = findStroke(id)
    if (!target || !shown(target)) return
    anchors.value = []
    // A compound path is picked whole (its first member active).
    const all = membersOf(id).map(s => s.id)
    if (all.length > 1) id = all[0]
    const others = all.filter(v => v !== id)
    if (add) {
      selected.value = selected.value.includes(id)
        ? selected.value.filter(v => !all.includes(v))
        : [...selected.value.filter(v => !all.includes(v)), ...others, id]
    } else if (!selected.value.includes(id)) {
      selected.value = [...others, id]
    } else {
      // Clicking an already selected path makes it active without dropping others.
      selected.value = [...selected.value.filter(v => !all.includes(v)), ...others, id]
    }
  }
  function selectAnchor(at: Anchor, add = false) {
    const has = anchors.value.some(a => a.stroke === at.stroke && a.node === at.node)
    if (add && has) {
      anchors.value = anchors.value.filter(a => !(a.stroke === at.stroke && a.node === at.node))
    } else if (add) {
      anchors.value = [...anchors.value, at]
    } else if (!has) {
      anchors.value = [at]
    } else {
      // Clicking an already selected point makes it active (last) without
      // dropping the others, the same rule as paths and nodes.
      anchors.value = [...anchors.value.filter(a => !(a.stroke === at.stroke && a.node === at.node)), at]
    }
    const ids: number[] = []
    for (const a of anchors.value) {
      const i = ids.indexOf(a.stroke)
      if (i >= 0) ids.splice(i, 1)
      ids.push(a.stroke)
    }
    if (ids.length) selected.value = ids
    else if (!add) selected.value = []
  }
  function clearSelection() {
    selected.value = []
    anchors.value = []
  }
  const isAnchorSelected = (at: Anchor) =>
    anchors.value.some(a => a.stroke === at.stroke && a.node === at.node)

  // ---- render cache (per stroke, keyed by its JSON) ----
  // Paths with an appearance graph render in a worker: the centerline and a
  // plain preview are computed here (cheap), the effect result arrives
  // asynchronously and replaces the previous one. The editor never blocks on
  // a heavy graph.
  const renderCache = new Map<number, {key: string; render: Render}>()
  const effectCache = new Map<number, {key: string; render: Render}>()
  const renderTick = ref(0)
  const queue = createRenderQueue((id, key, r) => {
    effectCache.set(id, {key, render: r})
    renderTick.value++
  })
  const renderPending = ref(0)
  function syncRender(s: Stroke): Render {
    try {
      return call<Render>('render', {stroke: s})
    } catch (e) {
      return {layers: [], hit: '', center: '', error: String(e)}
    }
  }
  // Node viewer (Ctrl+Shift+click a node): the canvas shows that node's result
  // for one path. A view only — the document (its links, saves, exports,
  // undo, other users of a shared look) never changes.
  const viewer = ref<{stroke: number; node: number} | null>(null)
  function viewed(s: Stroke): Stroke {
    const v = viewer.value
    const g = s.appearance
    if (!v || v.stroke !== s.id || !g?.enabled || !g.nodes.some(n => n.id === v.node)) return s
    try {
      return {...s, appearance: call<Graph>('graphConnect', {graph: g, from: v.node, to: g.output, port: 0})}
    } catch { return s }
  }
  // Compound paths: the first member paints the whole shape (computed by the
  // core from all members); the others draw only their own centerline, so
  // they show and pick like any path but paint nothing twice.
  const shapeCache = new Map<number, {key: string; stroke: Stroke}>()
  function compoundShape(group: Stroke[], key: string): Stroke {
    const head = group[0]
    const hit = shapeCache.get(head.id)
    if (hit && hit.key === key) return hit.stroke
    let shape: Stroke
    try { shape = call<Stroke>('compoundShape', {strokes: group}) } catch { shape = {...head, contours: []} }
    shapeCache.set(head.id, {key, stroke: shape})
    if (shapeCache.size > 2000) shapeCache.clear()
    return shape
  }
  /** What a path paints: itself, its compound's shape (first member), or nothing but its line (other members). */
  function painted(s: Stroke): {stroke: Stroke; key: string} {
    if (s.compound == null) return {stroke: s, key: JSON.stringify(s)}
    const group = groups.value.get(s.compound) ?? [s]
    if (group[0] !== s) {
      const line = {...s, fill: null, edge: null, appearance: null}
      return {stroke: line, key: 'member:' + JSON.stringify(line)}
    }
    const key = 'compound:' + JSON.stringify(group)
    return {stroke: compoundShape(group, key), key}
  }
  function render(s0: Stroke): Render {
    const p = painted(s0)
    const s = viewed(p.stroke)
    // 外観を表示しない (view setting): the plain look, nothing sent to the worker
    const looks = settings.showLooks
    const key = (looks ? '' : 'plain:') + (s !== p.stroke ? `view:${viewer.value!.node}:` : '') + p.key
    const hit = renderCache.get(s.id)
    if (hit && hit.key === key) return hit.render
    let r: Render
    if (!looks && s.appearance?.enabled) {
      r = syncRender({...s, appearance: null})
    } else if (s.appearance && s.appearance.enabled && queue.available) {
      const plain = syncRender({...s, appearance: null})
      const done = effectCache.get(s.id)
      if (done && done.key === key) {
        r = {...done.render, center: plain.center}
      } else {
        queue.request(s.id, key, s)
        // Until the worker answers: the previous effect result (stale but
        // stable), or the plain look the first time.
        r = done ? {...done.render, center: plain.center, stale: true} : {...plain, stale: true}
        renderCache.set(s.id, {key: '', render: r})
        return r
      }
    } else {
      r = syncRender(s)
    }
    renderCache.set(s.id, {key, render: r})
    return r
  }
  // The path's own geometry (its outline with line widths, no appearance
  // effects): what position, size, the reference point and the selection box
  // measure. It does not depend on view settings (外観 off, the node viewer)
  // or on when the worker finishes. Effects are counted only by exports of
  // the selection / whole drawing, which the core measures itself.
  const geometryCache = new Map<number, {key: string; render: Render}>()
  function geometryRender(s0: Stroke): Render | undefined {
    if (!s0.appearance?.enabled || (s0.compound != null && !isHead(s0, groups.value))) return renders.value.get(s0.id)
    const p = painted(s0)
    const plain = {...p.stroke, appearance: null}
    const key = 'geometry:' + p.key
    const s = s0
    const hit = geometryCache.get(s.id)
    if (hit && hit.key === key) return hit.render
    const r = syncRender(plain)
    geometryCache.set(s.id, {key, render: r})
    if (geometryCache.size > 2000) geometryCache.clear()
    return r
  }
  const renders = computed(() => {
    void revision.value
    void renderTick.value
    const live = new Set(doc.value.strokes.map(s => s.id))
    for (const id of renderCache.keys()) if (!live.has(id)) { renderCache.delete(id); effectCache.delete(id) }
    const m = new Map(doc.value.strokes.map(s => [s.id, render(s)]))
    renderPending.value = queue.busy
    return m
  })

  // ---- common path commands ----
  // ---- style of the next path drawn (Illustrator: the last selected path's
  // look carries over; deselecting keeps it). Not part of the document. ----
  const drawStyle = reactive<DrawStyle>({fill: null, edge: [32, 33, 36], width: 24, join: 'Miter', cap: 'Butt'})
  watch(() => [active.value?.id, revision.value], () => {
    const s = active.value
    if (!s || s.contours || s.compound != null) return
    drawStyle.fill = s.fill ? [...s.fill] as RGB : null
    drawStyle.edge = s.edge ? [...s.edge] as RGB : null
    const ws = [s.start_width, s.end_width, ...s.widths.map(k => k.width)]
    drawStyle.width = Math.max(...ws.map(w => w.left + w.right))
    drawStyle.join = s.join
    drawStyle.cap = s.start_cap ?? 'Butt'
  })
  function newPath(p: Vec2): Stroke {
    const d = doc.value
    const id = nextId(d)
    const half = drawStyle.width / 2
    const s: Stroke = {
      id, name: `パス ${d.strokes.length + 1}`, visible: true, closed: false,
      nodes: [{id: nextId(d), p: {...p}, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false}],
      widths: [],
      start_width: {left: half, right: half, linked: true, angle: 0},
      end_width: {left: half, right: half, linked: true, angle: 0},
      start_angle: 90, end_angle: 90,
      fill: drawStyle.fill ? [...drawStyle.fill] as RGB : null,
      edge: drawStyle.edge ? [...drawStyle.edge] as RGB : null,
      edge_width: 24, frame_angle: 0,
      corners: [], join: drawStyle.join, miter_limit: 10, contours: null, appearance: null,
    }
    if (drawStyle.cap !== 'Butt') { s.start_cap = drawStyle.cap; s.end_cap = drawStyle.cap }
    d.strokes.push(s)
    selected.value = [id]
    return s
  }
  function deleteSelection() {
    if (anchors.value.length) {
      const ids = anchors.value
      coreEdit('deleteAnchors', {anchors: ids})
      anchors.value = []
      return
    }
    if (!selected.value.length) return
    edit(d => {
      d.strokes = d.strokes.filter(s => !selected.value.includes(s.id))
    })
    selected.value = []
  }
  /** A copy of a path with fresh ids, moved by (dx, dy). */
  function copyStroke(d: Document, s: Stroke, dx: number, dy: number, name: string): Stroke {
    const c: Stroke = clone(s)
    const remap = new Map<number, number>()
    c.id = nextId(d)
    c.name = name
    for (const n of c.nodes) {
      const old = n.id
      n.id = nextId(d)
      remap.set(old, n.id)
      n.p = {x: n.p.x + dx, y: n.p.y + dy}
    }
    for (const k of c.widths) {
      k.id = nextId(d)
      k.segment = remap.get(k.segment) ?? k.segment
    }
    for (const corner of c.corners) corner.node = remap.get(corner.node) ?? corner.node
    if (c.contours) c.contours = c.contours.map(r => r.map(p => ({x: p.x + dx, y: p.y + dy})))
    for (const placed of [c.text, c.image]) if (placed) { placed.matrix[4] += dx; placed.matrix[5] += dy }
    return c
  }
  function duplicateSelection() {
    if (!selected.value.length) return
    edit(d => {
      const copies: number[] = []
      for (const id of selected.value) {
        const s = d.strokes.find(v => v.id === id)
        if (!s) continue
        const c = copyStroke(d, s, 20, 20, `${s.name} のコピー`)
        d.strokes.push(c)
        copies.push(c.id)
      }
      remapCompounds(d.strokes.filter(s => copies.includes(s.id)), () => nextId(d))
      selected.value = copies
      anchors.value = []
    })
  }

  // ---- blends (Object → Blend) ----
  /** The blend a path takes part in (as a source or a generated step). */
  function blendOf(id: number) {
    const s = findStroke(id)
    return doc.value.blends?.find(b => b.a === id || b.b === id || b.id === s?.blend_of)
  }
  function makeBlend(steps = 5) {
    const sel = selected.value.map(id => findStroke(id)).filter((s): s is Stroke => !!s && s.blend_of == null)
    if (sel.length !== 2) { status.value = 'ブレンドするパスを2つ選んでください'; return }
    const [a, b] = sel
    const problem = blendProblem(a, b)
    if (problem) { error.value = problem; return }
    if (blendOf(a.id) || blendOf(b.id)) { error.value = 'すでにブレンドに使われているパスです（いったん解除してください）'; return }
    edit(d => { (d.blends ??= []).push({id: nextId(d), a: a.id, b: b.id, steps}) })
    status.value = `ブレンドしました（${steps}段）。どちらかを動かす・形や色を変えると間も追従します`
  }
  function setBlendSteps(id: number, steps: number) {
    const bl = doc.value.blends?.find(b => b.id === id)
    if (!bl) return
    begin()
    bl.steps = Math.max(1, Math.min(200, Math.round(steps)))
    changed()
  }
  /** Release: the steps go away, the two sources stay. */
  function releaseBlend(id: number) {
    edit(d => {
      d.blends = (d.blends ?? []).filter(b => b.id !== id)
      d.strokes = d.strokes.filter(s => s.blend_of !== id)
    })
    blendSeen.delete(id)
  }
  /** Expand: the steps become ordinary paths, gathered with the sources in a folder. */
  function expandBlend(id: number) {
    const bl = doc.value.blends?.find(b => b.id === id)
    if (!bl) return
    edit(d => {
      const members = d.strokes.filter(s => s.blend_of === id || s.id === bl.a || s.id === bl.b)
      d.folders ??= []
      const folder = nextId(d)
      const parent = findStroke(bl.a)?.folder ?? null
      d.folders.push({id: folder, name: 'ブレンド（拡張）', visible: true, open: false, ...(parent != null ? {parent} : {})})
      for (const s of members) { s.blend_of = null; delete s.blend_of; s.folder = folder }
      d.blends = (d.blends ?? []).filter(b => b.id !== id)
      regroup(d)
      selected.value = members.map(s => s.id)
    })
    blendSeen.delete(id)
  }

  /** Illustrator's Make / Release Guides on the selected paths. */
  function makeGuides(on: boolean) {
    const ids = new Set(selected.value)
    const targets = doc.value.strokes.filter(s => ids.has(s.id) && !!s.guide !== on && !s.contours && s.compound == null)
    if (!targets.length) { status.value = on ? 'ガイドにするパスを選んでください' : '解除するガイドを選んでください'; return }
    edit(() => { for (const s of targets) s.guide = on || undefined })
    anchors.value = []
    status.value = on ? `${targets.length}本のパスをガイドにしました（⌥⌘5で解除）` : `${targets.length}本のガイドをパスに戻しました`
  }

  // ---- compound paths (Object → Compound Path) ----
  /** ⌘8: the selected paths become one compound path (holes where one lies inside another).
   *  Old flattened shapes (pathfinder results, outlines from earlier versions) become editable on the way. */
  function makeCompound() {
    if (!selected.value.length) { status.value = '複合パスにするパスを選んでください'; return }
    const ids = coreEdit<number[]>('makeCompound', {ids: selected.value})
    if (!ids) return
    selected.value = [...ids.slice(1), ids[0]]
    anchors.value = []
    status.value = ids.length > 1
      ? '複合パスにしました。内側の形は穴になります（ダイレクト選択でアンカーを直せます・⌥⇧⌘8で解除）'
      : '編集できるパスにしました（ダイレクト選択でアンカーを直せます）'
  }
  /** ⌥⇧⌘8: the members become separate paths again. */
  function releaseCompound() {
    const ids = coreEdit<number[]>('releaseCompound', {ids: selected.value})
    if (!ids) return
    selected.value = ids
    anchors.value = []
    status.value = `複合パスを解除しました（${ids.length}本のパス）`
  }

  /** Stacking order of the selected paths (Illustrator's Arrange). Folders stay together. */
  function arrange(where: 'front' | 'forward' | 'backward' | 'back') {
    const ids = new Set(withCompounds(selected.value))
    if (!ids.size) return
    edit(d => {
      const list = d.strokes
      if (where === 'front' || where === 'back') {
        const moving = list.filter(s => ids.has(s.id)), rest = list.filter(s => !ids.has(s.id))
        d.strokes = where === 'front' ? [...rest, ...moving] : [...moving, ...rest]
      } else {
        // Step past one neighbour — a compound path counts as one.
        const units: Stroke[][] = []
        for (const s of list) {
          const prev = units[units.length - 1]
          if (prev && s.compound != null && prev[0].compound === s.compound) prev.push(s)
          else units.push([s])
        }
        const sel = (u: Stroke[]) => ids.has(u[0].id)
        if (where === 'forward') {
          for (let i = units.length - 2; i >= 0; i--) if (sel(units[i]) && !sel(units[i + 1])) [units[i], units[i + 1]] = [units[i + 1], units[i]]
        } else {
          for (let i = 1; i < units.length; i++) if (sel(units[i]) && !sel(units[i - 1])) [units[i], units[i - 1]] = [units[i - 1], units[i]]
        }
        d.strokes = units.flat()
      }
      regroup(d)
    })
  }

  // ---- clipboard (paths with their widths, corners and appearance) ----
  const CLIP_KIND = 'pathweaver/strokes'
  let pasteCount = 0
  /** The selected paths as clipboard text (back to front), or '' when none. */
  function copySelection(): string {
    const ids = new Set(selected.value)
    const strokes = doc.value.strokes.filter(s => ids.has(s.id))
    if (!strokes.length) return ''
    pasteCount = 0
    // Placed images travel with their pixels (pasting into another document).
    const keys = strokes.map(s => s.image?.asset).filter((k): k is string => !!k)
    const assets = Object.fromEntries(keys.filter(k => doc.value.assets?.[k]).map(k => [k, doc.value.assets![k]]))
    const text = JSON.stringify(keys.length ? {kind: CLIP_KIND, strokes, assets} : {kind: CLIP_KIND, strokes})
    try { localStorage.setItem('pw.clipboard', text) } catch { /* memory only */ }
    clip.value = text
    status.value = `${strokes.length}本のパスをコピーしました`
    return text
  }
  const clip = ref('') // reactive, so menus know when there is something to paste
  function hasClipboard() {
    if (clip.value) return true
    try { return !!localStorage.getItem('pw.clipboard') } catch { return false }
  }
  function cutSelection(): string {
    const text = copySelection()
    if (text) { deleteSelection(); status.value = 'カットしました' }
    return text
  }
  /** Paste paths copied from PathWeaver (this or another document). */
  function paste(text?: string): boolean {
    let src = text ?? clip.value
    if (!src) { try { src = localStorage.getItem('pw.clipboard') ?? '' } catch { src = '' } }
    let data: {kind?: string; strokes?: Stroke[]; assets?: Record<string, string>}
    try { data = JSON.parse(src) } catch { return false }
    if (data?.kind !== CLIP_KIND || !Array.isArray(data.strokes) || !data.strokes.length) return false
    pasteCount++
    const off = 20 * pasteCount
    edit(d => {
      const folders = new Set((d.folders ?? []).map(f => f.id))
      const ids: number[] = []
      for (const [k, v] of Object.entries(data.assets ?? {})) {
        if (typeof v === 'string' && v.startsWith('data:image/') && !d.assets?.[k]) (d.assets ??= {})[k] = v
      }
      for (const s of data.strokes!) {
        const c = copyStroke(d, s, off, off, s.name)
        if (c.folder != null && !folders.has(c.folder)) c.folder = null
        if (c.appearance) c.appearance.shared = null // an independent copy, like Illustrator
        d.strokes.push(c)
        ids.push(c.id)
      }
      remapCompounds(d.strokes.filter(s => ids.includes(s.id)), () => nextId(d))
      call('validate', {doc: d})
      selected.value = ids
      anchors.value = []
    })
    status.value = `${data.strokes.length}本のパスをペーストしました`
    return true
  }

  watch(selected, () => {
    // Graph node selection belongs to the active path.
    selectedNodes.value = []
  })

  return reactive({
    doc, selected, anchors, selectedNodes, tool, status, error, filePath, fileName, revision, generation, drawStyle, viewer, geometryRender,
    history, dirty, active, renders, renderPending,
    begin, commit, cancel, inGesture, edit, coreEdit, changed, undo, redo, load, markSaved, markUnsaved,
    findStroke, nextId, shown, folderTree, inFolder, folderOf, groupSelection, ungroup, moveInList, regroup, selectPath, selectAnchor, clearSelection, isAnchorSelected,
    blendOf, makeBlend, setBlendSteps, releaseBlend, expandBlend, membersOf, withCompounds, makeCompound, releaseCompound,
    render, newPath, deleteSelection, duplicateSelection, copySelection, cutSelection, paste, makeGuides, arrange, hasClipboard,
  })
}

export type Editor = ReturnType<typeof createEditor>
