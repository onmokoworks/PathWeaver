import {test, expect, type Page, type Download} from '@playwright/test'
import {boot, doc} from './helpers'

const bytes = async (d: Download) => Buffer.concat(await (await d.createReadStream())!.toArray())
const open = async (page: Page) => { await page.keyboard.press('Meta+e'); await expect(page.getByTestId('export-dialog')).toBeVisible() }
const pick = (page: Page, field: string, label: string) => page.locator(`[data-field=${field}]`).getByText(label, {exact: true}).first().click()
async function run(page: Page) {
  const dl = page.waitForEvent('download')
  await page.getByTestId('export-run').click()
  return dl
}

/** A folder「線画」with a uniform round-capped curve and a tapering one, on a 300×200 artboard. */
async function drawing(page: Page) {
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit((d: any) => {
      const folder = ed.nextId()
      d.folders = [...(d.folders ?? []), {id: folder, name: '線画', visible: true, open: true}]
      const path = (name: string, y: number) => {
        const s = ed.newPath({x: 20, y})
        s.name = name
        s.nodes = [
          {id: ed.nextId(), p: {x: 20, y}, incoming: {x: 0, y: 0}, outgoing: {x: 60, y: -60}, smooth: false},
          {id: ed.nextId(), p: {x: 260, y}, incoming: {x: -60, y: 60}, outgoing: {x: 0, y: 0}, smooth: false},
        ]
        s.closed = false; s.fill = null; s.edge = [20, 40, 160]; s.folder = folder
        s.start_width = {left: 3, right: 3, linked: true, angle: 0}
        s.end_width = {left: 3, right: 3, linked: true, angle: 0}
        s.widths = []
        if (!d.strokes.includes(s)) d.strokes.push(s)
        return s
      }
      const a = path('一定', 70)
      a.start_cap = 'Round'; a.end_cap = 'Round'
      const b = path('強弱', 150)
      b.end_width = {left: 12, right: 12, linked: true, angle: 0}
      d.artboards = [{id: ed.nextId(), name: 'A', x: 0, y: 0, w: 300, h: 200}]
    })
    ed.selected = []
  })
}

test('SVG export keeps folders, curves and line settings; it places back as the same line', async ({page}) => {
  const errors = await boot(page)
  await drawing(page)
  await open(page)
  await pick(page, 'export-target', 'アートボード')
  await pick(page, 'export-format', 'SVG')
  await pick(page, 'export-lines', '線のまま')
  await expect(page.getByTestId('export-summary')).toHaveText('300×200 pt')
  const svg = (await bytes(await run(page))).toString()
  // well-formed, with the folder as a group holding both paths
  const parsed = await page.evaluate(text => {
    const x = new DOMParser().parseFromString(text, 'image/svg+xml')
    if (x.querySelector('parsererror')) return null
    const g = x.querySelector('g[data-name="線画"]')
    const path = (name: string) => {
      const p = x.querySelector(`path[data-name="${name}"]`)
      return p && {d: p.getAttribute('d')!, stroke: p.getAttribute('stroke'), width: p.getAttribute('stroke-width'), cap: p.getAttribute('stroke-linecap'), fill: p.getAttribute('fill'), inFolder: !!g?.contains(p)}
    }
    return {label: g?.getAttribute('inkscape:label'), uniform: path('一定'), tapered: path('強弱')}
  }, svg)
  expect(parsed).not.toBeNull()
  expect(parsed!.label).toBe('線画')
  expect(parsed!.uniform).toEqual({d: 'M20 70 C80 10 200 130 260 70', stroke: '#1428a0', width: '6', cap: 'round', fill: 'none', inFolder: true})
  // the tapering line is its outline, as a handful of curves
  expect(parsed!.tapered!.stroke).toBeNull()
  expect(parsed!.tapered!.fill).toBe('#1428a0')
  expect(parsed!.tapered!.d.match(/[CL]/g)!.length).toBeLessThan(60)
  expect(parsed!.tapered!.inFolder).toBe(true)

  // placed back: the uniform line is a line again (two nodes, width 6, round caps)
  const before = (await doc(page)).strokes.length
  const chooser = page.waitForEvent('filechooser')
  await page.keyboard.press('Meta+Shift+p')
  await (await chooser).setFiles([{name: 'back.svg', mimeType: 'image/svg+xml', buffer: Buffer.from(svg)}])
  await expect.poll(async () => (await doc(page)).strokes.length).toBeGreaterThan(before)
  const back = (await doc(page)).strokes.slice(before)
  const line = back.find((s: any) => s.name === '一定')
  expect(line.nodes).toHaveLength(2)
  expect(line.closed).toBe(false)
  expect(line.start_width.left + line.start_width.right).toBeCloseTo(6)
  expect(line.start_cap).toBe('Round')
  expect(line.edge).toEqual([20, 40, 160])

  // 輪郭にする: no lines left, only filled shapes
  await open(page)
  await pick(page, 'export-lines', '輪郭にする')
  const outlined = (await bytes(await run(page))).toString()
  expect(outlined).not.toContain('stroke=')
  expect(outlined).toContain('data-name="一定"')
  expect(errors).toEqual([])
})

test('PDF export: one file, a page per artboard, pictures embedded', async ({page}) => {
  const errors = await boot(page)
  await drawing(page)
  // a GIF picture (redrawn as PNG for the PDF) and a second artboard
  await page.evaluate(() => {
    const {ed} = (window as any).__pw
    ed.edit((d: any) => {
      d.assets = {...(d.assets ?? {}), g1: 'data:image/gif;base64,R0lGODlhAQABAIAAAP8AAAAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw=='}
      const s = ed.newPath({x: 0, y: 0})
      s.name = '画像'
      s.closed = true; s.fill = null; s.edge = null; s.nodes = []
      s.image = {asset: 'g1', width: 1, height: 1, matrix: [40, 0, 0, 40, 10, 10], opacity: 0.5}
      s.contours = [[{x: 10, y: 10}, {x: 50, y: 10}, {x: 50, y: 50}, {x: 10, y: 50}]]
      if (!d.strokes.includes(s)) d.strokes.push(s)
      d.artboards.push({id: ed.nextId(), name: 'B', x: 400, y: 0, w: 100, h: 50})
    })
  })
  await open(page)
  await pick(page, 'export-target', 'アートボード')
  await pick(page, 'export-format', 'PDF')
  await pick(page, 'export-lines', '線のまま')
  await expect(page.getByTestId('export-summary')).toHaveText('1 ファイル・2 ページ')
  const file = await run(page)
  expect(file.suggestedFilename()).toBe('名称未設定.pdf')
  const pdf = (await bytes(file)).toString('latin1')
  expect(pdf.startsWith('%PDF-1.4')).toBe(true)
  expect(pdf).toContain('/Count 2')
  expect(pdf).toContain('/MediaBox [0 0 300 200]')
  expect(pdf).toContain('/MediaBox [0 0 100 50]')
  expect(pdf).toContain('/Subtype /Image')
  expect(pdf).toContain('/ca 0.5')
  expect(pdf.trimEnd().endsWith('%%EOF')).toBe(true)
  expect(errors).toEqual([])
})
