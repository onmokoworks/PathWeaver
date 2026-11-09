import {test, expect} from '@playwright/test'
import {boot, doc, state} from './helpers'

test('floating tool palette: select tools, drag to move, double-click grip for horizontal, remembered', async ({page}) => {
  const errors = await boot(page)
  const palette = page.getByTestId('tool-palette')
  await expect(palette).toBeVisible()
  await palette.locator('[data-tool=direct]').click()
  expect(await page.evaluate(() => (window as any).__pw.ed.tool)).toBe('direct')
  await palette.locator('[data-tool=pen]').click()
  expect((await doc(page)).strokes).toHaveLength(0) // clicks never reach the canvas

  const grip = page.getByTestId('tool-palette-grip')
  const before = (await palette.boundingBox())!
  const g = (await grip.boundingBox())!
  await page.mouse.move(g.x + g.width / 2, g.y + g.height / 2)
  await page.mouse.down()
  await page.mouse.move(g.x + 300, g.y + 120, {steps: 10})
  await page.mouse.up()
  const moved = (await palette.boundingBox())!
  expect(moved.x - before.x).toBeGreaterThan(250)
  expect(moved.y - before.y).toBeGreaterThan(100)
  expect((await doc(page)).strokes).toHaveLength(0)

  await grip.dblclick()
  const flat = (await palette.boundingBox())!
  expect(flat.width).toBeGreaterThan(flat.height)

  // dragging far outside keeps it inside the canvas
  const g2 = (await grip.boundingBox())!
  await page.mouse.move(g2.x + 5, g2.y + 5)
  await page.mouse.down()
  await page.mouse.move(-500, -500, {steps: 5})
  await page.mouse.up()
  const canvas = (await page.getByTestId('canvas').boundingBox())!
  const inside = (await palette.boundingBox())!
  expect(inside.x).toBeGreaterThanOrEqual(canvas.x + 20)
  expect(inside.y).toBeGreaterThanOrEqual(canvas.y + 20)

  await page.reload()
  await expect(page.getByTestId('tool-palette')).toBeVisible()
  const again = (await page.getByTestId('tool-palette').boundingBox())!
  expect(again.width).toBeGreaterThan(again.height) // orientation remembered
  expect(Math.abs(again.x - inside.x)).toBeLessThan(2)
  void state
  expect(errors).toEqual([])
})
