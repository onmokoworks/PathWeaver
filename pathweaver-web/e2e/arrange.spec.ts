import {test, expect, type Page} from '@playwright/test'
import {boot, click, doc, drag, screen, state, type Pt} from './helpers'

type B = {x0: number; y0: number; x1: number; y1: number}
/** Geometry bounds of one path (what the selection box measures). */
const boundsOf = (page: Page, id: number): Promise<B> =>
  page.evaluate(id => { const w = (window as any).__pw; return w.canvas.strokeBounds(w.ed.findStroke(id)) }, id)
async function rect(page: Page, a: Pt, b: Pt): Promise<number> {
  await page.keyboard.press('m')
  await drag(page, a, b)
  await page.waitForTimeout(350) // the next press must not read as a double click
  return (await state(page)).selected.at(-1)
}
const ids = async (page: Page) => (await doc(page)).strokes.map((s: {id: number}) => s.id) as number[]

test('align and distribute: to the selection, a key object and the artboard; one undo step each', async ({page}) => {
  const errors = await boot(page)
  const a = await rect(page, {x: 0, y: 0}, {x: 40, y: 40})
  const b = await rect(page, {x: 100, y: 60}, {x: 160, y: 100})
  const c = await rect(page, {x: 300, y: 20}, {x: 340, y: 80})
  await page.keyboard.press('v')
  await page.keyboard.press('Meta+a')
  const panel = page.getByTestId('align')
  await expect(panel).toBeVisible()

  // left: every left edge onto the selection's left edge
  const undo = (await state(page)).undo
  await panel.locator('[data-align=left]').click()
  const left = (await boundsOf(page, a)).x0
  for (const id of [b, c]) expect((await boundsOf(page, id)).x0).toBeCloseTo(left, 3)
  expect((await state(page)).undo).toBe(undo + 1)
  await page.keyboard.press('Meta+z')
  expect((await boundsOf(page, c)).x0).toBeGreaterThan(250)

  // equal gaps between neighbours (the outer two stay)
  await panel.locator('[data-distribute=hgap]').click()
  const [ba, bb, bc] = [await boundsOf(page, a), await boundsOf(page, b), await boundsOf(page, c)]
  expect(bb.x0 - ba.x1).toBeCloseTo(bc.x0 - bb.x1, 3)
  expect(bc.x0).toBeGreaterThan(250)

  // key object: the last clicked (b) stays, the others line up with it
  await page.evaluate(() => { (window as any).__pw.canvas.state.alignTo = 'key' }) // (the 基準 dropdown)
  const kb = await boundsOf(page, b)
  await click(page, {x: kb.x0 + 3, y: (kb.y0 + kb.y1) / 2}) // already selected: becomes the key
  expect((await state(page)).selected).toHaveLength(3)
  await expect(page.getByTestId('key-object')).toBeVisible()
  await panel.locator('[data-align=top]').click()
  expect((await boundsOf(page, b)).y0).toBeCloseTo(kb.y0, 3)
  for (const id of [a, c]) expect((await boundsOf(page, id)).y0).toBeCloseTo(kb.y0, 3)

  // one item: it lines up with the artboard
  await page.evaluate(() => { const ed = (window as any).__pw.ed; ed.edit((d: any) => { d.artboards = [{id: d.next_id++, name: 'A', x: -50, y: -50, w: 500, h: 400}] }) })
  await page.evaluate(id => { (window as any).__pw.ed.selected = [id] }, c)
  await panel.locator('[data-align=right]').click()
  expect((await boundsOf(page, c)).x1).toBeCloseTo(450, 3)
  await panel.locator('[data-align=bottom]').click()
  expect((await boundsOf(page, c)).y1).toBeCloseTo(350, 3)
  expect(errors).toEqual([])
})

test('a folder whose paths are all selected aligns as one piece', async ({page}) => {
  const errors = await boot(page)
  const a = await rect(page, {x: 0, y: 0}, {x: 40, y: 40})
  const b = await rect(page, {x: 60, y: 100}, {x: 100, y: 140})
  const c = await rect(page, {x: 300, y: 300}, {x: 340, y: 340})
  await page.evaluate(([a, b]) => { const ed = (window as any).__pw.ed; ed.selected = [a, b]; ed.groupSelection() }, [a, b])
  const before = [await boundsOf(page, a), await boundsOf(page, b)]
  await page.evaluate(() => { const w = (window as any).__pw; w.ed.selected = w.ed.doc.strokes.map((s: any) => s.id) })
  await page.keyboard.press('v')
  await page.getByTestId('align').locator('[data-align=right]').click()
  const after = [await boundsOf(page, a), await boundsOf(page, b)]
  // the folder kept its shape: both moved by the same amount, its right edge onto c's
  expect(after[0].x0 - before[0].x0).toBeCloseTo(after[1].x0 - before[1].x0, 3)
  expect(after[1].x1).toBeCloseTo((await boundsOf(page, c)).x1, 3)
  expect(errors).toEqual([])
})

test('lock: not picked or edited on the canvas, still drawn; ⌘2 / ⌥⌘2; a locked folder locks its paths', async ({page}) => {
  const errors = await boot(page)
  const a = await rect(page, {x: 0, y: 0}, {x: 80, y: 80})
  const b = await rect(page, {x: 200, y: 0}, {x: 280, y: 80})
  await page.keyboard.press('v')
  // the lock in the layer list
  await page.locator(`[data-lock="${a}"]`).click({force: true})
  expect((await doc(page)).strokes.find((s: any) => s.id === a).locked).toBe(true)
  await page.keyboard.press('Escape')
  await click(page, {x: 2, y: 40})
  expect((await state(page)).selected).toEqual([])
  // dragging over it moves nothing
  const n0 = (await doc(page)).strokes.find((s: any) => s.id === a).nodes[0].p
  await drag(page, {x: 2, y: 40}, {x: 30, y: 60})
  expect((await doc(page)).strokes.find((s: any) => s.id === a).nodes[0].p).toEqual(n0)
  // a marquee over both and ⌘A take only the unlocked one
  await drag(page, {x: 420, y: 160}, {x: -30, y: -30})
  expect((await state(page)).selected).toEqual([b])
  await page.keyboard.press('Meta+a')
  expect((await state(page)).selected).toEqual([b])
  // the direct tool does not grab its anchors
  await page.keyboard.press('a')
  await click(page, {x: 0, y: 0})
  expect((await state(page)).anchors).toEqual([])
  await page.keyboard.press('v')
  // still drawn
  await expect(page.locator(`[data-stroke="${a}"] path`).first()).toBeAttached()
  // ⌘2 locks the selection, ⌥⌘2 unlocks everything and selects it
  await page.keyboard.press('Meta+a')
  await page.keyboard.press('Meta+2')
  expect((await doc(page)).strokes.every((s: any) => s.locked)).toBe(true)
  expect((await state(page)).selected).toEqual([])
  await page.keyboard.press('Alt+Meta+2')
  expect((await doc(page)).strokes.some((s: any) => s.locked)).toBe(false)
  expect([...(await state(page)).selected].sort()).toEqual([a, b].sort())
  // folder lock
  await page.keyboard.press('Meta+g')
  const folder = (await doc(page)).folders[0].id
  await page.locator(`[data-lock-folder="${folder}"]`).click({force: true})
  expect((await state(page)).selected).toEqual([])
  await click(page, {x: 202, y: 40})
  expect((await state(page)).selected).toEqual([])
  await expect(page.locator(`[data-lock="${b}"]`)).toBeDisabled()
  // saved with the file; still exported
  const saved = await page.evaluate(() => { const w = (window as any).__pw; return w.call('save', {doc: {...w.ed.doc, assets: {}}}) })
  expect(String(saved)).toMatch(/"locked":\s*true/)
  const svg = await page.evaluate(() => { const w = (window as any).__pw; return w.call('svg', {doc: {...w.ed.doc, assets: {}}}) })
  expect((String(svg).match(/<path/g) ?? []).length).toBeGreaterThanOrEqual(2)
  expect(errors).toEqual([])
})

test('smart guides: a moved or resized box snaps to another object\'s edge, with a guide line; the toggle turns it off', async ({page}) => {
  const errors = await boot(page)
  // hairlines: a resize handle then sits on the drawn edge (line widths do not scale)
  await page.evaluate(() => { (window as any).__pw.ed.drawStyle.width = 0.01 })
  const a = await rect(page, {x: 0, y: 0}, {x: 100, y: 100})
  const b = await rect(page, {x: 300, y: 250}, {x: 360, y: 300})
  await page.keyboard.press('v')
  const A = await boundsOf(page, a), B0 = await boundsOf(page, b)
  // grab b by its left side and move it so its left edge stops 3 px short of a's right edge
  const grab = {x: B0.x0 + 1, y: B0.y0 + 16} // on the line, clear of the box handles
  const to = {x: grab.x + (A.x1 + 3 - B0.x0), y: grab.y}
  const s0 = await screen(page, grab), s1 = await screen(page, to)
  await page.mouse.move(s0.x, s0.y)
  await page.mouse.down()
  await page.mouse.move(s1.x, s1.y, {steps: 8})
  await expect(page.getByTestId('smart-guides').locator('line')).not.toHaveCount(0) // (a vertical line has no width: attached, not 'visible')
  await page.mouse.up()
  await expect(page.getByTestId('smart-guides')).toHaveCount(0)
  expect((await boundsOf(page, b)).x0).toBeCloseTo(A.x1, 3)
  // resize: the left handle onto a's centre line
  await page.keyboard.press('Meta+z')
  const B1 = await boundsOf(page, b)
  const handle = {x: B1.x0, y: (B1.y0 + B1.y1) / 2}
  await drag(page, handle, {x: (A.x0 + A.x1) / 2 + 2, y: handle.y})
  expect((await boundsOf(page, b)).x0).toBeCloseTo((A.x0 + A.x1) / 2, 0) // (2 off without the snap)
  expect((await boundsOf(page, b)).x1).toBeCloseTo(B1.x1, 0)
  await page.keyboard.press('Meta+z')
  // off: the move lands where the pointer put it
  await page.getByTestId('snap-bar').locator('[data-snap="snap_objects"]').click()
  expect((await doc(page)).snap_objects).toBe(false)
  await drag(page, grab, to)
  expect((await boundsOf(page, b)).x0 - A.x1).toBeGreaterThan(2) // (the mouse moves in whole pixels)
  expect(errors).toEqual([])
})
