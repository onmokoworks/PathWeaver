import {test, expect} from '@playwright/test'
import {boot, pen} from './helpers'

test('Tweeq inputs in place: dials for angles, 2D pad for position, linked size, bars for ranges', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 50}}, {p: {x: 200, y: 80}}])
  await expect(page.locator('[data-field=cap-start] .TqInputRotary')).toBeVisible()
  await expect(page.locator('[data-field=position] .TqInputTranslate').first()).toBeVisible()
  await expect(page.locator('[data-field=size].TqInputSize, [data-field=size] .chain').first()).toBeVisible()
  // no icon is blank (the vendored icons resolve offline)
  expect(await page.evaluate(() => [...document.querySelectorAll('svg.iconify')].filter(e => !e.innerHTML.trim()).length)).toBe(0)
  // typing a size with the ratio linked scales both sides
  const w = page.locator('[data-field=size] input').first()
  await w.fill('400'); await w.press('Enter')
  const b = await page.evaluate(() => (window as any).__pw.canvas.selectionBounds())
  expect(b.x1 - b.x0).toBeCloseTo(400, 0)
  expect(errors).toEqual([])
})

test('colour swatch drag: the pad stays on screen, and channel keys do not hit app shortcuts', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 50}}, {p: {x: 200, y: 80}}])
  const tool = await page.evaluate(() => (window as any).__pw.ed.tool)
  const sw = page.locator('[data-paint="線"] .TqInputColorPad').first()
  const b = (await sw.boundingBox())!
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
  await page.mouse.down()
  for (let i = 1; i < 6; i++) { await page.mouse.move(b.x + b.width / 2 - i * 4, b.y + b.height / 2); await page.waitForTimeout(16) }
  const pad = page.locator('.TqTweakOverlay .pad')
  await expect(pad).toBeVisible()
  const r = (await pad.boundingBox())!
  const vw = page.viewportSize()!
  expect(r.x).toBeGreaterThanOrEqual(0)
  expect(r.x + r.width).toBeLessThanOrEqual(vw.width)
  expect(r.y + r.height).toBeLessThanOrEqual(vw.height)
  // V / A switch the colour channel here, not the tool
  await page.keyboard.down('v'); await page.keyboard.up('v')
  await page.keyboard.down('a'); await page.keyboard.up('a')
  await page.mouse.up()
  expect(await page.evaluate(() => (window as any).__pw.ed.tool)).toBe(tool)
  expect(errors).toEqual([])
})
