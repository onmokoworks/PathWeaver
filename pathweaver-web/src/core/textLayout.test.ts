import {describe, expect, it} from 'vitest'
import {applyM, layoutOnPath, layoutPoint, measurePath, nearestOnPath, pathTextStart, placedRings, pointAt, type Shaped} from './textLayout'
import type {PathNode, TextSpec} from './types'

// A monospace "font": every glyph a 10 × 10 box standing on the baseline, advance 10.
const box = [[{x: 0, y: -10}, {x: 10, y: -10}, {x: 10, y: 0}, {x: 0, y: 0}]]
function shaped(text: string): Shaped {
  const glyphs = text.split('\n').flatMap((l, line) => Array.from(l).map((_, i) => ({line, x: i * 10, advance: 10, rings: box})))
  return {glyphs, lines: text.split('\n').length, ascent: 8, descent: 2}
}
const spec = (p: Partial<TextSpec> = {}): TextSpec => ({text: '', font: 'F', size: 10, line_height: 1, matrix: [1, 0, 0, 1, 0, 0], ...p})
const xs = (ps: {x: number}[]) => ps.map(p => p.x)
const node = (id: number, x: number, y: number): PathNode => ({id, p: {x, y}, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false})

describe('point text', () => {
  it('left text starts at the origin; centre and right keep the origin put', () => {
    const sh = shaped('abcd')
    const left = placedRings(layoutPoint(sh, spec())).flat()
    expect(Math.min(...xs(left))).toBeCloseTo(0)
    expect(Math.max(...xs(left))).toBeCloseTo(40)
    const center = placedRings(layoutPoint(sh, spec({align: 'center'}))).flat()
    expect(Math.min(...xs(center))).toBeCloseTo(-20)
    expect(Math.max(...xs(center))).toBeCloseTo(20)
    const right = placedRings(layoutPoint(sh, spec({align: 'right'}))).flat()
    expect(Math.max(...xs(right))).toBeCloseTo(0)
  })
  it('lines align one by one; justify spreads all but the last line', () => {
    const sh = shaped('abcd\nab')
    const g = layoutPoint(sh, spec({align: 'right'}))
    expect(g[3].m[4] + 10).toBeCloseTo(0)   // 'd' ends at the origin
    expect(g[5].m[4] + 10).toBeCloseTo(0)   // second 'b' too
    expect(g[4].m[5]).toBeCloseTo(g[0].m[5] + 10) // next line box below
    const j = layoutPoint(shaped('ab\nabcd\nx'), spec({align: 'justify'}))
    expect(j[1].m[4]).toBeCloseTo(30)         // 'b' pushed to the widest line's end
    expect(j[6].m[4]).toBeCloseTo(0)          // the last line stays as typed
  })
  it('tracking is 1/1000 em after every glyph; baseline shift raises', () => {
    const g = layoutPoint(shaped('abc'), spec({tracking: 500}))
    expect(g.map(v => v.m[4])).toEqual([0, 15, 30])
    const base = layoutPoint(shaped('a'), spec())[0].m[5]
    expect(layoutPoint(shaped('a'), spec({baseline_shift: 3}))[0].m[5]).toBeCloseTo(base - 3)
    // centred text with tracking stays centred on its full width
    const c = layoutPoint(shaped('ab'), spec({align: 'center', tracking: 1000}))
    expect(c[0].m[4]).toBeCloseTo(-20)
  })
})

describe('text on a path', () => {
  const line = measurePath([node(1, 0, 0), node(2, 100, 0)], false)!
  it('measures a path and finds points on it', () => {
    expect(line.total).toBeCloseTo(100)
    expect(pointAt(line, 25).p.x).toBeCloseTo(25)
    expect(pointAt(line, 120).p.x).toBeCloseTo(120) // open paths continue straight
    expect(nearestOnPath(line, {x: 30, y: 5})).toEqual({offset: 0.3, distance: 5})
  })
  it('glyphs sit on the path, above it, from the offset', () => {
    const t = spec({on_path: {stroke: 1, offset: 0.2}})
    const pts = placedRings(layoutOnPath(shaped('ab'), t, line)).flat()
    expect(Math.min(...xs(pts))).toBeCloseTo(20)
    expect(Math.max(...xs(pts))).toBeCloseTo(40)
    expect(Math.max(...pts.map(p => p.y))).toBeCloseTo(0)  // baseline on the path
    expect(Math.min(...pts.map(p => p.y))).toBeCloseTo(-10) // letters on the left side (above)
  })
  it('centres around the offset and flips to the other side running back', () => {
    const c = placedRings(layoutOnPath(shaped('ab'), spec({align: 'center', on_path: {stroke: 1, offset: 0.5}}), line)).flat()
    expect(Math.min(...xs(c))).toBeCloseTo(40)
    expect(Math.max(...xs(c))).toBeCloseTo(60)
    const f = layoutOnPath(shaped('ab'), spec({on_path: {stroke: 1, offset: 0.5, flip: true}}), line)
    const pts = placedRings(f).flat()
    expect(Math.max(...pts.map(p => p.y))).toBeCloseTo(10)  // below the path now
    expect(Math.max(...xs(pts))).toBeCloseTo(50)             // and running to the left from the anchor
    expect(applyM(f[0].m, {x: 0, y: 0}).x).toBeGreaterThan(applyM(f[1].m, {x: 0, y: 0}).x)
    expect(pathTextStart(shaped('ab'), spec({on_path: {stroke: 1, offset: 0.5, flip: true}}), line).t.x).toBeCloseTo(-1)
  })
  it('turns glyphs to the tangent around a corner', () => {
    const l = measurePath([node(1, 0, 0), node(2, 10, 0), node(3, 10, 100)], false)!
    const g = layoutOnPath(shaped('abc'), spec({on_path: {stroke: 1, offset: 0}}), l)
    // the third glyph's middle is at s = 25: on the vertical leg, turned 90°
    expect(g[2].m[0]).toBeCloseTo(0)
    expect(g[2].m[1]).toBeCloseTo(1)
  })
  it('wraps around a closed path', () => {
    const sq = measurePath([node(1, 0, 0), node(2, 10, 0), node(3, 10, 10), node(4, 0, 10)], true)!
    expect(sq.total).toBeCloseTo(40)
    expect(pointAt(sq, 45).p.x).toBeCloseTo(5)
  })
})
