import {test, expect, type Page} from '@playwright/test'
import {boot, doc, pen, state} from './helpers'

const first = async (page: Page) => (await doc(page)).strokes[0]
async function triangleWithFill(page: Page) {
  await pen(page, [{p: {x: 0, y: 0}}, {p: {x: 200, y: 0}}, {p: {x: 100, y: 150}}])
  const insp = page.getByTestId('inspector')
  await insp.getByRole('button', {name: '塗りを追加'}).click()
  return insp
}

test('paint opacity: the colour field’s % sets the fill opacity in one undo step', async ({page}) => {
  const errors = await boot(page)
  const insp = await triangleWithFill(page)
  const undo = (await state(page)).undo
  const alpha = insp.locator('[data-paint="塗り"] .alpha input')
  await alpha.fill('40')
  await alpha.press('Enter')
  await expect.poll(async () => (await first(page)).fill_opacity).toBeCloseTo(0.4, 2)
  expect((await first(page)).fill).toEqual([32, 33, 36])
  expect((await state(page)).undo).toBe(undo + 1)
  const id = (await first(page)).id
  const path = page.locator(`[data-stroke="${id}"] path`).first()
  expect(Number(await path.getAttribute('fill-opacity'))).toBeCloseTo(0.4, 2)
  // back to 100%: the field disappears from the file
  await alpha.fill('100')
  await alpha.press('Enter')
  await expect.poll(async () => (await first(page)).fill_opacity).toBeUndefined()
  expect(errors).toEqual([])
})

test('gradient fill: type switch, stops on the bar, angle — each one undo step; drawn and exported', async ({page}) => {
  const errors = await boot(page)
  const insp = await triangleWithFill(page)
  const id = (await first(page)).id
  let undo = (await state(page)).undo
  await insp.locator('[data-field="fill-kind"] label', {hasText: '線形'}).click()
  await expect.poll(async () => (await first(page)).fill_gradient?.kind).toBe('Linear')
  expect((await state(page)).undo).toBe(++undo)
  const g = (await first(page)).fill_gradient
  expect(g.stops).toHaveLength(2)
  expect(g.stops[0].color).toEqual([32, 33, 36])
  // the canvas paints it with a gradient element of its own
  await expect(page.locator(`[data-stroke="${id}"] linearGradient stop`)).toHaveCount(2)
  expect(await page.locator(`[data-stroke="${id}"] path`).first().getAttribute('fill')).toMatch(/^url\(#pwg-/)

  // click the bar: a stop in the colour already there
  const bar = insp.locator('[data-gradient-bar="塗り"] .track')
  const b = (await bar.boundingBox())!
  await page.mouse.click(b.x + b.width * 0.5, b.y + b.height / 2)
  await expect.poll(async () => (await first(page)).fill_gradient.stops.length).toBe(3)
  expect((await state(page)).undo).toBe(++undo)
  const added = (await first(page)).fill_gradient.stops[2]
  expect(added.offset).toBeCloseTo(0.5, 1)
  expect(added.color[0]).toBeGreaterThan(100)

  // drag it to a quarter: one step
  const stop = insp.locator('[data-gradient-bar="塗り"] [data-stop="2"]')
  const s = (await stop.boundingBox())!
  await page.mouse.move(s.x + s.width / 2, s.y + s.height / 2)
  await page.mouse.down()
  for (let i = 1; i <= 8; i++) { await page.mouse.move(s.x + s.width / 2 - (b.width * 0.25 * i) / 8, s.y + s.height / 2); await page.waitForTimeout(16) }
  await page.mouse.up()
  await expect.poll(async () => (await first(page)).fill_gradient.stops[2].offset).toBeLessThan(0.3)
  expect((await state(page)).undo).toBe(++undo)

  // the selected stop's colour with opacity
  const stopAlpha = insp.locator('[data-field="fill-stop-color"] .alpha input')
  await stopAlpha.fill('25')
  await stopAlpha.press('Enter')
  await expect.poll(async () => (await first(page)).fill_gradient.stops[2].opacity).toBeCloseTo(0.25, 2)
  expect((await state(page)).undo).toBe(++undo)

  // angle
  const angle = insp.locator('[data-field="fill-angle"] input')
  await angle.fill('90')
  await angle.press('Enter')
  await expect.poll(async () => {
    const m = (await first(page)).fill_gradient.matrix
    return Math.round(Math.atan2(m[1], m[0]) * 180 / Math.PI)
  }).toBe(90)
  expect((await state(page)).undo).toBe(++undo)

  // exported SVG carries the gradient
  const svg = await page.evaluate(() => { const w = (window as any).__pw; return w.call('svg', {doc: {...w.ed.doc, assets: {}}}) as string })
  expect(svg).toContain('<linearGradient')
  expect(svg).toMatch(/fill="url\(#pw-gradient-1\)"/)

  // radial, then back to solid: the first stop's colour
  await insp.locator('[data-field="fill-kind"] label', {hasText: '円形'}).click()
  await expect(page.locator(`[data-stroke="${id}"] radialGradient`)).toHaveCount(1)
  await insp.locator('[data-field="fill-kind"] label', {hasText: '単色'}).click()
  await expect.poll(async () => (await first(page)).fill_gradient).toBeUndefined()
  expect((await first(page)).fill).toEqual([32, 33, 36])
  expect(errors).toEqual([])
})

test('gradient handles on the canvas (direct selection): drag the end, one undo step; moving the path carries the gradient', async ({page}) => {
  const errors = await boot(page)
  const insp = await triangleWithFill(page)
  await insp.locator('[data-field="fill-kind"] label', {hasText: '線形'}).click()
  await expect.poll(async () => (await first(page)).fill_gradient?.kind).toBe('Linear')
  // not shown with the selection tool (its box handles are there)
  await expect(page.locator('[data-gradient="fill"]')).toHaveCount(0)
  await page.keyboard.press('a')
  const end = page.locator('[data-gradient="fill"] [data-grip="end"]')
  await expect(end).toHaveCount(1)
  const undo = (await state(page)).undo
  const e = (await end.boundingBox())!
  await page.mouse.move(e.x + e.width / 2, e.y + e.height / 2)
  await page.mouse.down()
  for (let i = 1; i <= 8; i++) { await page.mouse.move(e.x + e.width / 2 - i * 5, e.y + e.height / 2 + i * 10); await page.waitForTimeout(16) }
  await page.mouse.up()
  const g = (await first(page)).fill_gradient
  // end = matrix · (1, 0): was (200, 75), now about (160, 155)
  expect(g.matrix[0] + g.matrix[4]).toBeCloseTo(160, -1)
  expect(g.matrix[1] + g.matrix[5]).toBeCloseTo(155, -1)
  expect((await state(page)).undo).toBe(undo + 1)
  expect((await first(page)).nodes.map((n: any) => n.p)).toEqual([{x: 0, y: 0}, {x: 200, y: 0}, {x: 100, y: 150}])
  // move the whole path through the core: the gradient goes along
  await page.evaluate(() => {
    const w = (window as any).__pw
    const r = w.call('edit', {doc: w.ed.doc, kind: 'transform', ids: w.ed.selected, matrix: [1, 0, 0, 1, 30, 0]})
    w.ed.edit((d: any) => { d.strokes = r.doc.strokes })
  })
  expect((await first(page)).fill_gradient.matrix[4]).toBeCloseTo(g.matrix[4] + 30, 3)
  expect(errors).toEqual([])
})

test('SVG import keeps gradients and opacities', async ({page}) => {
  const errors = await boot(page)
  const SVG = `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="200" height="100" viewBox="0 0 200 100">
    <defs>
      <linearGradient id="base"><stop offset="0" stop-color="#ff0000"/><stop offset="1" stop-color="#0000ff" stop-opacity="0.5"/></linearGradient>
      <linearGradient id="lg" xlink:href="#base" x1="0" y1="0" x2="1" y2="0"/>
      <radialGradient id="rg" gradientUnits="userSpaceOnUse" cx="150" cy="50" r="40"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient>
    </defs>
    <rect id="lin" x="0" y="0" width="100" height="100" fill="url(#lg)"/>
    <circle id="rad" cx="150" cy="50" r="40" fill="url(#rg)" stroke="rgba(0, 128, 0, 0.5)" stroke-width="2" opacity="0.5"/>
    <rect id="half" x="0" y="0" width="10" height="10" fill="#00ff00" fill-opacity="0.25"/>
  </svg>`
  const chooser = page.waitForEvent('filechooser')
  await page.keyboard.press('Meta+Shift+p')
  await (await chooser).setFiles([{name: 'g.svg', mimeType: 'image/svg+xml', buffer: Buffer.from(SVG)}])
  await expect.poll(async () => (await doc(page)).strokes.length).toBe(3)
  const d = await doc(page)
  const by = (n: string) => d.strokes.find((s: any) => s.name === n)
  const lin = by('lin')
  expect(lin.fill_gradient.kind).toBe('Linear')
  expect(lin.fill_gradient.stops.map((s: any) => [s.color, s.opacity])).toEqual([[[255, 0, 0], 1], [[0, 0, 255], 0.5]])
  // the rect spans the gradient: start and end at its left and right edges (after centring)
  const xs = lin.nodes.map((n: any) => n.p.x)
  const m = lin.fill_gradient.matrix
  expect(m[4]).toBeCloseTo(Math.min(...xs), 3)
  expect(m[0] + m[4]).toBeCloseTo(Math.max(...xs), 3)
  const rad = by('rad')
  expect(rad.fill_gradient.kind).toBe('Radial')
  expect(rad.fill_gradient.matrix[0]).toBeCloseTo(40, 3)
  // element opacity folds into the stops; stroke alpha × opacity
  expect(rad.fill_gradient.stops.map((s: any) => s.opacity)).toEqual([0.5, 0.5])
  expect(rad.edge).toEqual([0, 128, 0])
  expect(rad.edge_opacity).toBeCloseTo(0.25, 2)
  expect(by('half').fill_opacity).toBeCloseTo(0.25, 2)
  await expect(page.locator(`[data-stroke="${lin.id}"] linearGradient`)).toHaveCount(1)
  await expect(page.locator(`[data-stroke="${rad.id}"] radialGradient`)).toHaveCount(1)
  expect(errors).toEqual([])
})
