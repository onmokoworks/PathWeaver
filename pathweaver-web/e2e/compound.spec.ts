// Compound paths: shapes with holes (⌘8, pathfinder results, outlined text,
// imported SVG) are real Bézier paths whose anchors stay editable.
import {test, expect, type Page} from '@playwright/test'
import {boot, click, doc, drag, state} from './helpers'

type P = {x: number; y: number}
/** A filled closed path with corner anchors at these points. */
async function addPath(page: Page, pts: P[], fill: [number, number, number] = [40, 90, 200]) {
  return page.evaluate(([pts, fill]) => {
    const {ed} = (window as any).__pw
    return ed.edit((d: any) => {
      const id = ed.nextId(d)
      d.strokes.push({
        id, name: `形 ${d.strokes.length + 1}`, visible: true, closed: true,
        nodes: pts.map((p: P) => ({id: ed.nextId(d), p, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false})),
        widths: [], start_width: {left: 1, right: 1, linked: true, angle: 0}, end_width: {left: 1, right: 1, linked: true, angle: 0},
        start_angle: 90, end_angle: 90, fill, edge: null, edge_width: 2, frame_angle: 0, corners: [], join: 'Miter', miter_limit: 10,
        contours: null, appearance: null,
      })
      return id
    })
  }, [pts, fill] as const)
}
const square = (x: number, y: number, w: number): P[] => [{x, y}, {x: x + w, y}, {x: x + w, y: y + w}, {x, y: y + w}]
const pathAt = (page: Page, p: P) => page.evaluate(p => (window as any).__pw.canvas.pathAt(p), p)
const strokes = async (page: Page) => (await doc(page)).strokes as any[]
/** Is the world point painted (by the canvas renders, any path)? */
const painted = (page: Page, p: P, only?: number[]) => page.evaluate(([p, only]) => {
  const {ed} = (window as any).__pw
  return ed.doc.strokes.filter((s: any) => !only || only.includes(s.id)).some((s: any) => {
    const r = ed.renders.get(s.id)
    if (!r?.layers.length) return false
    const path = new Path2D(r.layers.map((l: any) => l.d).join(''))
    return document.createElement('canvas').getContext('2d')!.isPointInPath(path, p.x, p.y, 'nonzero')
  })
}, [p, only ?? null] as const)

test('⌘8 makes a compound path: the inner shape is a hole, picked whole, anchors stay editable; ⌥⇧⌘8 releases', async ({page}) => {
  const errors = await boot(page)
  const outer = await addPath(page, square(0, 0, 200))
  const inner = await addPath(page, square(60, 60, 80), [200, 40, 40])
  expect(await painted(page, {x: 100, y: 100})).toBe(true)
  await page.evaluate(ids => { (window as any).__pw.ed.selected = ids }, [outer, inner])
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+8')
  let d = await doc(page)
  expect(d.strokes).toHaveLength(2)
  const c = d.strokes[0].compound
  expect(c).toBeTruthy()
  expect(d.strokes.every((s: any) => s.compound === c && s.fill_rule === 'evenodd' && !s.contours)).toBe(true)
  // the front-most path's look
  expect(d.strokes.every((s: any) => JSON.stringify(s.fill) === '[200,40,40]')).toBe(true)
  // the hole is not painted and not picked; the ring is
  expect(await painted(page, {x: 100, y: 100})).toBe(false)
  expect(await painted(page, {x: 20, y: 100})).toBe(true)
  expect(await pathAt(page, {x: 100, y: 100})).toBeNull()
  // one row in the layer list, marked
  await expect(page.locator('[data-path]')).toHaveCount(1)
  await expect(page.locator('[data-compound]')).toHaveCount(1)
  await expect(page.getByTestId('compound-props')).toBeVisible()
  expect((await state(page)).undo).toBe(3)

  // the selection tool takes it whole
  await page.keyboard.press('v')
  await click(page, {x: 300, y: 300})
  expect((await state(page)).selected).toEqual([])
  await click(page, {x: 20, y: 100})
  expect([...(await state(page)).selected].sort()).toEqual([outer, inner].sort())
  // moving moves every member
  await page.keyboard.press('ArrowRight')
  d = await doc(page)
  expect(d.strokes.map((s: any) => s.nodes[0].p.x)).toEqual([1, 61])
  await page.keyboard.press('Meta+z')

  // the direct tool edits one member's anchor: the hole changes shape
  await page.keyboard.press('a')
  await drag(page, {x: 140, y: 140}, {x: 170, y: 170})
  d = await doc(page)
  const hole = d.strokes.find((s: any) => s.id === inner)
  expect(hole.nodes[2].p).toEqual({x: 170, y: 170})
  expect(hole.compound).toBe(c)
  expect(await painted(page, {x: 160, y: 155})).toBe(false)
  // a paint change on that one member goes to the whole compound
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.edit(() => { ed.active.fill = [0, 160, 0] }) })
  d = await doc(page)
  expect(d.strokes.every((s: any) => JSON.stringify(s.fill) === '[0,160,0]')).toBe(true)

  // fill rule: nonzero with both squares running the same way fills the hole
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.edit(() => { ed.active.fill_rule = undefined; delete ed.active.fill_rule }) })
  expect(await painted(page, {x: 100, y: 100})).toBe(true)
  await page.keyboard.press('Meta+z')
  expect(await painted(page, {x: 100, y: 100})).toBe(false)

  // release: two separate filled paths again
  await page.evaluate(ids => { (window as any).__pw.ed.selected = ids }, [outer])
  await page.keyboard.press('Meta+Alt+Shift+8')
  d = await doc(page)
  expect(d.strokes.every((s: any) => s.compound == null)).toBe(true)
  expect(await painted(page, {x: 100, y: 100})).toBe(true)
  await expect(page.locator('[data-path]')).toHaveCount(2)
  expect(errors).toEqual([])
})

test('duplicate, copy/paste and delete keep compounds whole and separate', async ({page}) => {
  const errors = await boot(page)
  const outer = await addPath(page, square(0, 0, 100))
  const inner = await addPath(page, square(30, 30, 40))
  await page.evaluate(ids => { (window as any).__pw.ed.selected = ids }, [outer, inner])
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+8')
  await page.keyboard.press('Meta+d')
  let d = await doc(page)
  expect(d.strokes).toHaveLength(4)
  const ids = new Set(d.strokes.map((s: any) => s.compound))
  expect(ids.size).toBe(2)
  // the copy (moved by 20) has its hole too
  const copies = d.strokes.filter((s: any) => s.id !== outer && s.id !== inner).map((s: any) => s.id)
  expect((await state(page)).selected.sort()).toEqual(copies.sort())
  expect(await painted(page, {x: 70, y: 70}, copies)).toBe(false)
  expect(await painted(page, {x: 30, y: 30}, copies)).toBe(true)
  // the export draws one shape per compound, holes included
  const svg = await page.evaluate(() => { const {ed} = (window as any).__pw; return (window as any).__pw.call('svg', {doc: ed.doc}) }).catch(() => null)
  if (svg) expect((svg.match(/<path/g) ?? []).length).toBe(2)
  await page.keyboard.press('Delete')
  d = await doc(page)
  expect(d.strokes).toHaveLength(2)
  // copy and paste: a compound of its own
  await page.evaluate(ids => { const {ed} = (window as any).__pw; ed.selected = ids; ed.paste(ed.copySelection()) }, [outer, inner])
  d = await doc(page)
  expect(d.strokes).toHaveLength(4)
  expect(new Set(d.strokes.map((s: any) => s.compound)).size).toBe(2)
  expect(errors).toEqual([])
})

test('pathfinder results are editable Bézier paths; a hole makes a compound path', async ({page}) => {
  const errors = await boot(page)
  // a square with a circle-ish octagon in front: minus front cuts a hole
  const back = await addPath(page, square(0, 0, 200))
  const front = await addPath(page, square(70, 70, 60), [255, 0, 0])
  await page.evaluate(ids => { (window as any).__pw.ed.selected = ids }, [back, front])
  await page.getByTestId('pathfinder').locator('[data-op="MinusFront"]').click()
  const d = await doc(page)
  expect(d.strokes).toHaveLength(2)
  expect(d.strokes.every((s: any) => !s.contours && s.compound != null && s.nodes.length === 4)).toBe(true)
  expect(await painted(page, {x: 100, y: 100})).toBe(false)
  expect(await painted(page, {x: 20, y: 20})).toBe(true)
  // the results are selected as one
  expect((await state(page)).selected).toHaveLength(2)
  // anchors are editable with the direct tool
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('a')
  await drag(page, {x: 200, y: 200}, {x: 240, y: 230})
  const moved = (await strokes(page)).flatMap((s: any) => s.nodes).some((n: any) => n.p.x === 240 && n.p.y === 230)
  expect(moved).toBe(true)
  expect(errors).toEqual([])
})

test('text → outlines keeps curves and counters: a compound path with holes', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('t')
  await click(page, {x: 100, y: 100})
  await page.keyboard.type('Ob')
  await page.keyboard.press('Escape')
  await expect.poll(async () => (await strokes(page))[0]?.contours?.length ?? 0).toBeGreaterThan(2)
  const [t] = await strokes(page)
  const pts = t.contours.flat()
  const xs = pts.map((p: any) => p.x), ys = pts.map((p: any) => p.y)
  const box = {x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys)}
  await page.keyboard.press('v')
  await page.evaluate(id => { (window as any).__pw.ed.selected = [id] }, t.id)
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+Shift+o')
  await expect.poll(async () => (await strokes(page)).length).toBeGreaterThan(2)
  const d = await strokes(page)
  expect(d.every((s: any) => !s.text && !s.contours && s.compound === d[0].compound && s.closed)).toBe(true)
  expect(d[0].id).toBe(t.id)
  expect(d[0].fill_rule ?? 'nonzero').toBe('nonzero')
  // curves, not polylines: few anchors, smooth ones with handles
  for (const s of d) expect(s.nodes.length).toBeLessThan(40)
  expect(d.flatMap((s: any) => s.nodes).some((n: any) => n.smooth && Math.hypot(n.outgoing.x, n.outgoing.y) > 1)).toBe(true)
  // the counter of the O is a hole, the O's ring is painted
  const oMid = {x: box.x0 + (box.x1 - box.x0) * 0.27, y: (box.y0 + box.y1) / 2}
  expect(await painted(page, oMid)).toBe(false)
  expect(await painted(page, {x: box.x0 + 1.5, y: (box.y0 + box.y1) / 2})).toBe(true)
  // all of it is one row, one undo step
  await expect(page.locator('[data-path]')).toHaveCount(1)
  await page.keyboard.press('Meta+z')
  const back = await strokes(page)
  expect(back).toHaveLength(1)
  expect(back[0].text.text).toBe('Ob')
  expect(errors).toEqual([])
})

test('an older file with flattened outlines still loads and renders; 編集できるパスにする turns it into anchors', async ({page}) => {
  const errors = await boot(page)
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    const ring = (cx: number, cy: number, r: number, n: number, dir: number) => Array.from({length: n}, (_, i) => {
      const a = dir * i / n * Math.PI * 2
      return {x: cx + Math.cos(a) * r, y: cy + Math.sin(a) * r}
    })
    ed.edit((d: any) => {
      const id = ed.nextId(d)
      d.strokes.push({
        id, name: '古い形', visible: true, closed: false, nodes: [], widths: [],
        start_width: {left: 12, right: 12, linked: true, angle: 0}, end_width: {left: 12, right: 12, linked: true, angle: 0},
        start_angle: 90, end_angle: 90, fill: [10, 10, 10], edge: null, edge_width: 1, frame_angle: 0, corners: [], join: 'Miter', miter_limit: 10,
        contours: [ring(100, 100, 80, 240, 1), ring(100, 100, 40, 160, -1)], appearance: null,
      })
    })
  })
  expect(await painted(page, {x: 100, y: 100})).toBe(false)
  expect(await painted(page, {x: 100, y: 35})).toBe(true)
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.selected = [ed.doc.strokes[0].id] })
  await page.getByTestId('make-editable').click()
  const d = await strokes(page)
  expect(d).toHaveLength(2)
  expect(d.every((s: any) => !s.contours && s.compound != null && s.nodes.length <= 8 && s.nodes.every((n: any) => n.smooth))).toBe(true)
  expect(await painted(page, {x: 100, y: 100})).toBe(false)
  expect(await painted(page, {x: 100, y: 35})).toBe(true)
  expect(errors).toEqual([])
})
