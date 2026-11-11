import {describe, expect, it} from 'vitest'
import {alignMoves, distributeMoves, smartSnap, snapTargets, union, boxXs, boxYs} from './arrange'

const B = (x0: number, y0: number, x1: number, y1: number) => ({x0, y0, x1, y1})
const xs = (m: {x: number}[]) => m.map(v => Math.round(v.x * 1000) / 1000)
const ys = (m: {y: number}[]) => m.map(v => Math.round(v.y * 1000) / 1000)

describe('align', () => {
  const boxes = [B(0, 0, 10, 10), B(30, 20, 50, 60), B(100, 5, 120, 15)]
  it('to the selection box: edges and centres', () => {
    const to = union(boxes)!
    expect(xs(alignMoves(boxes, to, 'left'))).toEqual([0, -30, -100])
    expect(xs(alignMoves(boxes, to, 'right'))).toEqual([110, 70, 0])
    expect(xs(alignMoves(boxes, to, 'hcenter'))).toEqual([55, 20, -50])
    expect(ys(alignMoves(boxes, to, 'top'))).toEqual([0, -20, -5])
    expect(ys(alignMoves(boxes, to, 'bottom'))).toEqual([50, 0, 45])
    expect(ys(alignMoves(boxes, to, 'vcenter'))).toEqual([25, -10, 20])
  })
  it('to a key object: the key stays', () => {
    const m = alignMoves(boxes, boxes[1], 'left')
    expect(xs(m)).toEqual([30, 0, -70])
    expect(m.every(v => v.y === 0)).toBe(true)
  })
})

describe('distribute', () => {
  const boxes = [B(0, 0, 10, 10), B(15, 0, 45, 10), B(90, 0, 100, 10)]
  it('centres evenly between the outer two (which stay)', () => {
    expect(xs(distributeMoves(boxes, 'hcenter'))).toEqual([0, 20, 0])
  })
  it('equal gaps between neighbours', () => {
    // sizes 10 + 30 + 10 = 50 in a span of 100: two gaps of 25
    expect(xs(distributeMoves(boxes, 'hgap'))).toEqual([0, 20, 0])
  })
  it('order follows position, not selection order', () => {
    const shuffled = [boxes[2], boxes[0], boxes[1]]
    expect(xs(distributeMoves(shuffled, 'hgap'))).toEqual([0, 0, 20])
  })
  it('a fixed gap from the key object, which does not move', () => {
    const m = distributeMoves(boxes, 'hgap', {gap: 5, key: 1})
    // laid out 0-10, 15-45, 50-60, then shifted so the key (index 1) stays
    expect(xs(m)).toEqual([0, 0, -40])
  })
  it('across an artboard: the outer ones go to its edges', () => {
    const m = distributeMoves(boxes, 'vcenter', {range: B(0, 0, 200, 200)})
    expect(ys(m)).toEqual([0, 95, 190])
    const g = distributeMoves(boxes, 'hgap', {range: B(0, 0, 200, 200)})
    expect(xs(g)).toEqual([0, 70, 100])
  })
})

describe('smart guides', () => {
  const t = snapTargets([B(100, 100, 200, 150)])
  it('snaps the nearest edge or centre within the threshold and draws the guide', () => {
    const moving = B(-3, 300, 97, 340) // right edge 97, 3 short of x = 100
    const r = smartSnap(moving, boxXs(moving), boxYs(moving), t, 5)
    expect(r.dx).toBeCloseTo(3)
    expect(r.dy).toBeNull()
    // one vertical guide at x = 100 from the target's top to the moved box's bottom
    expect(r.guides).toEqual([{a: {x: 100, y: 100}, b: {x: 100, y: 340}}])
  })
  it('centres snap too; out of reach nothing happens', () => {
    const m = B(140, 0, 162, 20) // centre 151 vs target centre 150
    expect(smartSnap(m, boxXs(m), boxYs(m), t, 2).dx).toBeCloseTo(-1)
    expect(smartSnap(m, boxXs(m), boxYs(m), t, 0.5).dx).toBeNull()
    expect(smartSnap(m, boxXs(m), boxYs(m), t, 0.5).guides).toEqual([])
  })
  it('only the given positions snap (a resize moves one edge)', () => {
    const m = B(0, 0, 98, 10)
    const r = smartSnap(m, [98], [], t, 5)
    expect(r.dx).toBeCloseTo(2)
    expect(r.dy).toBeNull()
  })
})
