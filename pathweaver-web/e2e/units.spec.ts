import {test, expect, type Page} from '@playwright/test'
import {boot, doc, drag, state} from './helpers'

const MM = 72 / 25.4
const bounds = (page: Page) => page.evaluate(() => (window as any).__pw.canvas.selectionBounds())
async function rect(page: Page) {
  await page.evaluate(() => (window as any).__pw.canvas.setPivot({u: 0, v: 0}))
  await page.keyboard.press('m')
  await drag(page, {x: 100, y: 100}, {x: 300, y: 200})
  await page.keyboard.press('v')
}
async function typeIn(page: Page, field: string, index: number, value: string) {
  const input = page.locator(`[data-field=${field}] input`).nth(index)
  await input.fill(value)
  await input.press('Enter')
}
const value = async (page: Page, field: string, index = 0) => Number(await page.locator(`[data-field=${field}] input`).nth(index).inputValue())

test('units: the ruler corner switches the document unit; fields show and take it; stored numbers stay pt', async ({page}) => {
  const errors = await boot(page)
  await rect(page)
  const before = JSON.stringify((await doc(page)).strokes)
  const h0 = await bounds(page).then(b => b.y1 - b.y0)
  // pt by default: the fields read document units
  const w0 = await bounds(page).then(b => b.x1 - b.x0)
  expect(await value(page, "size", 0)).toBeCloseTo(w0, 1)
  await expect(page.getByTestId('unit-corner')).toHaveText('pt')

  // the corner → mm
  const undo = (await state(page)).undo
  await page.getByTestId('unit-corner').click()
  await page.getByText('mm（ミリ）').click()
  await expect(page.getByTestId('unit-corner')).toHaveText('mm')
  expect((await doc(page)).units).toBe('mm')
  expect((await state(page)).undo).toBe(undo + 1)
  // nothing stored moved; the fields now read mm
  expect(JSON.stringify((await doc(page)).strokes)).toBe(before)
  expect(await value(page, "size", 0)).toBeCloseTo(w0 / MM, 2)
  await expect(page.locator('[data-field=size]').first()).toContainText('mm')

  // a plain number is mm, a number with a unit is that unit, arithmetic mixes them
  await typeIn(page, 'position', 0, '10')
  expect((await bounds(page)).x0).toBeCloseTo(10 * MM, 1)
  await typeIn(page, 'position', 0, '1in')
  expect((await bounds(page)).x0).toBeCloseTo(72, 1)
  await typeIn(page, 'position', 1, '10mm + 2pt')
  expect((await bounds(page)).y0).toBeCloseTo(10 * MM + 2, 1)
  expect(await value(page, 'position', 1)).toBeCloseTo(10 + 2 / MM, 2)
  // the size keeps its ratio (linked) and takes cm
  await typeIn(page, 'size', 0, '5cm')
  const b = await bounds(page)
  expect(b.x1 - b.x0).toBeCloseTo(50 * MM, 1)
  expect((b.y1 - b.y0) / (b.x1 - b.x0)).toBeCloseTo(h0 / w0, 2)
  // line width too
  await typeIn(page, 'line-width', 0, '0.5mm')
  expect(await value(page, 'line-width')).toBeCloseTo(0.5, 2)

  // rulers count mm: at zoom 1 a tick every 50 mm, labelled in mm
  const labels = await page.locator('.ruler text').allTextContents()
  expect(labels).toContain('50')
  expect(labels).toContain('100')
  expect(labels).toContain('150') // never a label at the pt ruler's 100 pt steps

  // saved with the file; undo goes back to pt
  const saved = JSON.parse(await page.evaluate(() => (window as any).__pw.call('save', {doc: (window as any).__pw.ed.doc})))
  expect(saved.units).toBe('mm')
  await page.evaluate(n => { const ed = (window as any).__pw.ed; while (ed.history.undo > n) ed.undo() }, undo)
  await expect(page.getByTestId('unit-corner')).toHaveText('pt')
  expect(errors).toEqual([])
})

test('units: settings has this document’s unit and the default for new documents', async ({page}) => {
  const errors = await boot(page)
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+,')
  const dlg = page.getByTestId('settings')
  // the default: a document with no unit of its own follows it
  await dlg.locator('[data-field="default-unit"]').getByText('in', {exact: true}).first().click()
  await expect(page.getByTestId('unit-corner')).toHaveText('in')
  expect((await doc(page)).units).toBeUndefined()
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('pw.settings')!).unit)).toBe('in')
  // this document: stored in it, wins over the default
  await dlg.locator('[data-field="doc-unit"]').getByText('cm', {exact: true}).first().click()
  await expect(page.getByTestId('unit-corner')).toHaveText('cm')
  expect((await doc(page)).units).toBe('cm')
  await page.keyboard.press('Escape')
  // the next path's width reads cm and takes inches
  await typeIn(page, 'draw-width', 0, '0.1in')
  expect(await page.evaluate(() => (window as any).__pw.ed.drawStyle.width)).toBeCloseTo(7.2, 3)
  expect(await value(page, 'draw-width')).toBeCloseTo(0.254, 3)
  expect(errors).toEqual([])
})
