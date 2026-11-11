// Screenshots for docs/guide.html, with the place of every numbered part.
//
//   npx vite --port 5174 &      (the dev server)
//   node scripts/guide-shots.mjs
//
// Writes docs/img/<scene>.png and puts the regions (percent of each picture)
// into guide.html between <!-- regions:start --> and <!-- regions:end -->, so
// the page's numbered markers follow the real UI after any layout change.
import {webkit} from 'playwright'
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs'
import {join, dirname} from 'node:path'
import {fileURLToPath} from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '../..')
const out = join(root, 'docs/img')
const guide = join(root, 'docs/guide.html')
mkdirSync(out, {recursive: true})
const URL = process.env.PW_URL ?? 'http://127.0.0.1:5174/'

const browser = await webkit.launch()
const page = await browser.newPage({viewport: {width: 1400, height: 900}, deviceScaleFactor: 2, colorScheme: 'light'})
await page.goto(URL)
await page.evaluate(() => { localStorage.clear() })
await page.goto(URL)
await page.getByTestId('canvas').waitFor()
await page.evaluate(() => { const v = window.__pw.canvas.view; v.panX = 120; v.panY = 110; v.zoom = 1 })

const canvasBox = async () => page.getByTestId('canvas').boundingBox()
/** Page point of a world point. */
async function at(x, y) {
  const b = await canvasBox()
  const s = await page.evaluate(([x, y]) => window.__pw.canvas.toScreen({x, y}), [x, y])
  return {x: b.x + s.x, y: b.y + s.y}
}
async function dragTo(a, b, steps = 6) { await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, {steps}); await page.mouse.up() }
const box = async sel => { const l = page.locator(sel).first(); await l.waitFor(); return l.boundingBox() }
const around = (p, r = 10) => ({x: p.x - r, y: p.y - r, width: 2 * r, height: 2 * r})
const pad = (b, p) => ({x: b.x - p, y: b.y - p, width: b.width + 2 * p, height: b.height + 2 * p})
const union = (...bs) => {
  const x0 = Math.min(...bs.map(b => b.x)), y0 = Math.min(...bs.map(b => b.y))
  const x1 = Math.max(...bs.map(b => b.x + b.width)), y1 = Math.max(...bs.map(b => b.y + b.height))
  return {x: x0, y: y0, width: x1 - x0, height: y1 - y0}
}
const vp = {x: 0, y: 0, width: 1400, height: 900}
const clamp = c => {
  const x = Math.max(0, c.x), y = Math.max(0, c.y)
  return {x, y, width: Math.min(vp.width - x, c.width - (x - c.x)), height: Math.min(vp.height - y, c.height - (y - c.y))}
}

const scenes = {}
/** Take one picture of `clip`; regions are page boxes, stored as % of the clip. */
async function shoot(name, clip, regions) {
  clip = clamp(clip)
  await page.waitForTimeout(250)
  await page.mouse.move(2, 898) // no hover state in the picture
  await page.waitForTimeout(150)
  await page.screenshot({path: join(out, `${name}.png`), clip})
  const pct = (v, o, s) => +(((v - o) / s) * 100).toFixed(2)
  scenes[name] = {
    w: Math.round(clip.width), h: Math.round(clip.height),
    regions: regions.map(([key, b]) => ({key, x: pct(b.x, clip.x, clip.width), y: pct(b.y, clip.y, clip.height), w: +((b.width / clip.width) * 100).toFixed(2), h: +((b.height / clip.height) * 100).toFixed(2)})),
  }
  console.log(`${name}: ${regions.length} regions`)
}

// ---------- a drawing to show ----------
await page.keyboard.press('p')
for (const [x, y, dx] of [[40, 300, 0], [260, 120, 90], [520, 260, 0]]) {
  const a = await at(x, y)
  if (dx) await dragTo(a, {x: a.x + dx, y: a.y - 20})
  else await page.mouse.click(a.x, a.y)
}
await page.keyboard.press('Escape')
await page.keyboard.press('l')
await dragTo(await at(620, 120), await at(800, 300))
await page.evaluate(() => {
  const {ed} = window.__pw
  ed.edit(d => {
    const s = d.strokes[0]
    s.name = '波線'
    for (const w of [s.start_width, s.end_width]) { w.left = 6; w.right = 6 }
    d.strokes[1].name = '円'
  })
})
// an appearance on the curve: ラフ → 光彩
await page.evaluate(() => {
  const {ed} = window.__pw
  ed.selected = [ed.doc.strokes[0].id]
})
await page.getByTestId('look-new').click().catch(() => {})
await page.waitForTimeout(200)
await page.evaluate(() => {
  const {ed, call} = window.__pw
  ed.edit(d => {
    const s = d.strokes[0]
    let g = s.appearance
    if (!g) return
    g.nodes.push(call('nodeNew', {kind: 'Rough', id: 900, position: [220, 40]}))
    g.nodes.push(call('nodeNew', {kind: 'Glow', id: 901, position: [440, 40]}))
    g = call('graphConnect', {graph: g, from: g.nodes.find(n => n.kind === 'Input').id, to: 900, port: 0})
    g = call('graphConnect', {graph: g, from: 900, to: 901, port: 0})
    g = call('graphConnect', {graph: g, from: 901, to: g.output, port: 0})
    const out = g.nodes.find(n => n.id === g.output); if (out) out.position = [660, 40]
    const inp = g.nodes.find(n => n.kind === 'Input'); if (inp) inp.position = [0, 40]
    g.name = 'ラフな光'
    s.appearance = g
  })
})
await page.keyboard.press('v')
await page.waitForTimeout(800)

// ---------- 1. overview ----------
{
  const r = []
  r.push(['topbar', await box('[data-testid=topbar]')])
  r.push(['palette', await box('[data-testid=tool-palette]')])
  r.push(['canvas', pad(await box('[data-testid=canvas]'), -40)])
  r.push(['snap', await box('[data-testid=snap-bar]')])
  r.push(['zoom', union(await box('[data-testid=looks-toggle]'), await box('[data-testid=zoom-badge]'))])
  r.push(['layers', await box('[data-testid=path-list]')])
  r.push(['inspector', await box('[data-testid=inspector]')])
  r.push(['nodes', await box('[data-testid=node-editor]')])
  r.push(['status', await box('[data-testid=status]')])
  await shoot('overview', vp, r)
}

// ---------- 2. tools ----------
{
  const pal = await box('[data-testid=tool-palette]')
  const r = []
  for (const t of ['select', 'direct', 'pen', 'shape', 'convert', 'text', 'artboard']) r.push([t, await box(`[data-tool=${t}]`)])
  await shoot('tools', pad(pal, 24), r)
}

// ---------- 3. width handles (direct selection) ----------
{
  await page.keyboard.press('a')
  await page.evaluate(() => { const {ed} = window.__pw; ed.selected = [ed.doc.strokes[0].id] })
  await page.waitForTimeout(400)
  const b = await canvasBox()
  const pts = await page.evaluate(() => {
    const {ed, canvas} = window.__pw
    const s = ed.doc.strokes[0]
    const hs = canvas.widthHandles(s)
    const g = canvas.widthGrips(hs[0])
    const end = s.nodes.at(-1).p, mid = s.nodes[1], first = s.nodes[0].p
    // a point on the curve between the middle anchor and the end (the core)
    const b = s.nodes[2], t = 0.5, u = 1 - t
    const c1 = {x: mid.p.x + mid.outgoing.x, y: mid.p.y + mid.outgoing.y}, c2 = {x: b.p.x + b.incoming.x, y: b.p.y + b.incoming.y}
    const core = {x: u * u * u * mid.p.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.p.x, y: u * u * u * mid.p.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.p.y}
    return {
      grip: canvas.toScreen(g.right), core: canvas.toScreen(core), end: canvas.toScreen(end),
      anchor: canvas.toScreen(mid.p), handle: canvas.toScreen({x: mid.p.x + mid.outgoing.x, y: mid.p.y + mid.outgoing.y}), first: canvas.toScreen(first),
    }
  })
  const P = p => ({x: b.x + p.x, y: b.y + p.y})
  const r = [
    ['grip', around(P(pts.grip), 12)], ['core', around(P(pts.core), 10)], ['end', around(P(pts.end), 14)],
    ['anchor', around(P(pts.anchor), 9)], ['handle', around(P(pts.handle), 9)],
  ]
  const area = union(around(P(pts.first), 60), around(P(pts.end), 60), around(P(pts.anchor), 90))
  await shoot('width', area, r)
}

// ---------- 4. selection box and reference point ----------
{
  await page.keyboard.press('v')
  await page.evaluate(() => { const {ed} = window.__pw; ed.selected = [ed.doc.strokes[1].id]; window.__pw.canvas.setPivot({u: 0.5, v: 0.5}) })
  await page.waitForTimeout(300)
  const bb = await page.evaluate(() => window.__pw.canvas.selectionBounds())
  const tl = await at(bb.x0, bb.y0), br = await at(bb.x1, bb.y1)
  const r = [
    ['handle', around(br, 9)],
    ['rotate', around({x: br.x + 14, y: br.y + 14}, 10)],
    ['pivot', await box('[data-testid=pivot]')],
  ]
  await shoot('selection', pad(union(around(tl, 1), around(br, 1)), 44), r)

  // the inspector's position block
  const pick = page.getByTestId('pivot-picker')
  await pick.scrollIntoViewIfNeeded()
  await page.waitForTimeout(200)
  const head = await box('[data-testid=pivot-picker]')
  const pos = await box('[data-field=position]'), size = await box('[data-field=size]'), rot = await box('[data-field=rotate]')
  const ins = await box('[data-testid=inspector]')
  const clip = {x: ins.x, y: head.y - 16, width: ins.width, height: rot.y + rot.height - head.y + 32}
  await shoot('position', clip, [['picker', pad(head, 3)], ['position', pos], ['size', size], ['rotate', rot]])
}

// ---------- 5. node editor ----------
{
  await page.evaluate(() => { const {ed} = window.__pw; ed.selected = [ed.doc.strokes[0].id] })
  await page.waitForTimeout(500)
  const ne = await box('[data-testid=node-editor]')
  const r = [
    ['picker', await box('[data-testid=look-picker]')],
    ['add', await box('[data-testid=node-add]')],
    ['card', await box('[data-node="900"] .head')],
    ['bypass', await box('[data-node="900"] .bypass-btn')],
    ['param', await box('[data-node="900"] [data-param="Amount"]')],
    ['socket', pad(await box('[data-node="900"] .socket.out'), 4)],
    ['output', await box(`[data-node] .head >> nth=-1`)],
  ]
  await shoot('nodes', {x: ne.x, y: ne.y, width: ne.width, height: Math.min(ne.height, 300)}, r)
}

// ---------- 6. artboards ----------
{
  await page.keyboard.press('Shift+O')
  await dragTo(await at(-20, 60), await at(560, 360))
  await dragTo(await at(600, 60), await at(840, 360))
  await page.waitForTimeout(300)
  const id = await page.evaluate(() => window.__pw.canvas.state.artboard)
  const a = await page.evaluate(() => window.__pw.ed.doc.artboards.at(-1))
  const br = await at(a.x + a.w, a.y + a.h)
  const panel = await box('[data-testid=artboard-panel]')
  const r = [
    ['name', await box(`[data-artboard="${id}"] .name`)],
    ['frame', around(br, 9)],
    ['list', await (async () => { await page.getByTestId('tab-artboards').click(); return box('[data-testid=artboard-list]') })()],
    ['fields', union(await box('[data-field=artboard-name]'), await box('[data-field=artboard-preset]'))],
    ['add', await box('[data-testid=artboard-add]')],
  ]
  const cv = await canvasBox()
  await shoot('artboards', {x: cv.x, y: cv.y, width: 1400 - cv.x, height: Math.min(cv.height, panel.y + panel.height + 60 - cv.y)}, r)
  await page.keyboard.press('v')
}

// ---------- 7. export dialog ----------
{
  await page.keyboard.press('Meta+e')
  await page.getByTestId('export-dialog').waitFor()
  const d = await box('[data-testid=export-dialog]')
  const r = [
    ['target', await box('[data-field=export-target]')],
    ['boards', await box('[data-testid=export-dialog] .boards')],
    ['format', await box('[data-field=export-format]')],
    ['scale', await box('[data-field=export-scale]')],
    ['background', await box('[data-field=export-background]')],
    ['run', union(await box('[data-testid=export-summary]'), await box('[data-testid=export-run]'))],
  ]
  await shoot('export', pad(d, 12), r)
  await page.keyboard.press('Escape')
}

await browser.close()

const html = readFileSync(guide, 'utf8')
const json = JSON.stringify(scenes)
const next = html.replace(/<!-- regions:start -->[\s\S]*?<!-- regions:end -->/, `<!-- regions:start --><script type="application/json" id="regions">${json}</script><!-- regions:end -->`)
if (next === html && !html.includes(json)) throw new Error('guide.html has no regions block')
writeFileSync(guide, next)
console.log('guide.html regions updated')
