import {test, expect, type Page} from '@playwright/test'
import {boot, doc, drag, click, state} from './helpers'

const strokes = async (page: Page) => (await doc(page)).strokes as any[]

test('shape tools: M draws a rectangle, the adjust panel rebuilds it in the same undo step', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('m')
  await expect(page.locator('[data-tool=shape]')).toHaveAttribute('aria-label', '長方形')
  await drag(page, {x: 50, y: 50}, {x: 250, y: 150})
  let [s] = await strokes(page)
  expect(s.closed).toBe(true)
  expect(s.nodes.map((n: any) => [Math.round(n.p.x), Math.round(n.p.y)])).toEqual([[50, 50], [250, 50], [250, 150], [50, 150]])
  expect((await state(page)).selected).toEqual([s.id])
  expect((await state(page)).undo).toBe(1)

  // Adjust Last Operation: width and corners
  const panel = page.getByTestId('shape-adjust')
  await expect(panel).toBeVisible()
  await panel.locator('[data-adjust=w] input').fill('300')
  await panel.locator('[data-adjust=w] input').press('Enter')
  await panel.locator('[data-adjust=corner] input').fill('20')
  await panel.locator('[data-adjust=corner] input').press('Enter')
  ;[s] = await strokes(page)
  expect(Math.max(...s.nodes.map((n: any) => n.p.x))).toBeCloseTo(350)
  expect(s.corners).toHaveLength(4)
  expect(s.corners[0].radius).toBeCloseTo(20)
  expect((await state(page)).undo).toBe(1)          // still the one step
  // anything else closes the panel; the shape is now just a path
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  await page.keyboard.press('ArrowRight')
  await expect(panel).toBeHidden()
  await page.keyboard.press('Meta+z')
  await page.keyboard.press('Meta+z')
  expect(await strokes(page)).toHaveLength(0)
  expect(errors).toEqual([])
})

test('shape tools: every Illustrator shape from the flyout, ↑ adds sides while drawing, click = default size', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('m')
  // second press on the active shape button opens the list
  await page.locator('[data-tool=shape]').click()
  const fly = page.getByTestId('shape-flyout')
  await expect(fly.locator('[data-shape]')).toHaveCount(10)

  const expectNodes: Record<string, number> = {rounded: 4, ellipse: 4, polygon: 7, star: 10, line: 2, arc: 2, spiral: 11}
  let x = 0
  for (const kind of Object.keys(expectNodes)) {
    if (!(await fly.isVisible())) await page.locator('[data-tool=shape]').click()
    await fly.locator(`[data-shape=${kind}]`).click()
    await expect(fly).toBeHidden()
    x += 120
    if (kind === 'polygon') {
      // drag from the centre and add a side with ↑ on the way
      const a = await (await import('./helpers')).screen(page, {x, y: 100})
      await page.mouse.move(a.x, a.y); await page.mouse.down()
      await page.mouse.move(a.x + 30, a.y + 30, {steps: 4})
      await page.keyboard.press('ArrowUp')
      await page.mouse.move(a.x + 40, a.y + 40, {steps: 2}); await page.mouse.up()
    } else if (kind === 'spiral') {
      await click(page, {x, y: 100})                    // click = default size (10 segments)
    } else {
      await drag(page, {x, y: 60}, {x: x + 80, y: 140})
    }
    const s = (await strokes(page)).at(-1)
    expect(s.nodes.length, kind).toBe(expectNodes[kind])
  }
  const all = await strokes(page)
  expect(all.find((s: any) => s.name.startsWith('直線')).closed).toBe(false)
  expect(all.find((s: any) => s.name.startsWith('角丸長方形')).corners.length).toBe(4)
  expect(errors).toEqual([])
})
