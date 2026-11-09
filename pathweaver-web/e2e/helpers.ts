import type {Page} from '@playwright/test'
export type Pt = {x: number; y: number}
/** Page coordinates of a world point on the art canvas. */
export async function screen(page: Page, p: Pt): Promise<Pt> {
  const box = (await page.getByTestId('canvas').boundingBox())!
  const s = await page.evaluate(p => (window as any).__pw.canvas.toScreen(p), p)
  return {x: box.x + s.x, y: box.y + s.y}
}
export async function doc(page: Page) {
  return page.evaluate(() => JSON.parse(JSON.stringify((window as any).__pw.ed.doc)))
}
export async function state(page: Page) {
  return page.evaluate(() => {
    const ed = (window as any).__pw.ed
    return {selected: [...ed.selected], anchors: JSON.parse(JSON.stringify(ed.anchors)), undo: ed.history.undo, error: ed.error, nodes: [...ed.selectedNodes]}
  })
}
export async function boot(page: Page) {
  const errors: string[] = []
  page.on('pageerror', e => errors.push(e.message))
  page.on('console', m => {
    if (m.type() === 'error' || m.text().includes('[Vue warn]')) errors.push(m.text())
  })
  await page.goto('/')
  await page.getByTestId('canvas').waitFor()
  // Fixed view: world (0,0) at canvas (100,100), zoom 1.
  await page.evaluate(() => { const v = (window as any).__pw.canvas.view; v.panX = 100; v.panY = 100; v.zoom = 1 })
  return errors
}
export async function click(page: Page, p: Pt, opts: {alt?: boolean; shift?: boolean; count?: number} = {}) {
  const s = await screen(page, p)
  const mods: ('Alt' | 'Shift')[] = []
  if (opts.alt) mods.push('Alt')
  if (opts.shift) mods.push('Shift')
  for (const m of mods) await page.keyboard.down(m)
  await page.mouse.click(s.x, s.y, {clickCount: opts.count ?? 1})
  for (const m of mods) await page.keyboard.up(m)
}
export async function drag(page: Page, a: Pt, b: Pt, opts: {alt?: boolean; steps?: number} = {}) {
  const sa = await screen(page, a), sb = await screen(page, b)
  await page.mouse.move(sa.x, sa.y)
  await page.mouse.down()
  if (opts.alt) await page.keyboard.down('Alt')
  await page.mouse.move(sb.x, sb.y, {steps: opts.steps ?? 8})
  await page.mouse.up()
  if (opts.alt) await page.keyboard.up('Alt')
}
/** Draw an open path with the pen: list of points, each either click or drag-to handle. */
export async function pen(page: Page, pts: {p: Pt; drag?: Pt}[]) {
  await page.keyboard.press('p')
  for (const {p, drag: h} of pts) {
    if (h) await drag(page, p, h)
    else await click(page, p)
    await page.waitForTimeout(400) // avoid being read as a double click
  }
  await page.keyboard.press('Enter')
}
/** Scrub a Tweeq number field horizontally, one move per frame like a hand. */
export async function scrub(page: Page, el: {boundingBox(): Promise<{x: number; y: number; width: number; height: number} | null>}, dx: number) {
  const b = (await el.boundingBox())!
  const x = b.x + b.width / 2, y = b.y + b.height / 2
  await page.mouse.move(x, y)
  await page.mouse.down()
  for (let i = 1; i <= 12; i++) { await page.mouse.move(x + (dx * i) / 12, y); await page.waitForTimeout(16) }
  await page.mouse.up()
}

/** Put looks in the browser-side library (the Mac app keeps them as .pwn files). */
export async function withLibrary(page: Page, files: Record<string, string>) {
  const fs = await import('fs')
  const texts = Object.fromEntries(Object.entries(files).map(([name, path]) => [name, fs.readFileSync(path, 'utf8')]))
  await page.addInitScript(t => localStorage.setItem('pw.library', JSON.stringify(t)), texts)
}
