// Old UX audit items (UX-07…47) re-checked on the Vue + Tweeq front end.
import {test, expect} from '@playwright/test'
import {boot, click, doc, pen, state} from './helpers'

test('UX-28 the next path takes the look of the last selected path; nothing selected shows that style', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  // make the first path round-joined, round-capped and 40pt wide
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit(() => { const s = ed.active; s.join = 'Round'; s.start_cap = 'Round'; s.end_cap = 'Round'; s.edge = [200, 30, 30] })
    ed.coreEdit('lineWidth', {stroke: ed.active.id, value: 40})
  })
  await page.keyboard.press('Escape')
  await page.keyboard.press('Escape') // deselect: the inspector shows the next path's style
  await expect(page.getByTestId('draw-style')).toBeVisible()
  await expect(page.locator('[data-field="draw-width"] input')).toHaveValue(/40/)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 200, y: 100}}])
  const d = await doc(page)
  const s = d.strokes[1]
  expect(s.join).toBe('Round')
  expect(s.start_cap).toBe('Round')
  expect(s.edge).toEqual([200, 30, 30])
  expect(s.start_width.left + s.start_width.right).toBeCloseTo(40)
  expect(errors).toEqual([])
})

test('UX-35 opening another document drops a half-drawn path (no continuing into a path with the same id)', async ({page}) => {
  await boot(page)
  await page.keyboard.press('p')
  await click(page, {x: 0, y: 0})
  await page.waitForTimeout(400)
  await click(page, {x: 100, y: 0})
  await page.waitForTimeout(400)
  expect(await page.evaluate(() => (window as any).__pw.canvas.state.penPath)).not.toBeNull()
  await page.evaluate(() => (window as any).pathweaver.command('example'))
  await page.waitForTimeout(200)
  expect(await page.evaluate(() => (window as any).__pw.canvas.state.penPath)).toBeNull()
  const before = await doc(page)
  const counts = before.strokes.map((s: any) => s.nodes.length)
  await click(page, {x: 30, y: 300})
  await page.waitForTimeout(400)
  const after = await doc(page)
  // existing paths are untouched; the click started a new path instead
  expect(after.strokes.slice(0, counts.length).map((s: any) => s.nodes.length)).toEqual(counts)
  expect(after.strokes.length).toBe(counts.length + 1)
})

test('UX-12 file commands work from inside a text field and confirm it first; Tab moves between fields', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  await page.keyboard.press('Escape')
  await page.keyboard.press('v')
  await click(page, {x: 100, y: 0})
  const name = page.locator('[data-field="name"] input')
  await name.click()
  await page.keyboard.press('Meta+a')
  await page.keyboard.type('新しい名前')
  // capture the save request instead of opening a download
  const download = page.waitForEvent('download')
  await page.keyboard.press('Meta+s')
  const file = await download
  const text = await (await file.createReadStream())!.toArray().then(b => Buffer.concat(b).toString())
  expect(JSON.parse(text).strokes[0].name).toBe('新しい名前')
  expect((await doc(page)).strokes[0].name).toBe('新しい名前')
  // Tab: X → Y in the position fields
  const x = page.locator('[data-field="position"] input').first()
  await x.focus()
  const before = await page.evaluate(() => document.activeElement?.outerHTML)
  await page.keyboard.press('Tab')
  const after = await page.evaluate(() => document.activeElement?.outerHTML)
  expect(after).not.toBe(before)
  expect(await page.evaluate(() => document.activeElement?.tagName)).toBe('INPUT')
})

test('UX-20 delete buttons name their target; Delete key removes points when points are selected', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 100, y: 0}}, {p: {x: 200, y: 0}}])
  await page.keyboard.press('a')
  await click(page, {x: 100, y: 0})
  const insp = page.getByTestId('inspector')
  await expect(insp.getByRole('button', {name: /点を削除（1）/})).toBeVisible()
  await expect(insp.getByRole('button', {name: 'パスを削除'})).toBeVisible()
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Delete')
  let d = await doc(page)
  expect(d.strokes.length).toBe(1)
  expect(d.strokes[0].nodes.length).toBe(2)
  await insp.getByRole('button', {name: 'パスを削除'}).click()
  d = await doc(page)
  expect(d.strokes.length).toBe(0)
})

test('UX-11 mixed corner styles show no option; the change applies to every selected path', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 100, y: 50}}, {p: {x: 200, y: 0}}])
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 100, y: 150}}, {p: {x: 200, y: 100}}])
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit(() => { ed.doc.strokes[0].join = 'Round' })
    ed.selected = ed.doc.strokes.map((s: any) => s.id)
  })
  const radio = page.getByTestId('inspector').locator('[data-field="join"]')
  await expect(radio.locator('input:checked')).toHaveCount(0)
  await radio.getByText('面取り').first().click()
  const d = await doc(page)
  expect(d.strokes.map((s: any) => s.join)).toEqual(['Bevel', 'Bevel'])
})

test('UX-24 the node editor shows when it receives the keys', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  const ne = page.getByTestId('node-editor')
  await page.getByTestId('canvas').hover()
  await expect(ne).not.toHaveClass(/keys/)
  await ne.hover()
  await expect(ne).toHaveClass(/keys/)
  await page.getByTestId('canvas').hover()
  await expect(ne).not.toHaveClass(/keys/)
  expect((await state(page)).error).toBe('')
})

test('UX-22 integer parameters stay integers (seed); UX-43 long names are cut with an ellipsis and shown in full on hover', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  await page.getByTestId('look-new').click()
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + 300, box.y + 120)
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill('ラフ')
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter')
  const rough = await page.evaluate(() => (window as any).__pw.ed.active.appearance.nodes.find((n: any) => n.kind === 'Rough').id)
  const seed = page.locator(`[data-node="${rough}"] [data-param="Seed"]`)
  await seed.click()
  await page.keyboard.press('Meta+a')
  await page.keyboard.type('7.9')
  await page.keyboard.press('Enter')
  const v = await page.evaluate(id => (window as any).__pw.ed.active.appearance.nodes.find((n: any) => n.id === id).seed, rough)
  expect(Number.isInteger(v)).toBe(true)
  await expect(seed.locator('input')).toHaveValue(String(v))

  const long = 'とても長いパスの名前とても長いパスの名前とても長いパスの名前ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
  await page.evaluate(n => { const {ed} = (window as any).__pw; ed.edit(() => { ed.active.name = n }) }, long)
  for (const sel of ['[data-testid="path-list"] .name', '[data-testid="node-bar"] .target']) {
    const el = page.locator(sel).first()
    await expect(el).toHaveAttribute('title', new RegExp(long.slice(0, 10)))
    const cut = await el.evaluate(e => ({over: e.scrollWidth > e.clientWidth, ellipsis: getComputedStyle(e).textOverflow}))
    expect(cut).toEqual({over: true, ellipsis: 'ellipsis'})
  }
})

test('UX-19 clicking an already selected point makes it the active one without dropping the others', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 100, y: 0}}, {p: {x: 200, y: 0}}])
  await page.keyboard.press('a')
  await click(page, {x: 0, y: 0})
  await click(page, {x: 200, y: 0}, {shift: true})
  let s = await state(page)
  expect(s.anchors.length).toBe(2)
  const first = s.anchors[0]
  await click(page, {x: 0, y: 0})
  s = await state(page)
  expect(s.anchors.length).toBe(2)
  expect(s.anchors[1]).toEqual(first)
  await expect(page.locator('.anchor.active')).toHaveAttribute('data-anchor', `${first.stroke}:${first.node}`)
})

test('H-04 pathfinder: two overlapping paths → minus front, one undo restores both; divide makes pieces', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  await pen(page, [{p: {x: 100, y: -60}}, {p: {x: 100, y: 60}}])
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.selected = ed.doc.strokes.map((s: any) => s.id) })
  const pf = page.getByTestId('pathfinder')
  await expect(pf.locator('[data-op]')).toHaveCount(6)
  await pf.locator('[data-op="MinusFront"]').click()
  let d = await doc(page)
  expect(d.strokes.length).toBe(1)
  expect(d.strokes[0].contours.length).toBe(2) // the horizontal bar cut in two
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+z')
  d = await doc(page)
  expect(d.strokes.length).toBe(2)
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.selected = ed.doc.strokes.map((s: any) => s.id) })
  await pf.locator('[data-op="Divide"]').click()
  d = await doc(page)
  expect(d.strokes.length).toBe(5) // two bar halves, two post halves, the crossing
  expect((await state(page)).selected.length).toBe(5)
  expect(errors).toEqual([])
})

test('H-06 settings (⌘,): zoom speed, wheel, theme, paper, width handles in one dialog; remembered', async ({page}) => {
  const errors = await boot(page)
  await page.getByTestId('canvas').hover()
  await page.keyboard.press('Meta+,')
  const dlg = page.getByTestId('settings')
  await expect(dlg).toBeVisible()
  // wheel → zoom: a plain scroll now zooms the canvas
  await dlg.locator('[data-field="wheel"]').getByText('ズーム').first().click()
  const before = await page.evaluate(() => (window as any).__pw.canvas.view.zoom)
  await page.keyboard.press('Escape')
  await expect(dlg).toHaveCount(0)
  const c = (await page.getByTestId('canvas').boundingBox())!
  await page.mouse.move(c.x + 400, c.y + 300)
  await page.mouse.wheel(0, -100)
  await expect.poll(() => page.evaluate(() => (window as any).__pw.canvas.view.zoom)).toBeGreaterThan(before)
  // paper and theme from the same dialog; the command from the menu opens it too
  await page.evaluate(() => (window as any).pathweaver.command('settings'))
  await dlg.locator('[data-field="paper"]').getByText('黒').first().click()
  await expect(page.getByTestId('canvas')).toHaveAttribute('data-paper', 'black')
  await dlg.locator('[data-field="theme"]').getByText('ダーク').first().click()
  await expect(page.locator('body')).toHaveAttribute('data-color-mode', 'dark')
  await page.reload()
  await page.getByTestId('canvas').waitFor()
  await expect(page.getByTestId('canvas')).toHaveAttribute('data-paper', 'black')
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('pw.settings')!).wheel)).toBe('zoom')
  expect(errors).toEqual([])
})

test('H-07 panels float anywhere and dock back; the canvas takes the freed space', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}])
  const canvasWidth = async () => (await page.getByTestId('canvas').boundingBox())!.width
  const canvasHeight = async () => (await page.getByTestId('canvas').boundingBox())!.height
  const w0 = await canvasWidth(), h0 = await canvasHeight()
  // layers and properties float → the right column folds away
  await page.locator('[data-pin="layers"]').click()
  await page.locator('[data-pin="props"]').click()
  await expect(page.locator('[data-floating="layers"]')).toBeVisible()
  await expect(page.locator('[data-floating="props"] [data-testid=inspector]')).toBeVisible()
  expect(await canvasWidth()).toBeGreaterThan(w0 + 200)
  // drag the layers panel by its bar
  const bar = page.locator('[data-floating="layers"] [data-testid=floating-bar]')
  const b = (await bar.boundingBox())!
  await page.mouse.move(b.x + 40, b.y + 10)
  await page.mouse.down()
  await page.mouse.move(b.x + 240, b.y + 110, {steps: 8})
  await page.mouse.up()
  const moved = (await page.locator('[data-floating="layers"]').boundingBox())!
  expect(Math.round(moved.x - (b.x - 1))).toBeGreaterThan(150)
  // the floating layers list still works
  await expect(page.locator('[data-floating="layers"] [data-path]')).toHaveCount(1)
  // nodes float → the canvas gets the full height
  await page.locator('[data-pin="nodes"]').click()
  await expect(page.locator('[data-floating="nodes"] [data-testid=node-editor]')).toBeVisible()
  expect(await canvasHeight()).toBeGreaterThan(h0 + 200)
  // remembered across reloads
  await page.reload()
  await page.getByTestId('canvas').waitFor()
  await expect(page.locator('[data-floating="layers"]')).toBeVisible()
  // dock everything back
  for (const id of ['layers', 'props', 'nodes']) await page.locator(`[data-floating="${id}"] .dock:not([data-fold])`).click()
  await expect(page.locator('[data-floating]')).toHaveCount(0)
  expect(Math.abs(await canvasWidth() - w0)).toBeLessThan(2)
  expect(errors).toEqual([])
})
