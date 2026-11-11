import {describe, expect, it} from 'vitest'
import {gradientAngle, gradientEnd, gradientStart, hexAlpha, makeGradient, mixGradient, parseHexAlpha, setPaintKind, stopAt, withAngle} from './paint'
import {blendSteps} from './blend'
import type {Stroke} from './types'

let id = 1
function rect(x: number, fill: [number, number, number] | null): Stroke {
  return {
    id: id++, name: 'r', visible: true, closed: true,
    nodes: [[x, 0], [x + 100, 0], [x + 100, 50], [x, 50]].map(([px, py]) => ({id: id++, p: {x: px, y: py}, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false})),
    widths: [], start_width: {left: 1, right: 1, linked: true, angle: 0}, end_width: {left: 1, right: 1, linked: true, angle: 0},
    start_angle: 90, end_angle: 90, fill, edge: null, edge_width: 1, frame_angle: 0, corners: [], join: 'Miter', miter_limit: 10,
  }
}

describe('paint', () => {
  it('hex with alpha round-trips', () => {
    expect(hexAlpha([255, 0, 16], 1)).toBe('#ff0010')
    expect(parseHexAlpha(hexAlpha([255, 0, 16], 0.5))).toEqual({color: [255, 0, 16], opacity: 0.502})
    expect(parseHexAlpha('#abc')).toEqual({color: [0xaa, 0xbb, 0xcc], opacity: 1})
  })
  it('a new gradient spans the path, keeps the opacity in its stops, and returns to solid', () => {
    const s = rect(0, [10, 20, 30])
    s.fill_opacity = 0.5
    setPaintKind(s, 'fill', 'Linear')
    const g = s.fill_gradient!
    expect(gradientStart(g)).toEqual({x: 0, y: 25})
    expect(gradientEnd(g)).toEqual({x: 100, y: 25})
    expect(g.stops[0]).toEqual({offset: 0, color: [10, 20, 30], opacity: 0.5})
    expect(s.fill_opacity).toBeUndefined()
    setPaintKind(s, 'fill', 'Radial')
    expect(s.fill_gradient!.kind).toBe('Radial')
    expect(gradientStart(s.fill_gradient!)).toEqual({x: 50, y: 25})
    setPaintKind(s, 'fill', 'Solid')
    expect(s.fill_gradient).toBeUndefined()
    expect(s.fill).toEqual([10, 20, 30])
    expect(s.fill_opacity).toBe(0.5)
  })
  it('angle turns about the middle; stops interpolate', () => {
    const s = rect(0, [0, 0, 0])
    const g = withAngle(makeGradient(s, 'fill', 'Linear'), 90)
    expect(gradientAngle(g)).toBeCloseTo(90)
    expect(gradientStart(g).x).toBeCloseTo(50)
    expect(gradientStart(g).y).toBeCloseTo(-25)
    expect(stopAt(g, 0.5)).toEqual({color: [128, 128, 128], opacity: 1})
  })
  it('blends mix opacity, fade a missing paint and match gradients', () => {
    const a = rect(0, [0, 0, 0]), b = rect(200, [200, 0, 0])
    b.fill_opacity = 0
    const [mid] = blendSteps(a, b, 1)
    expect(mid.fill).toEqual([100, 0, 0])
    expect(mid.fill_opacity).toBe(0.5)
    // no fill on one side: the other's colour fades in
    const c = rect(200, null)
    expect(blendSteps(a, c, 1)[0].fill_opacity).toBe(0.5)
    expect(blendSteps(a, c, 1)[0].fill).toEqual([0, 0, 0])
    // gradient against solid: stop by stop
    setPaintKind(a, 'fill', 'Linear')
    const [g] = blendSteps(a, b, 1)
    expect(g.fill_gradient!.stops.map(s => s.color)).toEqual([[100, 0, 0], [228, 128, 128]])
    expect(gradientStart(g.fill_gradient!)).toEqual({x: 0, y: 25})
    // different kinds: the nearer one
    expect(mixGradient(a.fill_gradient!, {...a.fill_gradient!, kind: 'Radial'}, 0.8)!.kind).toBe('Radial')
  })
})
