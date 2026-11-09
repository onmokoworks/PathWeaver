// Per-viewer preferences (not part of the document), kept in localStorage.
// One place for everything that used to be scattered: zoom feel, wheel
// behaviour, artwork background, width handles, theme.
import {reactive, watch} from 'vue'

export type Paper = 'white' | 'gray' | 'black' | 'checker'
export type Wheel = 'pan' | 'zoom'
export type Theme = 'system' | 'light' | 'dark'

export interface Settings {
  /** Multiplies pinch and ⌘/Ctrl-wheel zoom speed. */
  zoomSpeed: number
  /** Plain scroll wheel: pan (trackpads) or zoom (mice). */
  wheel: Wheel
  /** Natural direction for pinch / wheel zoom; true flips it. */
  invertZoom: boolean
  paper: Paper
  showWidth: boolean
  theme: Theme
}

const KEY = 'pw.settings'
const defaults: Settings = {zoomSpeed: 1, wheel: 'pan', invertZoom: false, paper: 'white', showWidth: true, theme: 'system'}

function load(): Settings {
  const out = {...defaults}
  try {
    // older single-key preferences
    const paper = localStorage.getItem('pw.paper')
    if (paper === 'gray' || paper === 'black' || paper === 'checker') out.paper = paper
    if (localStorage.getItem('pw.showWidth') === '0') out.showWidth = false
    Object.assign(out, JSON.parse(localStorage.getItem(KEY) ?? '{}'))
  } catch { /* preferences only */ }
  if (!(out.zoomSpeed >= 0.25 && out.zoomSpeed <= 4)) out.zoomSpeed = 1
  return out
}

export const settings = reactive<Settings>(load())
watch(settings, v => { try { localStorage.setItem(KEY, JSON.stringify(v)) } catch { /* preferences only */ } }, {deep: true})
export const resetSettings = () => Object.assign(settings, defaults)

/** Zoom factor for a wheel/pinch delta, with the viewer's speed and direction. */
export function wheelZoom(deltaY: number, pinch: boolean): number {
  const k = (pinch ? 0.01 : 0.002) * settings.zoomSpeed * (settings.invertZoom ? -1 : 1)
  return Math.exp(-deltaY * k)
}
export function gestureZoom(ratio: number): number {
  const r = Math.pow(ratio, settings.zoomSpeed)
  return settings.invertZoom ? 1 / r : r
}
