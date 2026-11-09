import {test, expect} from '@playwright/test'
import {boot, pen} from './helpers'

// The Mac app's pointer lock (WKWebView has none): pretend to be the host and
// feed raw deltas like main.swift does. The value keeps changing past where
// the screen would end, and the lock is released on mouse up.
test('UX-08 numeric drag in the Mac app keeps going past the screen edge (native pointer lock)', async ({page}) => {
  await page.addInitScript(() => {
    const sent: string[] = []
    ;(window as any).__sent = sent
    ;(window as any).webkit = {messageHandlers: {pathweaver: {postMessage: (m: string) => sent.push(m)}}}
  })
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  const field = page.getByTestId('inspector').locator('.TqInputNumber').first()
  const before = await field.locator('input').inputValue()
  const b = (await field.boundingBox())!
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
  await page.mouse.down()
  await page.mouse.move(b.x + b.width / 2 + 10, b.y + b.height / 2, {steps: 3})
  await expect.poll(() => page.evaluate(() => (window as any).__sent.some((m: string) => m.includes('"pointerLock","value":true')))).toBe(true)
  // the cursor is frozen at the edge; the host keeps sending movement
  for (let i = 0; i < 40; i++) await page.evaluate(() => (window as any).__pwPointerDelta(25, 0))
  const during = await field.locator('input').inputValue()
  await page.mouse.up()
  expect(parseFloat(during)).toBeGreaterThan(parseFloat(before) + 20)
  await expect.poll(() => page.evaluate(() => (window as any).__sent.some((m: string) => m.includes('"pointerLock","value":false')))).toBe(true)
  expect(await page.evaluate(() => document.pointerLockElement)).toBeNull()
})
