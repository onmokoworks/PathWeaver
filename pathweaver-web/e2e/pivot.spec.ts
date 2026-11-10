import {test, expect, type Page} from '@playwright/test'
import {boot, doc, drag, screen, state} from './helpers'

const bounds = (page: Page) => page.evaluate(() => (window as any).__pw.canvas.selectionBounds())
const pivotState = (page: Page) => page.evaluate(() => ({...(window as any).__pw.canvas.state.pivot}))
async function rect(page: Page) {
  await page.evaluate(() => localStorage.removeItem('pw.pivot'))
  await page.evaluate(() => (window as any).__pw.canvas.setPivot({u: 0.5, v: 0.5}))
  await page.keyboard.press('m')
  await drag(page, {x: 100, y: 100}, {x: 300, y: 200})
  await page.keyboard.press('v')
  return bounds(page)
}
async function typeIn(page: Page, field: string, index: number, value: string) {
  const input = page.locator(`[data-field=${field}] input`).nth(index)
  await input.fill(value)
  await input.press('Enter')
}

test('reference point: the locator picks one of nine points; X/Y show it, size and rotation keep it still', async ({page}) => {
  const errors = await boot(page)
  const b0 = await rect(page)
  const x = page.locator('[data-field=position] input').first()
  // default: the centre
  await expect(page.getByTestId('pivot-picker').locator('[data-pivot="0.5 0.5"]')).toHaveAttribute('aria-checked', 'true')
  expect(Number(await x.inputValue())).toBeCloseTo((b0.x0 + b0.x1) / 2, 0)
  // top-left: X/Y read the corner, typing X puts the corner there
  await page.getByTestId('pivot-picker').locator('[data-pivot="0 0"]').click()
  expect(Number(await x.inputValue())).toBeCloseTo(b0.x0, 0)
  await typeIn(page, 'position', 0, '50')
  let b = await bounds(page)
  expect(b.x0).toBeCloseTo(50, 0)
  expect(b.x1 - b.x0).toBeCloseTo(b0.x1 - b0.x0, 0)
  // bottom-right: a new width keeps the right edge where it was
  await page.getByTestId('pivot-picker').locator('[data-pivot="1 1"]').click()
  const right = b.x1, bottom = b.y1
  await typeIn(page, 'size', 0, String(Math.round((b.x1 - b.x0) / 2)))
  b = await bounds(page)
  expect(b.x1).toBeCloseTo(right, 0)
  expect(b.y1).toBeCloseTo(bottom, 0)
  // rotation: 180° around the bottom-right corner flips the box over it
  const before = b
  const undo = (await state(page)).undo
  await typeIn(page, 'rotate', 0, '180')
  b = await bounds(page)
  expect(b.x0).toBeCloseTo(before.x1, 0)
  expect(b.y0).toBeCloseTo(before.y1, 0)
  // …as one undo step, and the dial reads 0 again
  expect((await state(page)).undo).toBe(undo + 1)
  expect(Number(await page.locator('[data-field=rotate] input').first().inputValue())).toBeCloseTo(0)
  expect(errors).toEqual([])
})

test('reference point on the canvas: drag the target (snaps to the nine points), corner rotation turns around it; a path under it still moves', async ({page}) => {
  const errors = await boot(page)
  const b0 = await rect(page)
  const c = {x: (b0.x0 + b0.x1) / 2, y: (b0.y0 + b0.y1) / 2}
  // the middle of a hollow rectangle: no ink there, so the target is grabbed
  await drag(page, c, {x: b0.x0 + 3, y: b0.y0 + 2})
  expect(await pivotState(page)).toEqual({u: 0, v: 0})
  expect((await doc(page)).strokes[0].nodes[0].p).toEqual({x: 100, y: 100}) // nothing moved
  // rotate by the bottom-right corner handle (just outside it): the top-left stays
  const from = {x: b0.x1 + 10, y: b0.y1 + 10}
  const ang = Math.atan2(from.y - b0.y0, from.x - b0.x0) + Math.PI / 2
  const r = Math.hypot(from.x - b0.x0, from.y - b0.y0)
  await drag(page, from, {x: b0.x0 + Math.cos(ang) * r, y: b0.y0 + Math.sin(ang) * r}, {steps: 12})
  const b = await bounds(page)
  // turned 90° clockwise around the old top-left corner: it is now the top-right
  expect(b.x1).toBeCloseTo(b0.x0, 0)
  expect(b.y0).toBeCloseTo(b0.y0, 0)
  // on a corner the target shares the spot with a handle and the ink: ⌘ takes it.
  // ⌘ + double-click: back to the centre
  await page.keyboard.press('Meta+z')
  const at = await screen(page, {x: b0.x0, y: b0.y0})
  await page.keyboard.down('Meta')
  await page.mouse.dblclick(at.x, at.y)
  await page.keyboard.up('Meta')
  expect(await pivotState(page)).toEqual({u: 0.5, v: 0.5})
  expect((await doc(page)).strokes[0].nodes[0].p).toEqual({x: 100, y: 100})
  expect(errors).toEqual([])
})
