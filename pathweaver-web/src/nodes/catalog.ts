import {call} from '../core/wasm'
import type {GraphNode, KindInfo, NodeInfo, NodeKind} from '../core/types'

export const CATEGORIES: {id: string; label: string; color: string; kinds: NodeKind[]}[] = [
  {id: 'io', label: '入出力', color: '#8a8f98', kinds: ['Input', 'Output', 'Reroute']},
  {id: 'shape', label: '形', color: '#4f9a6b', kinds: ['Rough', 'Zigzag', 'Transform', 'Offset', 'Round', 'Band', 'Pucker', 'Simplify', 'Streak', 'Dissolve', 'Halftone', 'Mezzotint']},
  {id: 'combine', label: '合成', color: '#8a6cc4', kinds: ['Stack', 'Pathfinder', 'Boolean']},
  {id: 'paint', label: '色・効果', color: '#c98a3c', kinds: ['Fill', 'Opacity', 'Shadow', 'Glow', 'Blur']},
  {id: 'raster', label: 'ラスター', color: '#b5527a', kinds: ['Image', 'Rasterize', 'RasterBlur', 'Threshold', 'Noise', 'Trace']},
  {id: 'number', label: '数値', color: '#4f7fc4', kinds: ['Value', 'Math', 'Measure']},
]
export const categoryOf = (k: NodeKind) => CATEGORIES.find(c => c.kinds.includes(k))!

let kinds: KindInfo[] | null = null
export function kindList(): KindInfo[] {
  return (kinds ??= call<KindInfo[]>('kinds'))
}
export const kindInfo = (k: NodeKind) => kindList().find(v => v.kind === k)!

const infoCache = new Map<string, NodeInfo>()
export function nodeInfo(n: GraphNode): NodeInfo {
  // An embedded picture can be megabytes: it never changes the parameter list.
  if (n.picture) n = {...n, picture: null}
  const key = JSON.stringify(n)
  let v = infoCache.get(key)
  if (!v) {
    v = call<NodeInfo>('nodeInfo', {node: n})
    if (infoCache.size > 500) infoCache.clear()
    infoCache.set(key, v)
  }
  return v
}

/** Does the node produce a number (rather than shape layers)? */
export const outputsNumber = (k: NodeKind) => kindInfo(k).number
export const hasOutput = (k: NodeKind) => k !== 'Output'

/** The parameters a fresh node of this kind has (what a number link can drive). */
const paramCache = new Map<NodeKind, string[]>()
export function paramKeys(k: NodeKind): string[] {
  let v = paramCache.get(k)
  if (!v) paramCache.set(k, v = nodeInfo(call<GraphNode>('nodeNew', {kind: k, id: 1, position: [0, 0]})).params.map(p => p.key))
  return v
}

/** Can a new node of this kind continue the link being dragged? A menu
 *  opened from a link lists only these (no pick that can only fail). */
export function fitsLink(k: NodeKind, link: {fromNumber: boolean} | {reverse: {param: string | null}}): boolean {
  if (k === 'Input' || k === 'Output') return false
  const info = kindInfo(k)
  if ('reverse' in link) return link.reverse.param ? info.number : !info.number
  return link.fromNumber ? paramKeys(k).length > 0 : info.inputs > 0
}

export const inputLabels = (n: GraphNode): string[] => {
  if (n.kind === 'Stack') return ['下', '上']
  if (n.kind === 'Boolean') return ['背面', '前面']
  return n.inputs.map(() => '形')
}

export const MATH = [
  ['Add', '足す'], ['Subtract', '引く'], ['Multiply', '掛ける'], ['Divide', '割る'], ['Min', '最小'],
  ['Max', '最大'], ['Power', 'べき乗'], ['Modulo', '余り'], ['Snap', '刻みで丸める'],
] as const
export const BOOLEAN = [['Union', '合体'], ['Difference', '前面で型抜き'], ['Intersect', '交差'], ['Xor', '中マド']] as const
export const JOINS = [['Round', '丸い角'], ['Miter', '鋭い角'], ['Bevel', '面取り']] as const
export const GRAIN = [['Dots', '点'], ['Lines', '線']] as const
export const BAND = [['Outer', '外側'], ['Center', '中央'], ['Inner', '内側']] as const

/** Search with Japanese label and English kind name. */
export function searchKinds(q: string): KindInfo[] {
  const s = q.trim().toLowerCase()
  const list = kindList().filter(k => k.kind !== 'Input' && k.kind !== 'Output')
  if (!s) return list
  // Best match first: exact name, then names starting with the query, then the rest.
  const score = (k: KindInfo) => {
    const names = [k.label.toLowerCase(), k.kind.toLowerCase()]
    if (names.some(n => n === s)) return 0
    if (names.some(n => n.startsWith(s))) return 1
    return names.some(n => n.includes(s)) ? 2 : -1
  }
  return list.map(k => ({k, v: score(k)})).filter(x => x.v >= 0).sort((a, b) => a.v - b.v).map(x => x.k)
}
