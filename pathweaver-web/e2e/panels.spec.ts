import {test, expect} from '@playwright/test'
import {boot, doc, pen, state} from './helpers'

test('fill: add, pick color by dragging out of the window, remove — no text selection', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}, {p: {x: 100, y: 150}}])
  const insp = page.getByTestId('inspector')
  await insp.getByRole('button', {name: '塗りを追加'}).click()
  expect((await doc(page)).strokes[0].fill).toEqual([32, 33, 36])
  const undo = (await state(page)).undo
  const swatch = insp.locator('[data-paint="塗り"] .TqInputColorPad .default-button')
  const out = async (el: typeof swatch, dir = {x: -140, y: 70}) => {
    const b = (await el.boundingBox())!
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
    await page.mouse.down()
    for (let i = 1; i <= 15; i++) { await page.mouse.move(b.x + b.width / 2 + i * dir.x, b.y + b.height / 2 + i * dir.y); await page.waitForTimeout(16) }
    await page.mouse.up()
  }
  // 1) drag the swatch itself (Tweeq tweak) far outside the window
  await out(swatch)
  expect(await page.evaluate(() => getSelection()?.toString() ?? '')).toBe('')
  expect((await doc(page)).strokes[0].fill).not.toEqual([32, 33, 36])
  expect((await state(page)).undo).toBe(undo + 1)
  // 2) open the popover and drag inside its pad far outside the window
  await swatch.click()
  const pad = page.locator('.TqInputColorPicker .TqInputColorChannelPad').first()
  await expect(pad).toBeVisible()
  const before = (await doc(page)).strokes[0].fill
  await out(pad, {x: -60, y: -90})
  expect(await page.evaluate(() => getSelection()?.toString() ?? '')).toBe('')
  expect((await doc(page)).strokes[0].fill).not.toEqual(before)
  await page.keyboard.press('Escape')
  expect((await state(page)).selected).toHaveLength(1) // Esc closed the popover only
  expect((await state(page)).undo).toBe(undo + 2)
  await page.getByTestId('canvas').screenshot({path: 'test-results/fill.png'})
  await insp.getByRole('button', {name: '塗りをなしにする'}).click({timeout: 3000})
  expect((await doc(page)).strokes[0].fill).toBeNull()
  expect(errors).toEqual([])
})

test('dark theme screenshot', async ({page}) => {
  await boot(page)
  await page.evaluate(() => (window as any).pathweaver.command('example'))
  await page.locator('[data-path]').first().click()
  await page.evaluate(() => (window as any).pathweaver.command('toggleTheme'))
  await page.getByRole('button', {name: 'ノードで外観を作る'}).first().click()
  await page.waitForTimeout(300)
  await page.screenshot({path: 'test-results/dark.png'})
})
