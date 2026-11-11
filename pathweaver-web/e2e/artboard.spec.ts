import {test, expect, type Page, type Download} from '@playwright/test'
import {boot, doc, drag, state} from './helpers'

const boards = async (page: Page) => ((await doc(page)).artboards ?? []) as any[]
const bytes = async (d: Download) => Buffer.concat(await (await d.createReadStream())!.toArray())
/** Size and corner pixel of a picture, decoded in the page. */
const picture = (page: Page, b64: string, mime: string) => page.evaluate(async ([b64, mime]) => {
  const img = new Image()
  img.src = `data:${mime};base64,${b64}`
  await img.decode()
  const c = document.createElement('canvas')
  c.width = img.naturalWidth; c.height = img.naturalHeight
  const g = c.getContext('2d')!
  g.drawImage(img, 0, 0)
  return {w: img.naturalWidth, h: img.naturalHeight, corner: [...g.getImageData(1, 1, 1, 1).data], middle: [...g.getImageData(c.width / 2, c.height / 2, 1, 1).data]}
}, [b64, mime] as const)

test('artboard tool: drag makes one, drag moves it, handles resize, panel edits, Delete removes — each one undo', async ({page}) => {
  const errors = await boot(page)
  await page.keyboard.press('Shift+O')
  await expect(page.getByTestId('artboard-panel')).toBeVisible()
  await drag(page, {x: 0, y: 0}, {x: 200, y: 100})
  let [a] = await boards(page)
  expect([a.x, a.y, a.w, a.h].map(Math.round)).toEqual([0, 0, 200, 100])
  expect(a.name).toBe('アートボード 1')
  // the panel shows the drawn size (not the 1×1 it started from)
  await page.getByTestId('tab-artboards').click()
  await expect(page.locator(`[data-artboard-item="${a.id}"] .size`)).toHaveText('200×100')
  const undo = (await state(page)).undo
  // drag it by its inside
  await drag(page, {x: 100, y: 50}, {x: 130, y: 70})
  ;[a] = await boards(page)
  expect([a.x, a.y].map(Math.round)).toEqual([30, 20])
  // the bottom-right handle resizes
  await drag(page, {x: 230, y: 120}, {x: 330, y: 220})
  ;[a] = await boards(page)
  expect([a.w, a.h].map(Math.round)).toEqual([300, 200])
  expect((await state(page)).undo).toBe(undo + 2)
  // the panel: name and a preset size
  const name = page.locator('[data-field=artboard-name] input')
  await name.fill('表紙'); await name.press('Enter')
  ;[a] = await boards(page)
  expect(a.name).toBe('表紙')
  await expect(page.locator(`[data-artboard="${a.id}"] .name`)).toHaveText('表紙')
  // a click on empty canvas makes nothing
  await drag(page, {x: 600, y: 300}, {x: 600, y: 300})
  expect(await boards(page)).toHaveLength(1)
  // Delete removes it; undo brings it back
  await page.locator(`[data-artboard-item="${a.id}"]`).click()
  await page.locator('[data-testid=canvas]').hover()
  await page.keyboard.press('Delete')
  expect(await boards(page)).toHaveLength(0)
  await page.keyboard.press('Meta+z')
  expect(await boards(page)).toHaveLength(1)
  // saved with the file
  const saved = JSON.parse(await page.evaluate(() => (window as any).__pw.call('save', {doc: (window as any).__pw.ed.doc})))
  expect(saved.artboards[0].name).toBe('表紙')
  expect(errors).toEqual([])
})

test('export: PNG of an artboard at 2×, JPEG on white, SVG framed by the artboard, several artboards = several files', async ({page}) => {
  const errors = await boot(page)
  // a filled square in the middle of a 200×100 artboard
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit((d: any) => {
      const s = ed.newPath({x: 80, y: 30})
      s.nodes = [{x: 80, y: 30}, {x: 120, y: 30}, {x: 120, y: 70}, {x: 80, y: 70}].map((p, i) => ({id: ed.nextId(), p, incoming: {x: 0, y: 0}, outgoing: {x: 0, y: 0}, smooth: false}))
      s.closed = true; s.fill = [200, 30, 30]; s.edge = null
      if (!d.strokes.includes(s)) d.strokes.push(s)
      d.artboards = [{id: ed.nextId(), name: 'A', x: 0, y: 0, w: 200, h: 100}]
    })
    ed.selected = []
  })
  const open = async () => { await page.keyboard.press('Meta+e'); await expect(page.getByTestId('export-dialog')).toBeVisible() }
  const pick = (field: string, label: string) => page.locator(`[data-field=${field}]`).getByText(label, {exact: true}).first().click()

  await open()
  await pick('export-target', 'アートボード')
  await pick('export-format', 'PNG')
  await pick('export-scale', '2×')
  await pick('export-background', '透明')
  await expect(page.getByTestId('export-summary')).toHaveText('400×200 px')
  let dl = page.waitForEvent('download')
  await page.getByTestId('export-run').click()
  let file = await dl
  expect(file.suggestedFilename()).toBe('名称未設定.png')
  let pic = await picture(page, (await bytes(file)).toString('base64'), 'image/png')
  expect([pic.w, pic.h]).toEqual([400, 200])
  expect(pic.corner[3]).toBe(0)                   // transparent outside the art
  expect(pic.middle.slice(0, 3)).toEqual([200, 30, 30])

  // JPEG: always on white
  await open()
  await pick('export-format', 'JPEG')
  await pick('export-scale', '1×')
  dl = page.waitForEvent('download')
  await page.getByTestId('export-run').click()
  file = await dl
  expect(file.suggestedFilename()).toBe('名称未設定.jpg')
  pic = await picture(page, (await bytes(file)).toString('base64'), 'image/jpeg')
  expect([pic.w, pic.h]).toEqual([200, 100])
  expect(pic.corner.slice(0, 3).every(v => v > 245)).toBe(true)

  // SVG: framed by the artboard
  await open()
  await pick('export-format', 'SVG')
  dl = page.waitForEvent('download')
  await page.getByTestId('export-run').click()
  const svg = (await bytes(await dl)).toString()
  expect(svg).toContain('viewBox="0.0000 0.0000 200.0000 100.0000"')

  // two artboards: two files named after them
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.edit((d: any) => { d.artboards.push({id: ed.nextId(), name: 'B', x: 300, y: 0, w: 50, h: 50}) }) })
  await open()
  await pick('export-format', 'PNG')
  const names: string[] = []
  page.on('download', d => names.push(d.suggestedFilename()))
  await page.getByTestId('export-run').click()
  await expect.poll(() => names.length).toBe(2)
  expect(names.sort()).toEqual(['名称未設定-A.png', '名称未設定-B.png'])
  expect(errors).toEqual([])
})

test('artboards panel (layers area tab): numbered in order, click selects, double-click renames, drag reorders, ⤢ shows it, add and delete', async ({page}) => {
  const errors = await boot(page)
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit((d: any) => {
      d.artboards = [
        {id: ed.nextId(), name: '表紙', x: 0, y: 0, w: 200, h: 100},
        {id: ed.nextId(), name: '裏', x: 3000, y: 2000, w: 100, h: 100},
      ]
    })
  })
  await page.getByTestId('tab-artboards').click()
  const items = page.locator('[data-artboard-item]')
  await expect(items).toHaveCount(2)
  await expect(items.nth(0).locator('.num')).toHaveText('1')
  await expect(items.nth(1).locator('.name')).toHaveText('裏')
  const [a, b] = await boards(page)
  // click: active (no undo step)
  const undo = (await state(page)).undo
  await items.nth(1).click()
  expect(await page.evaluate(() => (window as any).__pw.canvas.state.artboard)).toBe(b.id)
  await expect(items.nth(1)).toHaveClass(/on/)
  expect((await state(page)).undo).toBe(undo)
  // ⤢: the artboard far away comes into view
  await items.nth(1).locator('.show').click()
  const inView = await page.evaluate(([x, y]) => { const c = (window as any).__pw.canvas; const s = c.toScreen({x, y}); return s.x > 0 && s.y > 0 && s.x < c.view.width && s.y < c.view.height }, [b.x + b.w / 2, b.y + b.h / 2])
  expect(inView).toBe(true)
  // double-click the name: rename
  await items.nth(0).locator('.name').dblclick()
  await page.getByTestId('artboard-rename').fill('カバー')
  await page.getByTestId('artboard-rename').press('Enter')
  expect((await boards(page))[0].name).toBe('カバー')
  // drag the second above the first: order (and numbers) change, one undo
  const before = (await state(page)).undo
  await items.nth(1).dragTo(items.nth(0), {targetPosition: {x: 20, y: 3}})
  expect((await boards(page)).map((v: any) => v.id)).toEqual([b.id, a.id])
  await expect(items.nth(0).locator('.name')).toHaveText('裏')
  expect((await state(page)).undo).toBe(before + 1)
  // add: next to the last one, same size; delete the active one
  await page.getByTestId('artboard-list-add').click()
  const list = await boards(page)
  expect(list).toHaveLength(3)
  expect([list[2].w, list[2].h]).toEqual([200, 100])
  await page.getByTestId('artboard-list-delete').click()
  expect(await boards(page)).toHaveLength(2)
  // back to layers
  await page.getByTestId('tab-layers').click()
  await expect(page.locator('[data-testid=path-list] ul')).toBeVisible()
  expect(errors).toEqual([])
})
