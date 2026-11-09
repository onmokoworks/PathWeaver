// Panel layout (per viewer): each panel is docked in its place or floats over
// the workspace at a remembered position and size.
import {reactive, watch} from 'vue'

export type PanelId = 'layers' | 'props' | 'nodes'
export interface PanelPlace {floating: boolean; x: number; y: number; w: number; h: number; z: number}

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
