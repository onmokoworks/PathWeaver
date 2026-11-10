import {describe, expect, it} from 'vitest'
import {buildShape, defaults, fromDrag} from './shapes'

const at = (b: ReturnType<typeof buildShape>) => b.nodes.map(n => [Math.round(n.p.x * 1000) / 1000, Math.round(n.p.y * 1000) / 1000])
const P = (x: number, y: number) => ({x, y})

describe('shapes', () => {
  it('rect from a drag in any direction; Shift = square; Option = from the centre', () => {
    const r = fromDrag('rect', defaults('rect'), P(100, 100), P(40, 60), false, false, 15)
    expect(at(buildShape(r))).toEqual([[40, 60], [100, 60], [100, 100], [40, 100]])
    const sq = fromDrag('rect', defaults('rect'), P(0, 0), P(30, 10), true, false, 15)
    expect([sq.w, sq.h]).toEqual([30, 30])
    const c = fromDrag('rect', defaults('rect'), P(50, 50), P(60, 70), false, true, 15)
    expect(at(buildShape(c))).toEqual([[40, 30], [60, 30], [60, 70], [40, 70]])
  })
  it('ellipse: four smooth anchors on the box midpoints with circle handles', () => {
    const b = buildShape({...defaults('ellipse'), x: 0, y: 0, w: 200, h: 100})
    expect(at(b)).toEqual([[100, 0], [200, 50], [100, 100], [0, 50]])
    expect(b.nodes.every(n => n.smooth)).toBe(true)
    expect(b.nodes[0].outgoing.x).toBeCloseTo(100 * 0.5522847, 3)
  })
  it('polygon and star: points on their radii, first one straight up', () => {
    const poly = buildShape({...defaults('polygon'), x: 0, y: 0, radius: 10, sides: 6})
    expect(poly.nodes).toHaveLength(6)
    expect(at(poly)[0]).toEqual([0, -10])
    for (const n of poly.nodes) expect(Math.hypot(n.p.x, n.p.y)).toBeCloseTo(10)
    const star = buildShape({...defaults('star'), x: 0, y: 0, radius: 10, radius2: 4, sides: 5})
    expect(star.nodes).toHaveLength(10)
    expect(Math.hypot(star.nodes[1].p.x, star.nodes[1].p.y)).toBeCloseTo(4)
  })
  it('polygon drag: radius and turn follow the pointer; Shift keeps it upright', () => {
    const p = fromDrag('polygon', defaults('polygon'), P(0, 0), P(30, 0), false, false, 15)
    expect(p.radius).toBeCloseTo(30)
    expect(p.angle).toBeCloseTo(90)
    expect(fromDrag('polygon', defaults('polygon'), P(0, 0), P(30, 0), true, false, 15).angle).toBe(0)
  })
  it('line: Shift snaps to the angle step', () => {
    const l = fromDrag('line', defaults('line'), P(0, 0), P(100, 8), true, false, 15)
    expect(l.h).toBeCloseTo(0)
    expect(l.w).toBeCloseTo(Math.hypot(100, 8))
    expect(buildShape(l).closed).toBe(false)
  })
  it('arc: open quarter curve corner to corner; closed adds the centre', () => {
    const a = buildShape({...defaults('arc'), x: 0, y: 0, w: 100, h: 100})
    expect(at(a)).toEqual([[0, 0], [100, 100]])
    expect(a.closed).toBe(false)
    const pie = buildShape({...defaults('arc'), x: 0, y: 0, w: 100, h: 100, closed: true})
    expect(at(pie)[2]).toEqual([100, 0])
    // the curve passes at radius from the centre at its middle (t = 0.5)
    const [n0, n1] = a.nodes
    const c1 = {x: n0.p.x + n0.outgoing.x, y: n0.p.y + n0.outgoing.y}, c2 = {x: n1.p.x + n1.incoming.x, y: n1.p.y + n1.incoming.y}
    const mid = {x: (n0.p.x + 3 * c1.x + 3 * c2.x + n1.p.x) / 8, y: (n0.p.y + 3 * c1.y + 3 * c2.y + n1.p.y) / 8}
    expect(Math.hypot(mid.x - 100, mid.y - 0)).toBeCloseTo(100, 0)
  })
  it('spiral: segments + 1 anchors, shrinking towards the centre, smooth inside', () => {
    const s = buildShape({...defaults('spiral'), x: 0, y: 0, radius: 100, sides: 8, decay: 80})
    expect(s.nodes).toHaveLength(9)
    const d = s.nodes.map(n => Math.hypot(n.p.x, n.p.y))
    expect(d[0]).toBeCloseTo(100)
    expect(d.at(-1)!).toBeLessThan(d[0] * 0.5)
    expect(s.nodes.slice(1, -1).every(n => n.smooth)).toBe(true)
    // tangent continuity: incoming and outgoing are opposite at inner anchors
    for (const n of s.nodes.slice(1, -1)) {
      if (Math.hypot(n.outgoing.x, n.outgoing.y) < 1e-9) continue
      expect(n.incoming.x * n.outgoing.y - n.incoming.y * n.outgoing.x).toBeCloseTo(0, 6)
    }
  })
})
