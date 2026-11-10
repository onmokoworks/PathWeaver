import {describe, expect, it} from 'vitest'
import {flatten, parsePath} from './svgPath'

const end = (d: string) => parsePath(d)[0].segs.at(-1)!.p

describe('parsePath', () => {
  it('reads absolute and relative lines, H/V and close', () => {
    const [s] = parsePath('M10 10 l20 0 V40 h-20 z')
    expect(s.closed).toBe(true)
    expect(s.segs.map(c => c.p)).toEqual([{x: 30, y: 10}, {x: 30, y: 40}, {x: 10, y: 40}])
  })
  it('continues implicit line-tos after M and splits subpaths', () => {
    const subs = parsePath('M0 0 10 0 10 10M20 20L30 30')
    expect(subs).toHaveLength(2)
    expect(subs[0].segs).toHaveLength(2)
    expect(subs[1].start).toEqual({x: 20, y: 20})
  })
  it('reflects the previous control point for S and T', () => {
    const [s] = parsePath('M0 0C0 10 10 10 10 0S20 -10 20 0')
    expect(s.segs[1].c1).toEqual({x: 10, y: -10})
    const [q] = parsePath('M0 0Q5 10 10 0T20 0')
    expect(q.segs[1].p).toEqual({x: 20, y: 0})
    // reflected quadratic control (15,-10) → cubic c1 = p + 2/3 (q - p)
    expect(q.segs[1].c1.y).toBeCloseTo(-20 / 3)
  })
  it('turns arcs into cubics that land on the arc', () => {
    const [s] = parsePath('M0 0A50 50 0 0 1 100 0')
    expect(end('M0 0A50 50 0 0 1 100 0')).toEqual({x: 100, y: 0})
    const ring = flatten(s, 0.05)
    for (const p of ring) expect(Math.hypot(p.x - 50, p.y)).toBeCloseTo(50, 0)
    // sweep=1 goes through negative y (clockwise on screen from the left end)
    expect(Math.min(...ring.map(p => p.y))).toBeLessThan(-49)
  })
  it('reads compact arc flags and exponent numbers', () => {
    expect(end('M0 0a10 10 0 0110 10')).toEqual({x: 10, y: 10})
    expect(end('M0 0L1e2-5e-1')).toEqual({x: 100, y: -0.5})
  })
  it('rejects garbage instead of guessing', () => {
    expect(() => parsePath('M0 0 L x')).toThrow()
  })
})
