import {test, expect, type Page} from '@playwright/test'
import {boot, doc, drag, click, state} from './helpers'

const strokes = async (page: Page) => (await doc(page)).strokes as any[]

test('polar grid: rings and spokes in a folder, counts adjustable afterwards', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('m')
  await page.locator('[data-tool=shape]').click()
  await page.getByTestId('shape-flyout').locator('[data-shape=polar]').click()
  await drag(page, {x: 0, y: 0}, {x: 200, y: 200})
  let d = await doc(page)
  expect(d.strokes).toHaveLength(6 + 5)              // outer + 5 rings, 5 spokes
  expect(new Set(d.strokes.map((s: any) => s.folder)).size).toBe(1)
  expect(d.folders[0].name).toMatch(/^同心円グリッド/)
  const panel = page.getByTestId('shape-adjust')
  await panel.locator('[data-adjust=cols] input').fill('8')
  await panel.locator('[data-adjust=cols] input').press('Enter')
  d = await doc(page)
  expect(d.strokes).toHaveLength(6 + 8)
  expect((await state(page)).undo).toBe(1)
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  await page.keyboard.press('Meta+z')
  expect(await strokes(page)).toHaveLength(0)
  expect(errors).toEqual([])
})

test('blend: ⌥⌘B makes live steps that follow the sources; one click takes the blend; expand', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('l')
  await drag(page, {x: 0, y: 0}, {x: 40, y: 40})
  await drag(page, {x: 300, y: 200}, {x: 400, y: 300})
  const [a, b] = await strokes(page)
  await page.keyboard.press('v')
  await click(page, {x: 20, y: 0})
  await click(page, {x: 350, y: 200}, {shift: true})
  await page.keyboard.press('Meta+Alt+b')
  let all = await strokes(page)
  expect(all).toHaveLength(2 + 5)
  expect(all.map((s: any) => s.blend_of ?? null)).toEqual([null, ...Array(5).fill((await doc(page)).blends[0].id), null])
  // the list shows only the two ends, marked
  await expect(page.locator('[data-blend-source]')).toHaveCount(2)

  // clicking a step selects the blend (both ends); dragging moves it all
  await page.keyboard.press('Escape')
  const mid = all[3].nodes[0].p
  await click(page, mid)
  expect([...(await state(page)).selected].sort()).toEqual([a.id, b.id].sort())

  // move one end alone (from the layer list) — the steps follow
  await page.locator(`[data-path="${b.id}"]`).click()
  await page.keyboard.press('Shift+ArrowDown')
  all = await strokes(page)
  const last = all.filter((s: any) => s.blend_of).at(-1)
  expect(last.nodes[0].p.y).toBeCloseTo(all.at(-1).nodes[0].p.y - (all.at(-1).nodes[0].p.y - all[0].nodes[0].p.y) / 6, 3)

  // steps from the inspector
  await page.locator('[data-field=blend-steps] input').fill('2')
  await page.locator('[data-field=blend-steps] input').press('Enter')
  expect((await strokes(page)).filter((s: any) => s.blend_of)).toHaveLength(2)

  // expand: plain paths in a folder, no blend left
  await page.getByTestId('blend-expand').click()
  const d = await doc(page)
  expect(d.blends ?? []).toEqual([])
  expect(d.strokes).toHaveLength(4)
  expect(d.strokes.every((s: any) => s.blend_of == null)).toBe(true)
  expect(errors).toEqual([])
})
