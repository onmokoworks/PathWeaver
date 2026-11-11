// Compound paths (Illustrator's compound path). Members are ordinary paths
// tied by `Stroke.compound`; the core fills them together (holes by the fill
// rule) with the first member's look. This file holds the document-side
// bookkeeping the editor does around that: one look for all members, members
// kept next to each other in the stacking order, fresh ids for copies.
import type {FillRule, PathNode, Stroke} from './types'

/** What every member of a compound shares (the editor keeps them equal). */
export const SHARED_LOOK = ['fill', 'edge', 'fill_opacity', 'edge_opacity', 'fill_gradient', 'edge_gradient', 'edge_width', 'join', 'miter_limit', 'appearance', 'fill_rule', 'visible', 'folder'] as const

/** Members of each compound, in stacking order (back first). */
export function compoundGroups(strokes: Stroke[]): Map<number, Stroke[]> {
  const m = new Map<number, Stroke[]>()
  for (const s of strokes) {
    if (s.compound == null) continue
    const list = m.get(s.compound)
    if (list) list.push(s)
    else m.set(s.compound, [s])
  }
  return m
}

/** The first member stands for the compound (paint, layer row). */
export const isHead = (s: Stroke, groups: Map<number, Stroke[]>) => s.compound == null || groups.get(s.compound)?.[0] === s

/** Each compound's members right after its first one. Returns whether the order changed. */
export function gather(d: {strokes: Stroke[]}): boolean {
  const groups = compoundGroups(d.strokes)
  if (![...groups.values()].some(g => g.length > 1)) return false
  const out: Stroke[] = []
  const done = new Set<number>()
  for (const s of d.strokes) {
    if (s.compound == null) out.push(s)
    else if (!done.has(s.compound)) { done.add(s.compound); out.push(...groups.get(s.compound)!) }
  }
  if (out.every((s, i) => s === d.strokes[i])) return false
  d.strokes = out
  return true
}

/** Copies (duplicate, paste) get compounds of their own; a lone copied member is a plain path. */
export function remapCompounds(copies: Stroke[], nextId: () => number) {
  const count = new Map<number, number>()
  for (const c of copies) if (c.compound != null) count.set(c.compound, (count.get(c.compound) ?? 0) + 1)
  const fresh = new Map<number, number>()
  for (const c of copies) {
    if (c.compound == null) continue
    if (count.get(c.compound)! < 2) { delete c.compound; delete c.fill_rule; continue }
    if (!fresh.has(c.compound)) fresh.set(c.compound, nextId())
    c.compound = fresh.get(c.compound)!
  }
}

export type Ring = Omit<PathNode, 'id'>[]

/**
 * Editable paths for a shape made of closed rings (outlined text, an imported
 * shape with holes) in the look of `template`. Several rings become one
 * compound path with `rule`; the first path keeps `firstId` when given.
 */
export function pathsFromRings(template: Stroke, rings: Ring[], rule: FillRule, nextId: () => number, firstId?: number): Stroke[] {
  const usable = rings.filter(r => r.length > 1)
  const compound = usable.length > 1 ? nextId() : null
  const half = Math.min(1000, (template.edge_width ?? 1) / 2)
  return usable.map((r, k) => {
    const s: Stroke = {
      id: k === 0 && firstId !== undefined ? firstId : nextId(),
      name: k === 0 ? template.name : `${template.name} ${k + 1}`,
      visible: template.visible, closed: true,
      nodes: r.map(n => ({...n, p: {...n.p}, incoming: {...n.incoming}, outgoing: {...n.outgoing}, id: nextId()})),
      widths: [],
      start_width: {left: half, right: half, linked: true, angle: 0}, end_width: {left: half, right: half, linked: true, angle: 0},
      start_angle: 90, end_angle: 90,
      fill: template.fill ? [...template.fill] : null, edge: template.edge ? [...template.edge] : null,
      edge_width: template.edge_width, frame_angle: 0, corners: [], join: template.join, miter_limit: template.miter_limit,
      contours: null, appearance: template.appearance ? JSON.parse(JSON.stringify(template.appearance)) : null,
    } as Stroke
    for (const k of ['fill_opacity', 'edge_opacity', 'fill_gradient', 'edge_gradient'] as const) {
      if (template[k] != null) (s as unknown as Record<string, unknown>)[k] = JSON.parse(JSON.stringify(template[k]))
    }
    if (template.folder != null) s.folder = template.folder
    if (compound != null) {
      s.compound = compound
      if (rule !== 'nonzero') s.fill_rule = rule
    }
    return s
  })
}
