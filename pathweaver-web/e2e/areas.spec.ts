import {test, expect} from '@playwright/test'
import {boot, pen} from './helpers'

const canvasBox = async (page: any) => (await page.getByTestId('canvas').boundingBox())!

test('N-05 fold like Blender areas: the button, the strip, the divider and Ctrl+Space', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  const h0 = (await canvasBox(page)).height
  // fold the nodes with its own button: a strip stays, the canvas takes the room
  await page.locator('[data-fold="nodes"]').click()
  const strip = page.locator('[data-strip="ノード"]')
  await expect(strip).toBeVisible()
  expect((await strip.boundingBox())!.height).toBeLessThan(30)
  expect((await canvasBox(page)).height).toBeGreaterThan(h0 + 200)
  await expect(page.getByTestId('node-editor')).toBeHidden() // kept mounted, just folded
  expect(await page.getByTestId('node-editor').count()).toBe(1)
  // a click on the strip brings it back at the same size
  await strip.click()
  await expect(page.getByTestId('node-editor')).toBeVisible()
  expect(Math.abs((await canvasBox(page)).height - h0)).toBeLessThan(2)
  // drag the divider almost shut: it folds; drag the strip up: it opens at that size
  const div = page.locator('[data-area-split="pw-nodes"] > .divider')
  const d = (await div.boundingBox())!
  const bottom = (await page.locator('[data-area-split="pw-nodes"]').boundingBox())!
  await page.mouse.move(d.x + 200, d.y); await page.mouse.down()
  await page.mouse.move(d.x + 200, bottom.y + bottom.height - 10, {steps: 8}); await page.mouse.up()
  await expect(strip).toBeVisible()
  const s = (await strip.boundingBox())!
  await page.mouse.move(s.x + 200, s.y + 10); await page.mouse.down()
  await page.mouse.move(s.x + 200, s.y - 250, {steps: 8}); await page.mouse.up()
  await expect(page.getByTestId('node-editor')).toBeVisible()
  const nh = (await page.getByTestId('node-editor').boundingBox())!.height
  expect(nh).toBeGreaterThan(220); expect(nh).toBeLessThan(300)
  // double-click the divider folds / unfolds
  const d2 = (await div.boundingBox())!
  await page.mouse.dblclick(d2.x + 200, d2.y)
  await expect(strip).toBeVisible()
  await strip.click()
  // the layers panel folds the same way; the top bar button folds the nodes
  await page.locator('[data-fold="layers"]').click()
  await expect(page.locator('[data-strip="レイヤー"]')).toBeVisible()
  await page.locator('[data-strip="レイヤー"]').click()
  await page.getByTestId('toggle-nodes').click()
  await expect(strip).toBeVisible()
  await page.getByTestId('toggle-nodes').click()
  await expect(page.getByTestId('node-editor')).toBeVisible()
  // Ctrl+Space over the node editor: it fills the workspace; again: back
  const ne = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(ne.x + 300, ne.y + 100)
  await page.keyboard.press('Control+Space')
  await expect(page.getByTestId('canvas')).toBeHidden()
  await expect(page.getByTestId('inspector')).toBeHidden()
  await page.keyboard.press('Control+Space')
  await expect(page.getByTestId('canvas')).toBeVisible()
  // folding is remembered
  await page.locator('[data-fold="nodes"]').click()
  await page.reload()
  await expect(page.locator('[data-strip="ノード"]')).toBeVisible()
  expect(errors).toEqual([])
})

test('N-05 a floating panel folds to its title bar the same way', async ({page}) => {
  await boot(page)
  await page.locator('[data-pin="layers"]').click()
  const panel = page.locator('[data-floating="layers"]')
  const h = (await panel.boundingBox())!.height
  await panel.locator('[data-fold="layers"]').click()
  expect((await panel.boundingBox())!.height).toBeLessThan(30)
  await panel.getByTestId('floating-bar').dblclick({position: {x: 80, y: 10}})
  expect(Math.abs((await panel.boundingBox())!.height - h)).toBeLessThan(2)
})
