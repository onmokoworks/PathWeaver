import {describe, expect, it} from 'vitest'
import {blendProblem, blendSteps, syncBlends} from './blend'
import type {Document, Stroke} from './types'

let id = 1
function path(points: [number, number][], closed: boolean, color: [number, number, number]): Stroke {
  return {
    id: id++, name: 'p', visible: true, closed,
    nodes: points.map(([x, y]) => ({id: id++, p: {x, y}, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false})),
    widths: [], start_width: {left: 1, right: 1, linked: true, angle: 0}, end_width: {left: 1, right: 1, linked: true, angle: 0},
    start_angle: 90, end_angle: 90, fill: color, edge: null, edge_width: 1, frame_angle: 0, corners: [], join: 'Miter', miter_limit: 10,
  }
}

describe('blend', () => {
  it('steps sit evenly between the sources and mix the colours', () => {
    const a = path([[0, 0], [10, 0], [10, 10], [0, 10]], true, [0, 0, 0])
    const b = path([[100, 0], [110, 0], [110, 10], [100, 10]], true, [200, 100, 0])
    const steps = blendSteps(a, b, 3)
    expect(steps).toHaveLength(3)
    expect(steps.map(s => s.nodes[0].p.x)).toEqual([25, 50, 75])
    expect(steps[1].fill).toEqual([100, 50, 0])
  })
  it('a triangle and a square: the triangle gets an extra anchor without changing shape', () => {
    const tri = path([[0, 0], [10, 0], [5, 10]], true, [0, 0, 0])
    const sq = path([[50, 0], [60, 0], [60, 10], [50, 10]], true, [0, 0, 0])
    const [mid] = blendSteps(tri, sq, 1)
    expect(mid.nodes).toHaveLength(4)
  })
  it('closed paths are matched start-to-start the best way round', () => {
    const a = path([[0, 0], [10, 0], [10, 10], [0, 10]], true, [0, 0, 0])
    // same square, listed from another corner and the other way round
    const b = path([[10, 10], [10, 0], [0, 0], [0, 10]], true, [0, 0, 0])
    const [mid] = blendSteps(a, b, 1)
    // no twisting: halfway is the same square
    expect(mid.nodes.map(n => [n.p.x, n.p.y])).toEqual([[0, 0], [10, 0], [10, 10], [0, 10]])
  })
  it('refuses mismatched kinds', () => {
    expect(blendProblem(path([[0, 0], [1, 1]], false, [0, 0, 0]), path([[0, 0], [1, 0], [1, 1]], true, [0, 0, 0]))).toMatch(/開いた/)
  })
  it('sync: steps follow a moved source; a deleted source releases the blend', () => {
    const a = path([[0, 0], [10, 0]], false, [0, 0, 0])
    const b = path([[0, 100], [10, 100]], false, [0, 0, 0])
    const d = {strokes: [a, b], blends: [{id: 999, a: a.id, b: b.id, steps: 4}], next_id: 5000} as unknown as Document
    const seen = new Map<number, string>()
    const next = () => d.next_id++
    syncBlends(d, next, seen)
    expect(d.strokes.map(s => s.blend_of ?? null)).toEqual([null, 999, 999, 999, 999, null])
    const stepIds = d.strokes.filter(s => s.blend_of).map(s => s.id)
    b.nodes[0].p.y = 200; b.nodes[1].p.y = 200
    syncBlends(d, next, seen)
    expect(d.strokes.filter(s => s.blend_of).map(s => s.nodes[0].p.y)).toEqual([40, 80, 120, 160])
    expect(d.strokes.filter(s => s.blend_of).map(s => s.id)).toEqual(stepIds)   // ids kept
    d.strokes = d.strokes.filter(s => s.id !== b.id)
    syncBlends(d, next, seen)
    expect(d.blends).toEqual([])
    expect(d.strokes).toHaveLength(1)
  })
})
