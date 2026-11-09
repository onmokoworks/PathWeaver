import {test, expect, type Page} from '@playwright/test'
import {boot, doc, pen} from './helpers'

async function threePaths(page: Page) {
  await pen(page, [{p: {x: 0, y: 50}}, {p: {x: 200, y: 50}}])
  await pen(page, [{p: {x: 0, y: 150}}, {p: {x: 200, y: 150}}])
  await pen(page, [{p: {x: 0, y: 250}}, {p: {x: 200, y: 250}}])
}
const ids = async (page: Page) => (await doc(page)).strokes.map((s: any) => s.id)
const row = (page: Page, id: number) => page.locator(`[data-path="${id}"]`)

test('layers: thumbnails, flat selection, folders (group, hide, rename, drag in/out), saved', async ({page}) => {
  const errors = await boot(page)
  await threePaths(page)
  const [a, b, c] = await ids(page)
  // every path row shows a thumbnail of its shape
  await expect(page.locator('[data-testid=path-list] .thumb svg path')).toHaveCount(3)
  // the selected row is a plain highlight (no rounded corners / side bar)
  await row(page, c).click()
  const style = await row(page, c).evaluate(e => { const s = getComputedStyle(e); return {radius: s.borderTopLeftRadius, shadow: s.boxShadow} })
  expect(style.radius).toBe('0px')
  expect(style.shadow).toBe('none')

  // group a and b with ⌘G
  await row(page, a).click()
  await row(page, b).click({modifiers: ['Shift']})
  await page.keyboard.press('Meta+g')
  let d = await doc(page)
  expect(d.folders).toHaveLength(1)
  const f = d.folders[0].id
  expect(d.strokes.filter((s: any) => s.folder === f).map((s: any) => s.id).sort()).toEqual([a, b].sort())
  await expect(page.locator(`[data-folder="${f}"]`)).toBeVisible()

  // rename the folder in place
  await page.locator(`[data-folder="${f}"] .name`).dblclick()
  await page.keyboard.press('Meta+a'); await page.keyboard.type('かな'); await page.keyboard.press('Enter')
  expect((await doc(page)).folders[0].name).toBe('かな')

  // hiding the folder hides its paths on the canvas (and only them)
  await page.locator(`[data-folder="${f}"] .eye`).click()
  await expect(page.locator(`[data-stroke="${a}"]`)).toHaveCount(0)
  await expect(page.locator(`[data-stroke="${b}"]`)).toHaveCount(0)
  await expect(page.locator(`[data-stroke="${c}"]`)).toHaveCount(1)
  await page.locator(`[data-folder="${f}"] .eye`).click()

  // drag path a out of the folder to the very top of the list
  const top = (await page.locator('[data-row]').first().boundingBox())!
  const ra = (await row(page, a).boundingBox())!
  await page.mouse.move(ra.x + 60, ra.y + ra.height / 2); await page.mouse.down()
  await page.mouse.move(ra.x + 60, top.y + 3, {steps: 10}); await page.mouse.up()
  d = await doc(page)
  expect(d.strokes.find((s: any) => s.id === a).folder ?? null).toBeNull()
  expect(d.strokes.at(-1).id).toBe(a) // front-most

  // drag path c onto the folder row: it goes inside
  const rc = (await row(page, c).boundingBox())!
  const rf = (await page.locator(`[data-folder="${f}"]`).boundingBox())!
  await page.mouse.move(rc.x + 60, rc.y + rc.height / 2); await page.mouse.down()
  await page.mouse.move(rf.x + 60, rf.y + rf.height / 2, {steps: 10}); await page.mouse.up()
  d = await doc(page)
  expect(d.strokes.find((s: any) => s.id === c).folder).toBe(f)
  // members stay contiguous in the stacking order
  const pos = d.strokes.map((s: any, i: number) => (s.folder === f ? i : -1)).filter((i: number) => i >= 0)
  expect(pos[pos.length - 1] - pos[0]).toBe(pos.length - 1)

  // folders survive save → load
  const reloaded = await page.evaluate(() => { const {ed, call} = (window as any).__pw; return call('load', {text: call('save', {doc: ed.doc})}) })
  expect(reloaded.folders).toEqual([{id: f, name: 'かな', visible: true, open: true}])
  expect(errors).toEqual([])
})

test('H-03 folders: an empty folder with nothing selected, folders inside folders, drag a folder in and out', async ({page}) => {
  const errors = await boot(page)
  await threePaths(page)
  const [a, b] = await ids(page)
  // nothing selected → ⌘G makes an empty folder, named in place
  await page.keyboard.press('Escape'); await page.keyboard.press('Escape')
  await page.evaluate(() => { (window as any).__pw.ed.selected = [] })
  await page.getByRole('button', {name: 'フォルダにまとめる'}).click()
  await page.keyboard.type('外側'); await page.keyboard.press('Enter')
  let d = await doc(page)
  expect(d.folders).toHaveLength(1)
  const outer = d.folders[0]
  expect(outer.name).toBe('外側')
  expect(d.strokes.every((s: any) => s.folder == null)).toBe(true)
  await expect(page.locator(`[data-folder="${outer.id}"] .count`)).toHaveText('0')

  // group a into its own folder, then drag that folder onto 外側 → nested
  await row(page, a).click()
  await page.keyboard.press('Meta+g')
  d = await doc(page)
  const inner = d.folders.find((f: any) => f.id !== outer.id)
  const src = (await page.locator(`[data-folder="${inner.id}"]`).boundingBox())!
  const dst = (await page.locator(`[data-folder="${outer.id}"]`).boundingBox())!
  await page.mouse.move(src.x + 60, src.y + src.height / 2)
  await page.mouse.down()
  await page.mouse.move(dst.x + 60, dst.y + dst.height / 2, {steps: 10})
  await page.mouse.up()
  d = await doc(page)
  expect(d.folders.find((f: any) => f.id === inner.id).parent).toBe(outer.id)
  // the path shows two levels deep, and 外側 counts it
  await expect(page.locator(`[data-folder="${outer.id}"] .count`)).toHaveText('1')
  const depth = await row(page, a).evaluate(e => parseFloat(getComputedStyle(e).paddingLeft))
  expect(depth).toBeGreaterThan(40)
  // hiding 外側 hides the nested path
  await page.locator(`[data-folder="${outer.id}"] .eye`).click()
  expect(await page.evaluate(id => (window as any).__pw.ed.shown((window as any).__pw.ed.findStroke(id)), a)).toBe(false)
  await page.locator(`[data-folder="${outer.id}"] .eye`).click()
  // a folder can't go inside itself: dropping 外側 onto 内側 does nothing
  const o2 = (await page.locator(`[data-folder="${outer.id}"]`).boundingBox())!
  const i2 = (await page.locator(`[data-folder="${inner.id}"]`).boundingBox())!
  await page.mouse.move(o2.x + 60, o2.y + o2.height / 2)
  await page.mouse.down()
  await page.mouse.move(i2.x + 60, i2.y + i2.height / 2, {steps: 10})
  await page.mouse.up()
  expect((await doc(page)).folders.find((f: any) => f.id === outer.id).parent ?? null).toBe(null)
  // removing 外側 moves 内側 up to the top level
  await page.locator(`[data-folder="${outer.id}"]`).click()
  await page.getByRole('button', {name: 'フォルダを解除'}).click()
  d = await doc(page)
  expect(d.folders.map((f: any) => f.id)).toEqual([inner.id])
  expect(d.folders[0].parent ?? null).toBe(null)
  expect(d.strokes.find((s: any) => s.id === a).folder).toBe(inner.id)
  expect(d.strokes.find((s: any) => s.id === b).folder ?? null).toBe(null)
  expect(errors).toEqual([])
})
