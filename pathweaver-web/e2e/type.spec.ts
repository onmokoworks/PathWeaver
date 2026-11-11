import {test, expect, type Page} from '@playwright/test'
import {boot, click, doc, drag, pen, screen, state} from './helpers'

const strokes = async (page: Page) => (await doc(page)).strokes
const bounds = (s: any) => {
  const pts = (s.contours ?? []).flat()
  const xs = pts.map((p: any) => p.x), ys = pts.map((p: any) => p.y)
  return {x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys)}
}
async function typeAt(page: Page, p: {x: number; y: number}, text: string) {
  await page.keyboard.press('t')
  await click(page, p)
  await expect(page.getByTestId('text-editor')).toBeFocused()
  await page.keyboard.type(text)
  await page.keyboard.press('Escape')
}
/** Select paths as the layer list would (no anchors). */
const select = (page: Page, ids: number[]) => page.evaluate(ids => { const ed = (window as any).__pw.ed; ed.selected = ids; ed.anchors = [] }, ids)
const textStroke = async (page: Page) => (await strokes(page)).find((s: any) => s.text)
async function setField(page: Page, field: string, value: string) {
  const input = page.locator(`[data-field=${field}] input`).first()
  await input.fill(value)
  await input.press('Enter')
}
const roundTrip = (page: Page) => page.evaluate(() => {
  const pw = (window as any).__pw
  const text = pw.call('save', {doc: pw.ed.doc})
  return pw.call('load', {text})
})

test('type settings: alignment keeps the origin, tracking, baseline shift, kerning; one undo each; saved', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('t')
  await click(page, {x: 100, y: 100})
  await page.keyboard.type('AVA')
  await page.keyboard.press('Enter')
  await page.keyboard.type('A')
  await page.keyboard.press('Escape')
  await expect.poll(async () => (await textStroke(page))?.contours?.length ?? 0).toBeGreaterThan(2)
  let s = await textStroke(page)
  expect(s.text.text).toBe('AVA\nA')
  const x0 = s.text.matrix[4]
  const left = bounds(s)
  expect(left.x0).toBeGreaterThan(x0 - 4)
  await page.keyboard.press('v')
  await select(page, [s.id])
  await expect(page.getByTestId('text-props')).toBeVisible()
  const undo0 = (await state(page)).undo

  // right: the lines end at the origin, which stays put
  await page.locator('[data-field=text-align] li.list label').nth(2).click()
  await expect.poll(async () => (await textStroke(page)).text.align).toBe('right')
  s = await textStroke(page)
  expect(s.text.matrix[4]).toBe(x0)
  let b = bounds(s)
  expect(b.x1).toBeLessThan(x0 + 4)
  expect(b.x1).toBeGreaterThan(x0 - 12)
  expect((await state(page)).undo).toBe(undo0 + 1)
  // centre: around the origin
  await page.locator('[data-field=text-align] li.list label').nth(1).click()
  await expect.poll(async () => (await textStroke(page)).text.align).toBe('center')
  b = bounds(await textStroke(page))
  expect(Math.abs((b.x0 + b.x1) / 2 - x0)).toBeLessThan(6)
  await page.locator('[data-field=text-align] li.list label').nth(0).click()
  await expect.poll(async () => (await textStroke(page)).text.align ?? 'left').toBe('left')

  // tracking 500 (1/1000 em): 'AVA' gets two gaps of 24 wider
  const w0 = bounds(await textStroke(page)).x1 - bounds(await textStroke(page)).x0
  await setField(page, 'tracking', '500')
  await expect.poll(async () => (await textStroke(page)).text.tracking).toBe(500)
  b = bounds(await textStroke(page))
  expect(b.x1 - b.x0).toBeGreaterThan(w0 + 40)
  expect(b.x1 - b.x0).toBeLessThan(w0 + 56)

  // baseline shift raises the glyphs
  const y1 = b.y1
  await setField(page, 'baseline-shift', '10')
  await expect.poll(async () => bounds(await textStroke(page)).y1).toBeLessThan(y1 - 9)

  // kerning off: stored, outlines still there
  await page.locator('[data-field=kerning] .checkbox').click()
  await expect.poll(async () => (await textStroke(page)).text.kerning).toBe(false)
  expect((await textStroke(page)).contours.length).toBeGreaterThan(2)

  // undo walks back one setting at a time
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  await page.keyboard.press('Meta+z')
  await expect.poll(async () => (await textStroke(page)).text.kerning ?? true).toBe(true)
  await page.keyboard.press('Meta+z')
  expect((await textStroke(page)).text.baseline_shift ?? 0).toBe(0)
  await page.keyboard.press('Meta+Shift+z')
  await page.keyboard.press('Meta+Shift+z')

  const back = await roundTrip(page)
  const t = back.strokes.find((v: any) => v.text).text
  expect(t).toMatchObject({tracking: 500, baseline_shift: 10, kerning: false})
  expect(errors).toEqual([])
})

test('text on a path: typed on the line, follows the path, offset / flip, moves and duplicates with it, falls back when the path goes', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 200}}, {p: {x: 400, y: 200}}])
  const path = (await strokes(page))[0]
  await typeAt(page, {x: 100, y: 200}, 'Hello')
  await expect.poll(async () => (await textStroke(page))?.contours?.length ?? 0).toBeGreaterThan(3)
  let s = await textStroke(page)
  expect(s.text.on_path.stroke).toBe(path.id)
  expect(s.text.on_path.offset).toBeCloseTo(0.25, 1)
  let b = bounds(s)
  expect(b.x0).toBeGreaterThan(96)
  expect(b.y1).toBeLessThan(205)   // standing on the path
  expect(b.y0).toBeLessThan(180)
  const undoTyped = (await state(page)).undo

  // reshape the path (one undo step): the text follows
  await page.keyboard.press('a')
  await drag(page, {x: 400, y: 200}, {x: 400, y: -200})
  await expect.poll(async () => bounds(await textStroke(page)).y0).toBeLessThan(100)
  expect((await state(page)).undo).toBe(undoTyped + 1)
  await page.keyboard.press('Meta+z')
  await expect.poll(async () => bounds(await textStroke(page)).y0).toBeGreaterThan(150)

  // offset and flip in the inspector
  await page.keyboard.press('v')
  await select(page, [s.id])
  await setField(page, 'path-offset', '50')
  await expect.poll(async () => bounds(await textStroke(page)).x0).toBeGreaterThan(195)
  await page.locator('[data-field=path-flip] .checkbox').click()
  await expect.poll(async () => bounds(await textStroke(page)).y0).toBeGreaterThan(198) // under the path now
  b = bounds(await textStroke(page))
  expect(b.x1).toBeLessThan(204)   // running back from the anchor

  // moving the text moves its path
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
  const over = await screen(page, {x: 300, y: 400})
  await page.mouse.move(over.x, over.y)
  await page.keyboard.press('ArrowRight')
  let all = await strokes(page)
  expect(all.find((v: any) => v.id === path.id).nodes[0].p.x).toBeCloseTo(1)

  // duplicate: the copy sits on a copy of the path
  await page.keyboard.press('Meta+d')
  all = await strokes(page)
  expect(all).toHaveLength(4)
  const copy = all.find((v: any) => v.text && v.id !== s.id)
  const copyPath = all.find((v: any) => v.id === copy.text.on_path.stroke)
  expect(copyPath.id).not.toBe(path.id)
  expect(copyPath.nodes[0].p.x).toBeCloseTo(21)

  // delete the original path: its text falls back to point text where it showed
  const before = bounds((await strokes(page)).find((v: any) => v.id === s.id))
  await select(page, [path.id])
  await page.keyboard.press('Delete')
  s = (await strokes(page)).find((v: any) => v.id === s.id)
  expect(s.text.on_path ?? null).toBeNull()
  b = bounds(s)
  expect(Math.abs(b.x0 - before.x0)).toBeLessThan(20)
  expect(Math.abs(b.y0 - before.y0)).toBeLessThan(40)
  await page.keyboard.press('Meta+z')
  s = (await strokes(page)).find((v: any) => v.id === s.id)
  expect(s.text.on_path.stroke).toBe(path.id)

  // saved and loaded unchanged
  const back = await roundTrip(page)
  expect(back.strokes.find((v: any) => v.id === s.id).text.on_path).toEqual(s.text.on_path)
  expect(errors).toEqual([])
})

test('パスに沿わせる / パスから外す: one undo step each', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 300}}, {p: {x: 300, y: 300}}])
  await typeAt(page, {x: 50, y: 100}, 'Hi')
  await expect.poll(async () => (await textStroke(page))?.contours?.length ?? 0).toBeGreaterThan(1)
  const [path, text] = await strokes(page)
  expect(text.text.on_path ?? null).toBeNull()
  await page.keyboard.press('v')
  await select(page, [path.id, text.id])
  const undo0 = (await state(page)).undo
  await page.getByTestId('attach-path').click()
  await expect.poll(async () => (await textStroke(page)).text.on_path?.stroke).toBe(path.id)
  expect(bounds(await textStroke(page)).y1).toBeLessThan(305)
  expect(bounds(await textStroke(page)).y1).toBeGreaterThan(290)
  expect((await state(page)).undo).toBe(undo0 + 1)
  await page.getByTestId('detach-path').click()
  await expect.poll(async () => (await textStroke(page)).text.on_path ?? null).toBeNull()
  expect((await state(page)).undo).toBe(undo0 + 2)
  await page.keyboard.press('Meta+z')
  expect((await textStroke(page)).text.on_path.stroke).toBe(path.id)
  expect(errors).toEqual([])
})
