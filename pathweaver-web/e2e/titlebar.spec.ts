import {test, expect} from '@playwright/test'
// Simulate the Mac host's message handler to check the title-bar contract.
test('top bar reports its empty area as a window drag region', async ({page}) => {
  await page.addInitScript(() => {
    const sent: unknown[] = []
    ;(window as any).__sent = sent
    ;(window as any).webkit = {messageHandlers: {pathweaver: {postMessage: (m: string) => sent.push(JSON.parse(m))}}}
  })
  await page.goto('/')
  const bar = page.getByTestId('topbar')
  const b = (await bar.boundingBox())!
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2)
  const regions = async () => page.evaluate(() => (window as any).__sent.filter((m: any) => m.type === 'dragRegion').map((m: any) => m.value))
  expect((await regions()).at(-1)).toBe(true)
  await bar.locator('.TqInputButton').last().hover()
  expect((await regions()).at(-1)).toBe(false)
  await page.mouse.move(b.x + b.width / 2, b.y + 300)
  expect((await regions()).at(-1)).toBe(false)
})

test('menu bar snap items: the page reports snap state and toggles from menu commands', async ({page}) => {
  await page.addInitScript(() => {
    const sent: unknown[] = []
    ;(window as any).__sent = sent
    ;(window as any).webkit = {messageHandlers: {pathweaver: {postMessage: (m: string) => sent.push(JSON.parse(m))}}}
  })
  await page.goto('/')
  await page.getByTestId('canvas').waitFor()
  const lastSnap = () => page.evaluate(() => (window as any).__sent.filter((m: any) => m.type === 'menu').at(-1).snap)
  expect(await lastSnap()).toEqual({snapGrid: false, snapGuides: true, snapTangent: false, snapNormal: false})
  await page.evaluate(() => (window as any).pathweaver.command('snapGrid'))
  await page.evaluate(() => (window as any).pathweaver.command('snapNormal'))
  await expect.poll(lastSnap).toEqual({snapGrid: true, snapGuides: true, snapTangent: false, snapNormal: true})
  expect(await page.evaluate(() => (window as any).__pw.ed.doc.snap_grid)).toBe(true)
})
