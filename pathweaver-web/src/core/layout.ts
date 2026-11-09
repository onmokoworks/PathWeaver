// Panel layout (per viewer): each panel is docked in its place or floats over
// the workspace at a remembered position and size.
import {reactive, watch} from 'vue'

export type PanelId = 'layers' | 'props' | 'nodes'
export interface PanelPlace {floating: boolean; x: number; y: number; w: number; h: number; z: number; shaded?: boolean}

const KEY = 'pw.layout'
const defaults: Record<PanelId, PanelPlace> = {
  layers: {floating: false, x: 80, y: 60, w: 260, h: 280, z: 1},
  props: {floating: false, x: 380, y: 60, w: 280, h: 420, z: 2},
  nodes: {floating: false, x: 120, y: 260, w: 680, h: 320, z: 3},
}
function load(): Record<PanelId, PanelPlace> {
  const out = JSON.parse(JSON.stringify(defaults)) as Record<PanelId, PanelPlace>
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}')
    for (const id of Object.keys(out) as PanelId[]) Object.assign(out[id], saved[id] ?? {})
  } catch { /* preference only */ }
  return out
}
export const layout = reactive(load())
watch(layout, v => { try { localStorage.setItem(KEY, JSON.stringify(v)) } catch { /* preference only */ } }, {deep: true})

export function setFloating(id: PanelId, floating: boolean) {
  layout[id].floating = floating
  if (floating) raise(id)
}
export function raise(id: PanelId) {
  layout[id].z = Math.max(...Object.values(layout).map(p => p.z)) + 1
}
export const PANEL_TITLES: Record<PanelId, string> = {layers: 'レイヤー', props: 'プロパティ', nodes: 'ノード'}

// ---- docked areas (Blender's areas): a split remembers its size and which
// side is folded to a strip. Folding never unmounts the panel (its view stays).
export type Side = 'first' | 'second'
export interface AreaState {size: number; first: boolean; second: boolean}
const AREA_KEY = 'pw.areas'
const areaDefaults: Record<string, AreaState> = {
  'pw-right': {size: 280, first: false, second: false},
  'pw-nodes': {size: 300, first: false, second: false},
  'pw-side': {size: 200, first: false, second: false},
}
function loadAreas(): Record<string, AreaState> {
  const out = JSON.parse(JSON.stringify(areaDefaults)) as Record<string, AreaState>
  try {
    const saved = JSON.parse(localStorage.getItem(AREA_KEY) ?? '{}')
    for (const k of Object.keys(out)) Object.assign(out[k], saved[k] ?? {})
  } catch { /* preference only */ }
  return out
}
export const areas = reactive(loadAreas())
watch(areas, v => { try { localStorage.setItem(AREA_KEY, JSON.stringify(v)) } catch { /* preference only */ } }, {deep: true})
export function area(name: string): AreaState {
  return (areas[name] ??= {size: 240, first: false, second: false})
}

/** Where each docked panel lives. */
export const PANEL_AREA: Record<PanelId, [string, Side]> = {layers: ['pw-side', 'first'], props: ['pw-side', 'second'], nodes: ['pw-nodes', 'second']}
export function isFolded(id: PanelId) {
  const p = layout[id]
  if (p.floating) return !!p.shaded
  const [name, side] = PANEL_AREA[id]
  return area(name)[side] || (name === 'pw-side' && area('pw-right').second)
}
/** Fold / unfold a panel the same way wherever it is (docked: strip, floating: title bar). */
export function setFolded(id: PanelId, folded: boolean) {
  const p = layout[id]
  if (p.floating) { p.shaded = folded; return }
  const [name, side] = PANEL_AREA[id]
  area(name)[side] = folded
  if (!folded && name === 'pw-side') area('pw-right').second = false
}
export const toggleFolded = (id: PanelId) => setFolded(id, !isFolded(id))
