// Editor state: document, selection, undo history and render cache.
// One user gesture = begin() … commit() = one undo step. Everything geometric
// goes through the Rust core (call()).
import {computed, reactive, ref, shallowRef, triggerRef, watch} from 'vue'
import {call, CoreError} from './wasm'
import {createRenderQueue} from './renderQueue'
import type {Anchor, Cap, Document, Folder, Join, Render, RGB, Stroke, Vec2} from './types'

export interface DrawStyle {fill: RGB | null; edge: RGB | null; width: number; join: Join; cap: Cap}

export type Tool = 'select' | 'direct' | 'pen' | 'convert'

interface Snapshot {doc: string; selected: number[]; anchors: Anchor[]; nodes: number[]}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v))

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
  const savedDoc = ref(JSON.stringify(doc.value))
  const revision = ref(0)                     // bumps on every doc change
  const generation = ref(0)                   // bumps when the document is replaced (new/open)

  const undoStack: Snapshot[] = []
  const redoStack: Snapshot[] = []
  const history = reactive({undo: 0, redo: 0})
  let pending: Snapshot | null = null

  const snapshot = (): Snapshot => ({
    doc: JSON.stringify(doc.value),
    selected: [...selected.value],
    anchors: clone(anchors.value),
    nodes: [...selectedNodes.value],
  })
  function restore(s: Snapshot) {
    doc.value = JSON.parse(s.doc)
    selected.value = s.selected.filter(id => doc.value.strokes.some(st => st.id === id))
    anchors.value = s.anchors.filter(a => findStroke(a.stroke)?.nodes.some(n => n.id === a.node))
    selectedNodes.value = s.nodes
    changed()
  }

  function changed() {
    revision.value++
    triggerRef(doc)
  }

  /** Start a gesture. Nested begins are ignored; the outermost gesture wins. */
  function begin() {
    if (!pending) pending = snapshot()
  }
  function commit() {
    if (!pending) return
    const before = pending
    pending = null
    if (before.doc === JSON.stringify(doc.value)) return
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
    savedDoc.value = JSON.stringify(next)
    error.value = ''
    status.value = ''
    generation.value++
    changed()
  }
  const dirty = computed(() => {
    void revision.value
    return JSON.stringify(doc.value) !== savedDoc.value
  })
  function markSaved(path: string | null) {
    savedDoc.value = JSON.stringify(doc.value)
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
        : d.strokes.filter(s => s.id === what.stroke)
      if (!moving.length && what.folder === undefined) return
      const rest = d.strokes.filter(s => !moving.includes(s))
      const fromTop = Math.max(0, Math.min(rest.length, beforeIndex))
      const at = rest.length - fromTop
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
    if (add) {
      selected.value = selected.value.includes(id)
        ? selected.value.filter(v => v !== id)
        : [...selected.value, id]
    } else if (!selected.value.includes(id)) {
      selected.value = [id]
    } else {
      // Clicking an already selected path makes it active without dropping others.
      selected.value = [...selected.value.filter(v => v !== id), id]
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
  function render(s: Stroke): Render {
    const key = JSON.stringify(s)
    const hit = renderCache.get(s.id)
    if (hit && hit.key === key) return hit.render
    let r: Render
    if (s.appearance && s.appearance.enabled && queue.available) {
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
    if (!s || s.contours) return
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
  function duplicateSelection() {
    if (!selected.value.length) return
    edit(d => {
      const copies: number[] = []
      for (const id of selected.value) {
        const s = d.strokes.find(v => v.id === id)
        if (!s) continue
        const c: Stroke = clone(s)
        const remap = new Map<number, number>()
        c.id = nextId(d)
        c.name = `${s.name} のコピー`
        for (const n of c.nodes) {
          const old = n.id
          n.id = nextId(d)
          remap.set(old, n.id)
          n.p = {x: n.p.x + 20, y: n.p.y + 20}
        }
        for (const k of c.widths) {
          k.id = nextId(d)
          k.segment = remap.get(k.segment) ?? k.segment
        }
        for (const corner of c.corners) corner.node = remap.get(corner.node) ?? corner.node
        if (c.contours) c.contours = c.contours.map(r => r.map(p => ({x: p.x + 20, y: p.y + 20})))
        d.strokes.push(c)
        copies.push(c.id)
      }
      selected.value = copies
      anchors.value = []
    })
  }

  watch(selected, () => {
    // Graph node selection belongs to the active path.
    selectedNodes.value = []
  })

  return reactive({
    doc, selected, anchors, selectedNodes, tool, status, error, filePath, fileName, revision, generation, drawStyle,
    history, dirty, active, renders, renderPending,
    begin, commit, cancel, inGesture, edit, coreEdit, changed, undo, redo, load, markSaved,
    findStroke, nextId, shown, folderTree, inFolder, folderOf, groupSelection, ungroup, moveInList, regroup, selectPath, selectAnchor, clearSelection, isAnchorSelected,
    render, newPath, deleteSelection, duplicateSelection,
  })
}

export type Editor = ReturnType<typeof createEditor>
