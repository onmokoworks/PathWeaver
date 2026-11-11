import {test, expect} from '@playwright/test'
import {boot, doc, pen} from './helpers'

test('autosave: interval in settings; unsaved changes leave a recovery copy that the next launch offers back; saving clears it', async ({page}) => {
  const errors = await boot(page)
  // settings: 1 minute
  await page.keyboard.press('Meta+,')
  await page.locator('[data-field=autosave]').getByText('1分', {exact: true}).first().click()
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('pw.settings')!).autosave)).toBe(1)
  await page.keyboard.press('Escape')

  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 50}}])
  await page.keyboard.press('Escape')
  await page.evaluate(() => (window as any).__pw.autosave())
  const kept = await page.evaluate(() => JSON.parse(localStorage.getItem('pw.recovery') ?? 'null'))
  expect(JSON.parse(kept.text).strokes).toHaveLength(1)
  await expect(page.getByTestId('autosaved')).toContainText('自動保存')

  // "crash": reload without saving → offered back, restored as unsaved
  // leaving with unsaved changes asks first (beforeunload); then the restore question
  let answer = true
  const asked: string[] = []
  page.on('dialog', d => {
    if (d.type() === 'beforeunload') return void d.accept()
    asked.push(d.message())
    void (answer ? d.accept() : d.dismiss())
  })
  await page.reload()
  await page.getByTestId('canvas').waitFor()
  await expect.poll(async () => (await doc(page)).strokes.length).toBe(1)
  expect(await page.evaluate(() => (window as any).__pw.ed.dirty)).toBe(true)
  expect(asked[0]).toContain('復元しますか')

  // saving clears the copy
  const dl = page.waitForEvent('download')
  await page.keyboard.press('Meta+s')
  await dl
  await expect.poll(() => page.evaluate(() => localStorage.getItem('pw.recovery'))).toBeNull()

  // declined: the copy is thrown away, the document stays empty
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 200, y: 150}}])
  await page.evaluate(() => (window as any).__pw.autosave())
  answer = false
  await page.reload()
  await page.getByTestId('canvas').waitFor()
  await expect.poll(() => page.evaluate(() => localStorage.getItem('pw.recovery'))).toBeNull()
  expect((await doc(page)).strokes).toHaveLength(0)
  expect(errors).toEqual([])
})
