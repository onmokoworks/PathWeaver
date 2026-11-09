import {test, expect} from '@playwright/test'
test('boots without errors or blank icons', async ({page}) => {
  const errors: string[] = []
  page.on('pageerror', e => errors.push(e.message))
  page.on('console', m => { if (m.type() === 'error' || m.text().includes('[Vue warn]')) errors.push(m.text()) })
  await page.goto('/')
  await expect(page.getByTestId('canvas')).toBeVisible()
  await page.evaluate(() => (window as any).pathweaver.command('example'))
  await page.locator('[data-path]').first().click()
  await page.waitForTimeout(300)
  // Every icon must have drawn content (an empty <svg> means it never resolved).
  const blank = await page.$$eval('svg.iconify', els => els.filter(e => !e.innerHTML.trim()).length)
  expect(blank).toBe(0)
  await page.screenshot({path: 'test-results/shot-example.png'})
  expect(errors).toEqual([])
})

test('a panic inside the core is reported and editing continues', async ({page}) => {
  await page.goto('/')
  await page.getByTestId('canvas').waitFor()
  const r = await page.evaluate(() => {
    const {call, ed} = (window as any).__pw
    let message = ''
    try { call('debugPanic') } catch (e) { message = (e as Error).message }
    // the core still answers afterwards
    const doc = call('example')
    ed.load(doc, null, 'after-panic')
    return {message, strokes: ed.doc.strokes.length, hit: ed.renders.get(ed.doc.strokes[0].id).hit.length}
  })
  expect(r.message).toContain('内部エラー')
  expect(r.message).toContain('debugPanic requested')
  expect(r.strokes).toBe(1)
  expect(r.hit).toBeGreaterThan(100)
})
