import {test, expect, type Page} from '@playwright/test'
import {boot, click, doc, drag, state} from './helpers'

const SVG = `<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200" viewBox="0 0 300 200">
  <style>.blue { fill: #0000ff }</style>
  <script>window.hacked = true</script>
  <rect id="box" class="blue" x="10" y="10" width="80" height="40" onload="window.hacked = true"/>
  <g transform="translate(150 100) scale(2)">
    <circle cx="0" cy="0" r="20" fill="none" stroke="rgb(255, 0, 0)" stroke-width="3"/>
  </g>
  <path id="ring" fill="#00aa00" fill-rule="evenodd" d="M200 10h80v80h-80zM220 30h40v40h-40z"/>
  <path id="open" fill="none" stroke="#000" stroke-width="4" stroke-linecap="round" d="M10 150 C 40 100, 80 100, 110 150"/>
  <defs><path id="hidden" d="M0 0h10v10z"/></defs>
  <text x="10" y="190" font-family="Helvetica" font-size="20" fill="#333">Hi</text>
</svg>`

async function placeFiles(page: Page, files: {name: string; mimeType: string; buffer: Buffer}[]) {
  const chooser = page.waitForEvent('filechooser')
  await page.keyboard.press('Meta+Shift+p')
  await (await chooser).setFiles(files)
}
async function pngBuffer(page: Page, w: number, h: number) {
  const url = await page.evaluate(([w, h]) => {
    const c = document.createElement('canvas')
    c.width = w; c.height = h
    const g = c.getContext('2d')!
    g.fillStyle = '#e66b5b'; g.fillRect(0, 0, w, h)
    g.fillStyle = '#5c88c5'; g.fillRect(0, 0, w / 2, h)
    return c.toDataURL('image/png')
  }, [w, h])
  return Buffer.from(url.split(',')[1], 'base64')
}
const strokes = async (page: Page) => (await doc(page)).strokes as any[]

test('place SVG: shapes become editable paths in a folder, nothing in the file runs, one undo', async ({page}) => {
  const errors = await boot(page)
  await placeFiles(page, [{name: 'art.svg', mimeType: 'image/svg+xml', buffer: Buffer.from(SVG)}])
  await expect.poll(async () => (await strokes(page)).length).toBe(6)
  const d = await doc(page)
  expect(await page.evaluate(() => (window as any).hacked)).toBeUndefined()
  expect(d.folders.map((f: any) => f.name)).toEqual(['art'])
  expect(d.strokes.every((s: any) => s.folder === d.folders[0].id)).toBe(true)
  const by = (n: string) => d.strokes.find((s: any) => s.name === n)
  // rect: 4 editable corners, CSS class fill, closed
  const box = by('box')
  expect(box.nodes).toHaveLength(4)
  expect(box.closed).toBe(true)
  expect(box.fill).toEqual([0, 0, 255])
  expect(box.edge).toBeNull()
  // circle under scale(2): stroke only, width scaled, smooth anchors, radius 40
  const circle = by('円')
  expect(circle.fill).toBeNull()
  expect(circle.edge).toEqual([255, 0, 0])
  expect(circle.start_width.left + circle.start_width.right).toBeCloseTo(6)
  expect(circle.nodes.length).toBe(4)
  expect(circle.nodes.every((n: any) => n.smooth)).toBe(true)
  const xs = circle.nodes.map((n: any) => n.p.x)
  expect(Math.max(...xs) - Math.min(...xs)).toBeCloseTo(80)
  // even-odd square ring keeps its hole: a compound path of two editable squares
  const ring = by('ring')
  const members = d.strokes.filter((s: any) => s.compound != null && s.compound === ring.compound)
  expect(members).toHaveLength(2)
  expect(members.every((s: any) => s.nodes.length === 4 && !s.contours && s.fill_rule === 'evenodd')).toBe(true)
  await expect(page.locator(`[data-path="${ring.id}"] [data-compound]`)).toHaveCount(1)
  // open curve: round caps, curve handles kept
  const open = by('open')
  expect(open.closed).toBe(false)
  expect(open.start_cap).toBe('Round')
  expect(Math.hypot(open.nodes[0].outgoing.x, open.nodes[0].outgoing.y)).toBeGreaterThan(10)
  // SVG text stays live text with outlines from the font
  const text = d.strokes.find((s: any) => s.text)
  expect(text.text.text).toBe('Hi')
  expect(text.contours.length).toBeGreaterThan(1)
  // the imported shapes are selected; the canvas draws them
  expect((await state(page)).selected).toHaveLength(6)
  await expect(page.locator(`[data-stroke="${box.id}"] path`)).toHaveCount(1)
  // all of it is one undo step
  await page.keyboard.press('Meta+z')
  expect(await strokes(page)).toHaveLength(0)
  expect(errors).toEqual([])
})

test('place image: drawn from the asset store, moves, exports; 下絵 is never picked or exported', async ({page}) => {
  const errors = await boot(page)
  await placeFiles(page, [{name: 'photo.png', mimeType: 'image/png', buffer: await pngBuffer(page, 60, 30)}])
  await expect.poll(async () => (await strokes(page)).length).toBe(1)
  let [s] = await strokes(page)
  expect(s.name).toBe('photo.png')
  expect(s.image.width).toBe(60)
  const d = await doc(page)
  expect(d.assets[s.image.asset]).toMatch(/^data:image\/png;base64,/)
  // drawn on the canvas with its placement
  const img = page.locator(`[data-placed="${s.id}"]`)
  await expect(img).toHaveAttribute('href', /^data:image\/png/)
  // drag it by the picture: the placement moves (and stays one undo step)
  const m0 = s.image.matrix
  const c = {x: m0[4] + 30, y: m0[5] + 15}
  await drag(page, c, {x: c.x + 50, y: c.y + 20})
  ;[s] = await strokes(page)
  expect(s.image.matrix[4]).toBeCloseTo(m0[4] + 50, 0)
  expect(s.image.matrix[5]).toBeCloseTo(m0[5] + 20, 0)
  // export keeps it as <image> with its transform
  let svg = await page.evaluate(() => (window as any).__pw.call('svg', {doc: (window as any).__pw.ed.doc}))
  expect(svg).toContain('<image')
  // duplicate shares the same pixels (stored once)
  await page.keyboard.press('Meta+d')
  let after = await doc(page)
  expect(after.strokes).toHaveLength(2)
  expect(Object.keys(after.assets)).toHaveLength(1)
  await page.keyboard.press('Meta+z')

  // make it a 下絵 in the inspector
  await click(page, {x: s.image.matrix[4] + 30, y: s.image.matrix[5] + 15})
  await page.locator('[data-field=reference] .checkbox').click()
  ;[s] = await strokes(page)
  expect(s.image.reference).toBe(true)
  await expect(page.locator(`[data-path="${s.id}"] [data-reference]`)).toBeVisible()
  svg = await page.evaluate(() => (window as any).__pw.call('svg', {doc: (window as any).__pw.ed.doc}))
  expect(svg).not.toContain('<image')
  // clicking it on the canvas selects nothing; ⌘A leaves it out
  await click(page, {x: s.image.matrix[4] + 30, y: s.image.matrix[5] + 15})
  expect((await state(page)).selected).toEqual([])
  await page.keyboard.press('Meta+a')
  expect((await state(page)).selected).toEqual([])
  // the layer list still selects it
  await page.locator(`[data-path="${s.id}"]`).click()
  expect((await state(page)).selected).toEqual([s.id])
  expect(errors).toEqual([])
})

test('text tool: click and type, retype by double-click, empty text leaves nothing, outline', async ({page}) => {
  const errors = await boot(page)
  // an empty text is removed without an undo step
  await page.keyboard.press('t')
  await click(page, {x: 100, y: 300})
  await expect(page.getByTestId('text-editor')).toBeFocused()
  await page.keyboard.press('Escape')
  expect(await strokes(page)).toHaveLength(0)
  expect((await state(page)).undo).toBe(0)

  await click(page, {x: 100, y: 100})
  await page.keyboard.type('Ab')
  await page.keyboard.press('Escape')
  await expect.poll(async () => (await strokes(page))[0]?.contours?.length ?? 0).toBeGreaterThan(1)
  let [s] = await strokes(page)
  expect(s.text.text).toBe('Ab')
  expect((await state(page)).undo).toBe(1)
  // the first baseline sits at the click: glyphs end near y = 100
  const pts = s.contours.flat()
  const ys = pts.map((p: any) => p.y), xs = pts.map((p: any) => p.x)
  expect(Math.max(...ys)).toBeGreaterThan(95)
  expect(Math.max(...ys)).toBeLessThan(110)
  expect(Math.min(...xs)).toBeGreaterThan(95)
  const width1 = Math.max(...xs) - Math.min(...xs)

  // inspector: bigger size relays out the outlines
  await page.keyboard.press('v')
  await click(page, {x: Math.min(...xs) + 5, y: Math.max(...ys) - 10})
  await expect(page.getByTestId('text-props')).toBeVisible()
  await page.evaluate(async () => {
    const pw = (window as any).__pw
    const st = pw.ed.findStroke(pw.ed.selected[0])
    pw.ed.begin(); st.text.size = 96; pw.ed.changed()
  })
  await page.evaluate(() => (window as any).__pw.relayoutActive())
  ;[s] = await strokes(page)
  const xs2 = s.contours.flat().map((p: any) => p.x)
  expect(Math.max(...xs2) - Math.min(...xs2)).toBeGreaterThan(width1 * 1.6)

  // double-click with the selection tool: type into it again
  // a point inside a glyph (picked like the canvas does)
  const p2 = await page.evaluate(() => {
    const pw = (window as any).__pw
    const st = pw.ed.doc.strokes[0]
    const pts = st.contours.flat()
    const x0 = Math.min(...pts.map((p: any) => p.x)), x1 = Math.max(...pts.map((p: any) => p.x))
    const y0 = Math.min(...pts.map((p: any) => p.y)), y1 = Math.max(...pts.map((p: any) => p.y))
    for (let y = y0; y < y1; y += 2) for (let x = x0; x < x1; x += 2) if (pw.canvas.pathAt({x, y}) === st.id) return {x, y}
    return null
  })
  expect(p2).not.toBeNull()
  await click(page, p2!, {count: 2})
  await expect(page.getByTestId('text-editor')).toBeFocused()
  await page.keyboard.press('End')
  await page.keyboard.type('c')
  await page.keyboard.press('Escape')
  await expect.poll(async () => (await strokes(page))[0].text.text).toBe('Abc')

  // moving keeps text and outlines together
  ;[s] = await strokes(page)
  const before = s.text.matrix[4]
  await page.keyboard.press('ArrowRight')
  ;[s] = await strokes(page)
  expect(s.text.matrix[4]).toBeCloseTo(before + 1)

  // ⇧⌘O: outlines become editable paths (one compound path)
  await page.keyboard.press('Meta+Shift+o')
  await expect.poll(async () => (await strokes(page)).length).toBeGreaterThan(1)
  const all = await strokes(page)
  expect(all.every((v: any) => !v.text && !v.contours && v.compound === all[0].compound && v.nodes.length > 1)).toBe(true)
  expect(errors).toEqual([])
})

test('paste an image from the clipboard places it', async ({page}) => {
  const errors = await boot(page)
  const png = (await pngBuffer(page, 20, 20)).toString('base64')
  await page.evaluate(b64 => {
    const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0))
    const dt = new DataTransfer()
    dt.items.add(new File([bytes], 'clip.png', {type: 'image/png'}))
    document.dispatchEvent(new ClipboardEvent('paste', {clipboardData: dt}))
  }, png)
  await expect.poll(async () => (await strokes(page)).length).toBe(1)
  expect((await strokes(page))[0].image.width).toBe(20)
  expect(errors).toEqual([])
})

test('抜き: black or white keyed out on the canvas and in the export, original kept, one undo each', async ({page}) => {
  const errors = await boot(page)
  // left half black, right half white
  const url = await page.evaluate(() => {
    const c = document.createElement('canvas')
    c.width = 40; c.height = 20
    const g = c.getContext('2d')!
    g.fillStyle = '#000'; g.fillRect(0, 0, 20, 20)
    g.fillStyle = '#fff'; g.fillRect(20, 0, 20, 20)
    return c.toDataURL('image/png')
  })
  await placeFiles(page, [{name: 'bw.png', mimeType: 'image/png', buffer: Buffer.from(url.split(',')[1], 'base64')}])
  await expect.poll(async () => (await strokes(page)).length).toBe(1)
  let [s] = await strokes(page)
  const original = (await doc(page)).assets[s.image.asset]
  const img = page.locator(`[data-placed="${s.id}"]`)
  await expect(img).toHaveAttribute('href', original)
  // alpha of the drawn picture at the black half and the white half
  const alphas = () => img.evaluate(async el => {
    const im = new Image()
    im.src = el.getAttribute('href')!
    await im.decode()
    const c = document.createElement('canvas')
    c.width = im.naturalWidth; c.height = im.naturalHeight
    const g = c.getContext('2d')!
    g.drawImage(im, 0, 0)
    return [g.getImageData(5, 10, 1, 1).data[3], g.getImageData(35, 10, 1, 1).data[3]]
  })

  const radio = (label: string) => page.locator('[data-field=knockout]').getByText(label, {exact: true}).first().click()
  await radio('黒')
  ;[s] = await strokes(page)
  expect(s.image.knockout).toBe('black')
  await expect(img).not.toHaveAttribute('href', original)
  expect(await alphas()).toEqual([0, 255])
  // the document still holds the untouched pixels
  expect((await doc(page)).assets[s.image.asset]).toBe(original)
  const svg = await page.evaluate(async () => {
    const pw = (window as any).__pw
    return pw.call('svg', {doc: {...pw.ed.doc, assets: await pw.exportAssets(pw.ed.doc)}})
  })
  expect(svg).toContain('<image')
  expect(svg).not.toContain(original)

  await radio('白')
  await expect.poll(alphas).toEqual([255, 0])
  await radio('なし')
  ;[s] = await strokes(page)
  expect(s.image.knockout).toBeUndefined()
  await expect(img).toHaveAttribute('href', original)
  // each switch is one undo step
  await page.keyboard.press('Meta+z')
  expect((await strokes(page))[0].image.knockout).toBe('white')
  expect(errors).toEqual([])
})
