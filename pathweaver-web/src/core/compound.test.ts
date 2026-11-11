import {describe, expect, it} from 'vitest'
import {compoundGroups, gather, isHead, pathsFromRings, remapCompounds} from './compound'
import type {Stroke} from './types'

const s = (id: number, compound?: number): Stroke => ({id, name: `p${id}`, compound} as unknown as Stroke)

describe('compound bookkeeping', () => {
  it('gathers members after the first one and finds heads', () => {
    const d = {strokes: [s(1, 9), s(2), s(3, 9), s(4)]}
    expect(gather(d)).toBe(true)
    expect(d.strokes.map(v => v.id)).toEqual([1, 3, 2, 4])
    expect(gather(d)).toBe(false)
    const g = compoundGroups(d.strokes)
    expect(isHead(d.strokes[0], g)).toBe(true)
    expect(isHead(d.strokes[1], g)).toBe(false)
    expect(isHead(d.strokes[2], g)).toBe(true)
  })
  it('copies get compounds of their own; a lone member becomes a plain path', () => {
    const copies = [s(1, 9), s(2, 9), s(3, 7)]
    let next = 100
    remapCompounds(copies, () => next++)
    expect(copies[0].compound).toBe(100)
    expect(copies[1].compound).toBe(100)
    expect(copies[2].compound).toBeUndefined()
  })
  it('rings become closed members with the template look', () => {
    const t = {...s(5), fill: [1, 2, 3], edge: null, edge_width: 4, join: 'Round', miter_limit: 10, visible: true, folder: 3, appearance: null} as unknown as Stroke
    const ring = [{x: 0, y: 0}, {x: 10, y: 0}, {x: 10, y: 10}].map(p => ({p, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false}))
    let next = 10
    const two = pathsFromRings(t, [ring, ring], 'evenodd', () => next++, 5)
    expect(two.map(v => v.id)[0]).toBe(5)
    expect(two.every(v => v.compound === two[0].compound && v.fill_rule === 'evenodd' && v.closed && v.folder === 3)).toBe(true)
    expect(two[0].start_width.left).toBe(2)
    const one = pathsFromRings(t, [ring], 'nonzero', () => next++)
    expect(one[0].compound).toBeUndefined()
  })
})
