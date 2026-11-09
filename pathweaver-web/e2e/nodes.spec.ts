import {test, expect, type Page} from '@playwright/test'
import {boot, doc, pen, scrub, state} from './helpers'

async function setup(page: Page) {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.getByTestId('look-new').click()
  await page.waitForTimeout(200)
  return errors
}
const graph = async (page: Page) => (await doc(page)).strokes[0].appearance
async function addNode(page: Page, query: string, at: {x: number; y: number}) {
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + at.x, box.y + at.y)
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill(query)
  await page.keyboard.press('Enter')
  await page.keyboard.press('Enter') // place it where it is (it follows the pointer until then)
  await page.waitForTimeout(150)
}

test('add node with Shift+A search and edit its value directly on the node', async ({page}) => {
  const errors = await setup(page)
  await addNode(page, 'ラフ', {x: 300, y: 90})
  const g = await graph(page)
  const rough = g.nodes.find((n: any) => n.kind === 'Rough')
  expect(rough).toBeTruthy()
  const field = page.locator(`[data-node="${rough.id}"] [data-param="Amount"]`)
  await expect(field).toBeVisible()
  // click → type → Enter
  await field.click()
  await page.keyboard.press('Meta+a')
  await page.keyboard.type('42')
  await page.keyboard.press('Enter')
  await page.waitForTimeout(100)
  let n = (await graph(page)).nodes.find((v: any) => v.id === rough.id)
  expect(n.amount).toBe(42)
  const undoBefore = (await state(page)).undo
  // scrub: horizontal drag changes the value, as one undo step
  const ne = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.click(ne.x + ne.width - 30, ne.y + ne.height - 30) // blur the field
  await scrub(page, field, 80)
  await page.waitForTimeout(100)
  n = (await graph(page)).nodes.find((v: any) => v.id === rough.id)
  expect(n.amount).not.toBe(42)
  expect((await state(page)).undo).toBe(undoBefore + 1)
  await page.keyboard.press('Escape')
  await page.locator('[data-testid=canvas]').hover()
  await page.keyboard.press('Meta+z')
  n = (await graph(page)).nodes.find((v: any) => v.id === rough.id)
  expect(n.amount).toBe(42)
  expect(errors).toEqual([])
})

test('dropping a free node onto a link inserts it in between', async ({page}) => {
  await setup(page)
  let g = await graph(page)
  const input = g.nodes.find((n: any) => n.kind === 'Input')
  const output = g.nodes.find((n: any) => n.kind === 'Output')
  expect(output.inputs[0]).toBe(input.id)
  await addNode(page, 'zigzag', {x: 200, y: 200})
  g = await graph(page)
  const zig = g.nodes.find((n: any) => n.kind === 'Zigzag')
  // drag the node header to the middle of the Input→Output wire
  const head = page.locator(`[data-node="${zig.id}"] .head`)
  const hb = (await head.boundingBox())!
  const outSock = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const inSock = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  const mid = {x: (outSock.x + inSock.x) / 2 + 5, y: (outSock.y + inSock.y) / 2 + 5}
  await page.mouse.move(hb.x + 20, hb.y + hb.height / 2)
  await page.mouse.down()
  await page.mouse.move(mid.x, mid.y, {steps: 15})
  await expect(page.locator('.wire.insert')).toHaveCount(1)
  await page.mouse.up()
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === output.id).inputs[0]).toBe(zig.id)
  expect(g.nodes.find((n: any) => n.id === zig.id).inputs[0]).toBe(input.id)
  // the downstream node moved aside instead of being covered
  const z = (await page.locator(`[data-node="${zig.id}"]`).boundingBox())!
  const o = (await page.locator(`[data-node="${output.id}"]`).boundingBox())!
  expect(o.x).toBeGreaterThanOrEqual(z.x + z.width)
  await page.screenshot({path: 'test-results/nodes-inserted.png'})
})

test('connect by dragging between sockets, detach by dragging the input away', async ({page}) => {
  await setup(page)
  await addNode(page, '値', {x: 80, y: 90})
  await addNode(page, 'ラフ', {x: 420, y: 90})
  let g = await graph(page)
  const value = g.nodes.find((n: any) => n.kind === 'Value')
  const rough = g.nodes.find((n: any) => n.kind === 'Rough')
  const from = (await page.locator(`[data-socket="out:${value.id}"]`).boundingBox())!
  const to = (await page.locator(`[data-socket="param:${rough.id}:Amount"]`).boundingBox())!
  await page.mouse.move(from.x + 5, from.y + 5)
  await page.mouse.down()
  await page.mouse.move(to.x + 5, to.y + 5, {steps: 10})
  await page.mouse.up()
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === rough.id).bindings).toEqual([['Amount', value.id]])
  // bound parameter shows the effective value instead of an editable field
  await expect(page.locator(`[data-node="${rough.id}"] .linked-value`)).toContainText('10')
  // pull the link off the input and drop on empty space → disconnected
  await page.mouse.move(to.x + 5, to.y + 5)
  await page.mouse.down()
  await page.mouse.move(to.x + 5, to.y + 120, {steps: 10})
  await page.mouse.up()
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === rough.id).bindings).toEqual([])
})

test('mezzotint: add from the menu (Japanese or English), edit density on the node, switch to line grain', async ({page}) => {
  const errors = await setup(page)
  // thicker shape so grains are visible
  await page.evaluate(() => { const {ed} = (window as any).__pw; ed.edit(() => { const s = ed.doc.strokes[0]; for (const w of [s.start_width, s.end_width]) { w.left = 60; w.right = 60 } }) })
  await addNode(page, 'mezzo', {x: 200, y: 200})
  let g = await graph(page)
  const mz = g.nodes.find((n: any) => n.kind === 'Mezzotint')
  expect(mz).toBeTruthy()
  const input = g.nodes.find((n: any) => n.kind === 'Input'), output = g.nodes.find((n: any) => n.kind === 'Output')
  await page.evaluate(([i, m, o]) => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => { const s = ed.doc.strokes[0]; let gr = call('graphConnect', {graph: s.appearance, from: i, to: m, port: 0}); gr = call('graphConnect', {graph: gr, from: m, to: o, port: 0}); s.appearance = gr })
  }, [input.id, mz.id, output.id])
  // Effects render in a worker: wait until the canvas shows the current state.
  const hit = async () => {
    await expect.poll(() => page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renderPending === 0 && !ed.renders.get(ed.doc.strokes[0].id).stale })).toBe(true)
    return page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renders.get(ed.doc.strokes[0].id).layers.map((l: any) => l.d.length).reduce((a: number, b: number) => a + b, 0) })
  }
  const dots = await hit()
  expect(dots).toBeGreaterThan(1000)
  const field = page.locator(`[data-node="${mz.id}"] [data-param="Amount"]`)
  await field.click(); await page.keyboard.press('Meta+a'); await page.keyboard.type('80'); await page.keyboard.press('Enter')
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === mz.id).effect.amount).toBe(80)
  expect(await hit()).toBeGreaterThan(dots)
  await page.locator(`[data-node="${mz.id}"] [aria-label="粒の形"]`).click()
  const opt = page.locator('.Popover li.option', {hasText: '線'})
  await opt.hover()
  await opt.click()
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === mz.id).effect.grain).toBe('Lines')
  await page.getByTestId('canvas').screenshot({path: 'test-results/mezzotint.png'})
  expect(errors).toEqual([])
})

const kinds = async (page: Page) => (await graph(page)).nodes.map((n: any) => n.kind)
const byKind = async (page: Page, k: string) => (await graph(page)).nodes.find((n: any) => n.kind === k)

test('Blender add: the new node follows the pointer, highlights a link, click inserts; Esc cancels', async ({page}) => {
  const errors = await setup(page)
  const input = await byKind(page, 'Input'), output = await byKind(page, 'Output')
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + 300, box.y + 250)
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill('ラフ')
  await page.keyboard.press('Enter')
  // follows the pointer to the middle of the Input→Output link
  const o = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const i = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  await page.mouse.move((o.x + i.x) / 2 + 5, (o.y + i.y) / 2 + 5, {steps: 12})
  await expect(page.locator('.wire.insert')).toHaveCount(1)
  await page.mouse.down(); await page.mouse.up()
  const rough = await byKind(page, 'Rough')
  expect((await byKind(page, 'Output')).inputs[0]).toBe(rough.id)
  expect(rough.inputs[0]).toBe(input.id)
  // both new links are drawn (the bug: linked but invisible)
  await page.waitForTimeout(100)
  await expect(page.locator('.wire:not(.pending):not(.lazy)')).toHaveCount(2)
  // add another and cancel with Esc: nothing changes
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill('blur')
  await page.keyboard.press('Enter')
  await page.mouse.move(box.x + 500, box.y + 100, {steps: 4})
  await page.keyboard.press('Escape')
  expect(await kinds(page)).not.toContain('Blur')
  expect(errors).toEqual([])
})

test('Node Wrangler: Option-drag node→node connects the best sockets; Ctrl+right-drag cuts links; Ctrl+X keeps the chain', async ({page}) => {
  await setup(page)
  await addNode(page, '値', {x: 80, y: 90})
  await addNode(page, 'ラフ', {x: 420, y: 90})
  const value = await byKind(page, 'Value'), rough = await byKind(page, 'Rough')
  // Lazy Connect: number output → first free number input of the target
  const vb = (await page.locator(`[data-node="${value.id}"] .head`).boundingBox())!
  const rb = (await page.locator(`[data-node="${rough.id}"] .head`).boundingBox())!
  await page.keyboard.down('Alt')
  await page.mouse.move(vb.x + 40, vb.y + 10); await page.mouse.down()
  await page.mouse.move(rb.x + 40, rb.y + 10, {steps: 10}); await page.mouse.up()
  await page.keyboard.up('Alt')
  expect((await byKind(page, 'Rough')).bindings).toEqual([['Amount', value.id]])
  // Cut Links: Ctrl + right-drag across the Value→Rough link
  const from = (await page.locator(`[data-socket="out:${value.id}"]`).boundingBox())!
  const to = (await page.locator(`[data-socket="param:${rough.id}:Amount"]`).boundingBox())!
  const mx = (from.x + to.x) / 2, my = (from.y + to.y) / 2
  await page.keyboard.down('Control')
  await page.mouse.move(mx, my - 25); await page.mouse.down({button: 'right'})
  await page.mouse.move(mx, my + 35, {steps: 10}); await page.mouse.up({button: 'right'})
  await page.keyboard.up('Control')
  expect((await byKind(page, 'Rough')).bindings).toEqual([])
  await expect(page.getByTestId('add-menu')).toHaveCount(0) // a cut never opens the menu
  // Ctrl+X: dissolve a node inserted between Input and Output
  const input = await byKind(page, 'Input'), output = await byKind(page, 'Output')
  await page.evaluate(([i, r, o]) => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => { const s = ed.doc.strokes[0]; let g = call('graphConnect', {graph: s.appearance, from: i, to: r, port: 0}); g = call('graphConnect', {graph: g, from: r, to: o, port: 0}); s.appearance = g })
  }, [input.id, rough.id, output.id])
  await page.locator(`[data-node="${rough.id}"] .head`).click()
  await page.keyboard.press('Control+x')
  expect(await kinds(page)).not.toContain('Rough')
  expect((await byKind(page, 'Output')).inputs[0]).toBe(input.id)
})

test('node sets are named, saved and applied from the appearance picker', async ({page}) => {
  const errors = await setup(page)
  await addNode(page, 'ラフ', {x: 420, y: 200})
  await page.getByTestId('look-name').fill('ざらざら')
  await page.getByTestId('look-name').press('Enter')
  expect((await graph(page)).name).toBe('ざらざら')
  await page.getByTestId('look-browse').click()
  await page.getByRole('button', {name: 'この外観をセットに保存'}).click()
  expect((await doc(page)).appearance_presets.map((p: any) => p.name)).toEqual(['ざらざら'])
  // a second path takes a copy of the set (not linked)
  await pen(page, [{p: {x: 0, y: 250}}, {p: {x: 300, y: 250}}])
  await page.getByTestId('look-browse').click()
  await page.locator('[data-set="ざらざら"]').click()
  const d = await doc(page)
  expect(d.strokes[1].appearance.nodes.map((n: any) => n.kind)).toEqual(d.strokes[0].appearance.nodes.map((n: any) => n.kind))
  expect(d.strokes[1].appearance.shared ?? null).toBeNull()
  await page.getByTestId('look-browse').click()
  await page.getByRole('button', {name: 'セット「ざらざら」を削除'}).click()
  expect((await doc(page)).appearance_presets).toEqual([])
  expect((await doc(page)).strokes[1].appearance.name).toBe('ざらざら') // the path keeps its look
  expect(errors).toEqual([])
})

test('heavy effects render off the main thread: the UI keeps answering while mezzotint → blur computes', async ({page}) => {
  await setup(page)
  await page.evaluate(() => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => {
      const s = ed.doc.strokes[0]
      for (const w of [s.start_width, s.end_width]) { w.left = 80; w.right = 80 }
      const g = s.appearance
      g.nodes.push(call('nodeNew', {kind: 'Mezzotint', id: 10, position: [200, 0]}))
      g.nodes.push(call('nodeNew', {kind: 'Blur', id: 11, position: [400, 0]}))
      let gg = call('graphConnect', {graph: g, from: 1, to: 10, port: 0})
      gg = call('graphConnect', {graph: gg, from: 10, to: 11, port: 0})
      gg = call('graphConnect', {graph: gg, from: 11, to: 2, port: 0})
      s.appearance = gg
    })
  })
  // the main thread answers immediately (a heavy render would block this)
  const t0 = Date.now()
  await page.evaluate(() => (window as any).__pw.ed.renders.size)
  expect(Date.now() - t0).toBeLessThan(400)
  await expect.poll(() => page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renderPending === 0 && !ed.renders.get(ed.doc.strokes[0].id).stale }), {timeout: 15000}).toBe(true)
  const layers = await page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renders.get(ed.doc.strokes[0].id).layers })
  expect(layers).toHaveLength(1) // the blur arrived from the worker: one smooth bitmap, not stepped bands
  expect(layers[0].image).toBeTruthy()
})

test('F-09 drag an item out of the add menu onto a link: inserted on release', async ({page}) => {
  await setup(page)
  const input = await byKind(page, 'Input'), output = await byKind(page, 'Output')
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + 300, box.y + 120)
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill('ラフ')
  const item = (await page.locator('[data-testid=add-menu] [data-kind=Rough]').boundingBox())!
  const o = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const i = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  await page.mouse.move(item.x + 30, item.y + item.height / 2); await page.mouse.down()
  await page.mouse.move((o.x + i.x) / 2 + 5, (o.y + i.y) / 2 + 5, {steps: 15})
  await expect(page.locator('.wire.insert')).toHaveCount(1)
  await page.mouse.up()
  const rough = await byKind(page, 'Rough')
  expect((await byKind(page, 'Output')).inputs[0]).toBe(rough.id)
  expect(rough.inputs[0]).toBe(input.id)
})

test('F-10 fuzzy connect: drop a link on a node body or near a socket', async ({page}) => {
  await setup(page)
  await addNode(page, '値', {x: 420, y: 230})
  await addNode(page, 'ラフ', {x: 760, y: 120})
  const value = await byKind(page, 'Value'), rough = await byKind(page, 'Rough')
  // number output dropped anywhere on the Rough node → its first number input
  const from = (await page.locator(`[data-socket="out:${value.id}"]`).boundingBox())!
  const body = (await page.locator(`[data-node="${rough.id}"] .head .title`).boundingBox())!
  await page.mouse.move(from.x + 5, from.y + 5); await page.mouse.down()
  await page.mouse.move(body.x + 20, body.y + 6, {steps: 10}); await page.mouse.up()
  expect((await byKind(page, 'Rough')).bindings).toEqual([['Amount', value.id]])
  // shape output dropped 14px away from Rough's shape input → connects
  const input = await byKind(page, 'Input')
  const o = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const s = (await page.locator(`[data-socket="in:${rough.id}:0"]`).boundingBox())!
  await page.mouse.move(o.x + 5, o.y + 5); await page.mouse.down()
  await page.mouse.move(s.x + 5 - 14, s.y + 5 + 4, {steps: 10}); await page.mouse.up()
  expect((await byKind(page, 'Rough')).inputs[0]).toBe(input.id)
})

test('F-11 the add menu stays inside the editor when opened near the bottom', async ({page}) => {
  await setup(page)
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + box.width - 20, box.y + box.height - 10)
  await page.keyboard.press('Shift+A')
  const m = (await page.getByTestId('add-menu').boundingBox())!
  expect(m.y).toBeGreaterThanOrEqual(box.y)
  expect(m.y + m.height).toBeLessThanOrEqual(box.y + box.height + 1)
  expect(m.x + m.width).toBeLessThanOrEqual(box.x + box.width + 1)
})

test('F-12 drag a link into empty space: the add menu opens and the chosen node is connected', async ({page}) => {
  await setup(page)
  const input = await byKind(page, 'Input')
  const o = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  await page.mouse.move(o.x + 5, o.y + 5); await page.mouse.down()
  await page.mouse.move(o.x + 160, o.y - 60, {steps: 10}); await page.mouse.up()
  const menu = page.getByTestId('add-menu')
  await expect(menu).toBeVisible()
  await expect(menu).toContainText('つなげるノードだけ')
  await menu.locator('input').fill('ジグザグ')
  await page.keyboard.press('Enter')
  const zig = await byKind(page, 'Zigzag')
  expect(zig.inputs[0]).toBe(input.id)
})

test('a node whose box covers a link is inserted even if the pointer is off the wire; Ctrl-drag pulls it back out', async ({page}) => {
  await setup(page)
  let g = await graph(page)
  const input = g.nodes.find((n: any) => n.kind === 'Input')
  const output = g.nodes.find((n: any) => n.kind === 'Output')
  await addNode(page, 'zigzag', {x: 200, y: 240})
  g = await graph(page)
  const zig = g.nodes.find((n: any) => n.kind === 'Zigzag')
  const card = page.locator(`[data-node="${zig.id}"]`)
  const head = card.locator('.head')
  const hb = (await head.boundingBox())!
  const cb = (await card.boundingBox())!
  const outSock = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const inSock = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  const mid = {x: (outSock.x + inSock.x) / 2 + 5, y: (outSock.y + inSock.y) / 2 + 5}
  // grab the header but move so the wire runs under the lower part of the card,
  // far (> 14px) from the pointer
  const grabY = hb.y + hb.height / 2
  const target = {x: mid.x, y: mid.y - (cb.height - 20)}
  expect(mid.y - target.y).toBeGreaterThan(30)
  await page.mouse.move(hb.x + 20, grabY)
  await page.mouse.down()
  await page.mouse.move(target.x, target.y, {steps: 15})
  await expect(page.locator('.wire.insert')).toHaveCount(1)
  await page.mouse.up()
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === output.id).inputs[0]).toBe(zig.id)
  expect(g.nodes.find((n: any) => n.id === zig.id).inputs[0]).toBe(input.id)

  // Ctrl-drag the node away: unplugged, and the chain closes behind it
  const hb2 = (await head.boundingBox())!
  await page.keyboard.down('Control')
  await page.mouse.move(hb2.x + 20, hb2.y + hb2.height / 2)
  await page.mouse.down()
  await page.mouse.move(hb2.x + 20, hb2.y + 220, {steps: 10})
  await page.mouse.up()
  await page.keyboard.up('Control')
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === output.id).inputs[0]).toBe(input.id)
  expect(g.nodes.find((n: any) => n.id === zig.id).inputs[0]).toBe(null)
  // one undo step brings the insert back
  await page.locator('[data-testid=canvas]').hover()
  await page.keyboard.press('Meta+z')
  g = await graph(page)
  expect(g.nodes.find((n: any) => n.id === output.id).inputs[0]).toBe(zig.id)
})

test('glow: outer / inner / both, and halo only without the source shape', async ({page}) => {
  await setup(page)
  let g = await graph(page)
  const input = g.nodes.find((n: any) => n.kind === 'Input')
  const output = g.nodes.find((n: any) => n.kind === 'Output')
  await addNode(page, '光彩', {x: 200, y: 240})
  g = await graph(page)
  const glow = g.nodes.find((n: any) => n.kind === 'Glow')
  // wire it in through the API-free route: drag onto the link
  const head = page.locator(`[data-node="${glow.id}"] .head`)
  const hb = (await head.boundingBox())!
  const outSock = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const inSock = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  await page.mouse.move(hb.x + 20, hb.y + hb.height / 2)
  await page.mouse.down()
  await page.mouse.move((outSock.x + inSock.x) / 2 + 5, (outSock.y + inSock.y) / 2 + 5, {steps: 15})
  await page.mouse.up()
  const card = page.locator(`[data-node="${glow.id}"]`)
  await card.locator('[aria-label="光彩の向き"]').click()
  await page.waitForTimeout(600) // Tweeq: a release within 500ms of opening keeps the list open
  const opt = page.locator('.Popover li.option', {hasText: '両方'})
  await opt.hover()
  await opt.click()
  await page.waitForTimeout(100)
  let n = (await graph(page)).nodes.find((v: any) => v.id === glow.id)
  expect(n.both).toBe(true)
  await card.getByText('元の形を残す').click()
  await page.waitForTimeout(100)
  n = (await graph(page)).nodes.find((v: any) => v.id === glow.id)
  expect(n.keep_source).toBe(false)
  await expect(page.getByTestId('render-busy')).toHaveCount(0, {timeout: 10000})
  await page.screenshot({path: 'test-results/glow-modes.png'})
})

test('raster stage: rasterize at the node resolution, blur as pixels, trace back to shapes', async ({page}) => {
  const errors = await setup(page)
  let g = await graph(page)
  const input = g.nodes.find((n: any) => n.kind === 'Input')
  const output = g.nodes.find((n: any) => n.kind === 'Output')
  // Input → ラスターぼかし (inserted on the link via the add menu drag)
  const box = (await page.getByTestId('node-editor').boundingBox())!
  const outSock = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  const inSock = (await page.locator(`[data-socket="in:${output.id}:0"]`).boundingBox())!
  const mid = {x: (outSock.x + inSock.x) / 2 + 5, y: (outSock.y + inSock.y) / 2 + 5}
  await page.mouse.move(box.x + 300, box.y + 220)
  await page.keyboard.press('Shift+A')
  await expect(page.getByTestId('add-menu').getByText('ラスター', {exact: true})).toBeVisible()
  await page.getByTestId('add-menu').locator('input').fill('ラスターぼかし')
  const item = page.getByTestId('add-menu').locator('[data-kind="RasterBlur"]')
  const ib = (await item.boundingBox())!
  await page.mouse.move(ib.x + 20, ib.y + ib.height / 2)
  await page.mouse.down()
  await page.mouse.move(mid.x, mid.y, {steps: 20})
  await page.mouse.up()
  g = await graph(page)
  const blur = g.nodes.find((n: any) => n.kind === 'RasterBlur')
  expect(g.nodes.find((n: any) => n.id === output.id).inputs[0]).toBe(blur.id)
  await expect(page.getByTestId('render-busy')).toHaveCount(0, {timeout: 15000})
  // the canvas now draws a bitmap
  await expect(page.locator('[data-testid=canvas] image.raster')).toHaveCount(1)
  const href = await page.locator('[data-testid=canvas] image.raster').getAttribute('href')
  expect(href).toMatch(/^data:image\/png;base64,/)
  // SVG export embeds it
  const svg = await page.evaluate(() => (window as any).__pw.call('svg', {doc: (window as any).__pw.ed.doc}))
  expect(svg).toContain('<image')
  // add 画像トレース after the blur: back to vector shapes
  await page.evaluate(id => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => {
      const g = ed.active.appearance
      const t = call('nodeNew', {kind: 'Trace', id: 90, position: [700, 90]})
      t.inputs[0] = id
      g.nodes.push(t)
      g.nodes.find((n: any) => n.id === g.output).inputs[0] = 90
    })
  }, blur.id)
  await expect(page.getByTestId('render-busy')).toHaveCount(0, {timeout: 15000})
  await expect(page.locator('[data-testid=canvas] image.raster')).toHaveCount(0)
  const layers = await page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renders.get(ed.active.id).layers })
  expect(layers.length).toBe(1)
  expect(layers[0].d.length).toBeGreaterThan(20)
  await expect(page.locator('[data-node="90"] [data-param="Amount"]')).toBeVisible()
  await page.getByTestId('canvas').screenshot({path: 'test-results/raster-trace.png'})
  expect(errors).toEqual([])
})

test('画像 node: choose a picture (embedded as PNG), place it over the path, trace it; drop a file to add another', async ({page}) => {
  const errors = await setup(page)
  // a 120×60 test picture with a red disc, made by the browser
  const b64 = await page.evaluate(() => {
    const c = document.createElement('canvas'); c.width = 120; c.height = 60
    const x = c.getContext('2d')!
    x.fillStyle = '#d02020'; x.beginPath(); x.arc(60, 30, 25, 0, Math.PI * 2); x.fill()
    return c.toDataURL('image/png').split(',')[1]
  })
  await addNode(page, '画像', {x: 300, y: 220})
  let g = await graph(page)
  const img = g.nodes.find((n: any) => n.kind === 'Image')
  const card = page.locator(`[data-node="${img.id}"]`)
  await expect(card.getByText('画像ファイルをここへドロップ')).toBeVisible()
  const chooser = page.waitForEvent('filechooser')
  await card.getByRole('button', {name: '画像を選ぶ…'}).click()
  await (await chooser).setFiles({name: 'disc.png', mimeType: 'image/png', buffer: Buffer.from(b64, 'base64')})
  await expect(card.locator('img.thumb')).toBeVisible()
  g = await graph(page)
  const node = g.nodes.find((n: any) => n.id === img.id)
  expect(node.picture.name).toBe('disc.png')
  expect([node.picture.width, node.picture.height]).toEqual([120, 60])
  // placed over the path (drawn from x=0 to 300): left edge and width follow its bounds
  expect(node.translation[0]).toBeLessThanOrEqual(0)
  expect(node.amount).toBeGreaterThan(290)
  // wire 画像 → 画像トレース → 出力 and check the traced disc
  await page.evaluate(id => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => {
      const g = ed.active.appearance
      const t = call('nodeNew', {kind: 'Trace', id: 91, position: [600, 200]})
      t.inputs[0] = id
      t.value = 5
      g.nodes.push(t)
      g.nodes.find((n: any) => n.id === g.output).inputs[0] = 91
    })
  }, img.id)
  await expect(page.getByTestId('render-busy')).toHaveCount(0, {timeout: 15000})
  const layers = await page.evaluate(() => { const {ed} = (window as any).__pw; return ed.renders.get(ed.active.id).layers })
  expect(layers.length).toBe(1)
  expect(layers[0].color).toBe('#d02020')
  // the picture is saved inside the document
  const saved = await page.evaluate(() => (window as any).__pw.call('save', {doc: (window as any).__pw.ed.doc}))
  expect(String(saved)).toContain(b64.slice(0, 40))
  // the minimum-area setting exists on the trace node
  await expect(page.locator('[data-node="91"] [data-param="Value"]')).toBeVisible()

  // drop a file on the node editor: a new 画像 node appears with it
  const before = (await graph(page)).nodes.length
  const box = (await page.getByTestId('node-editor').boundingBox())!
  const dt = await page.evaluateHandle(b64 => {
    const bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0))
    const d = new DataTransfer()
    d.items.add(new File([bytes], 'dropped.png', {type: 'image/png'}))
    return d
  }, b64)
  const target = page.getByTestId('node-editor')
  const at = {clientX: box.x + 120, clientY: box.y + box.height - 60}
  await target.dispatchEvent('dragover', {dataTransfer: dt, ...at})
  await target.dispatchEvent('drop', {dataTransfer: dt, ...at})
  await expect.poll(async () => (await graph(page)).nodes.length).toBe(before + 1)
  g = await graph(page)
  expect(g.nodes.filter((n: any) => n.kind === 'Image').map((n: any) => n.picture?.name)).toContain('dropped.png')
  expect(errors).toEqual([])
})

test('H-05 node options step with ‹ › (wrapping) without opening the list', async ({page}) => {
  const errors = await setup(page)
  await addNode(page, '形の合成', {x: 300, y: 220})
  const g = await graph(page)
  const b = g.nodes.find((n: any) => n.kind === 'Boolean')
  const card = page.locator(`[data-node="${b.id}"]`)
  const op = async () => (await graph(page)).nodes.find((n: any) => n.id === b.id).operation
  expect(await op()).toBe('Union')
  await card.getByRole('button', {name: /次へ/}).click()
  expect(await op()).toBe('Difference')
  await card.getByRole('button', {name: /前へ/}).click()
  await card.getByRole('button', {name: /前へ/}).click()
  expect(await op()).toBe('Xor') // wrapped around
  await expect(page.locator('.Popover li.option')).toHaveCount(0)
  expect(errors).toEqual([])
})

test('add menu like Blender: pick the node first — header 追加 → category → type, then it follows the pointer', async ({page}) => {
  const errors = await setup(page)
  await page.getByTestId('node-add').click()
  const menu = page.getByTestId('add-menu')
  await expect(menu.locator('[data-kind]')).toHaveCount(0) // categories first, no long list
  await menu.locator('[data-category="shape"]').hover()
  const item = menu.locator('[data-kind="Zigzag"]')
  await expect(item).toBeVisible()
  await item.click()
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + 300, box.y + 250, {steps: 5})
  await page.mouse.down(); await page.mouse.up()
  expect((await graph(page)).nodes.some((n: any) => n.kind === 'Zigzag')).toBe(true)
  // keyboard: ↓ category, → into its types, Enter picks; Esc while held leaves nothing
  await page.mouse.move(box.x + 200, box.y + 200)
  await page.keyboard.press('Shift+A')
  await page.keyboard.press('ArrowDown'); await page.keyboard.press('ArrowDown') // 形
  await page.keyboard.press('ArrowRight'); await page.keyboard.press('Enter')    // ラフ
  await page.keyboard.press('Escape')
  expect((await graph(page)).nodes.some((n: any) => n.kind === 'Rough')).toBe(false)
  expect(errors).toEqual([])
})

test('a menu opened from a dragged link lists only nodes that can take that link', async ({page}) => {
  await setup(page)
  const input = await byKind(page, 'Input')
  const o = (await page.locator(`[data-socket="out:${input.id}"]`).boundingBox())!
  await page.mouse.move(o.x + 5, o.y + 5); await page.mouse.down()
  await page.mouse.move(o.x + 160, o.y - 60, {steps: 10}); await page.mouse.up()
  const menu = page.getByTestId('add-menu')
  await menu.locator('input').fill('値')
  await expect(menu.locator('[data-kind="Value"]')).toHaveCount(0) // a number node cannot take a shape
  await menu.locator('input').fill('')
  await menu.locator('[data-category="number"]').hover() // 数値: only 測る (it reads a shape)
  await expect(menu.locator('[data-kind]')).toHaveCount(1)
  await expect(menu.locator('[data-kind="Measure"]')).toHaveCount(1)
})

test('no start buttons: adding a node to a path without nodes makes Input → it → Output, one undo', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await expect(page.getByTestId('node-editor').locator('.empty button')).toHaveCount(0)
  await page.getByTestId('node-add').click()
  await page.getByTestId('add-menu').locator('input').fill('ラフ')
  await page.keyboard.press('Enter')
  const g = await graph(page)
  const input = g.nodes.find((n: any) => n.kind === 'Input'), output = g.nodes.find((n: any) => n.kind === 'Output')
  const rough = g.nodes.find((n: any) => n.kind === 'Rough')
  expect(rough.inputs[0]).toBe(input.id)
  expect(output.inputs[0]).toBe(rough.id)
  await page.keyboard.press('Meta+z')
  expect(await graph(page)).toBeFalsy()
  expect(errors).toEqual([])
})

test('appearances are data-blocks: path 2 picks path 1\'s, edits sync, the user count makes its own copy', async ({page}) => {
  const errors = await boot(page)
  const looks = async () => (await doc(page)).strokes.map((s: any) => s.appearance)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.getByTestId('look-new').click()
  await page.getByTestId('look-name').fill('ノード1')
  await page.getByTestId('look-name').press('Enter')
  await pen(page, [{p: {x: 0, y: 250}}, {p: {x: 300, y: 250}}])
  // path 2: pick path 1's appearance from the browse list
  await page.getByTestId('look-browse').click()
  await page.locator('[data-look="ノード1"]').click()
  let [a, b] = await looks()
  expect(b.shared).toBe(a.shared)
  expect(b.shared).not.toBeNull()
  await expect(page.getByTestId('look-users')).toHaveText('2')
  // an edit on path 2 shows on path 1 too
  const box = (await page.getByTestId('node-editor').boundingBox())!
  await page.mouse.move(box.x + 280, box.y + 220)
  await page.keyboard.press('Shift+A')
  await page.getByTestId('add-menu').locator('input').fill('ジグザグ')
  await page.keyboard.press('Enter'); await page.keyboard.press('Enter')
  ;[a, b] = await looks()
  expect(a.nodes.some((n: any) => n.kind === 'Zigzag')).toBe(true)
  expect(JSON.stringify(a)).toBe(JSON.stringify(b))
  // the user count: this path gets its own copy; further edits stay here
  await page.getByTestId('look-users').click()
  ;[a, b] = await looks()
  expect(b.shared ?? null).toBeNull()
  expect(a.shared ?? null).toBeNull() // one user left: no longer shared
  expect(b.name).not.toBe(a.name)
  await page.getByTestId('look-unlink').click()
  ;[a, b] = await looks()
  expect(b).toBeFalsy()
  expect(a.nodes.some((n: any) => n.kind === 'Zigzag')).toBe(true)
  expect(errors).toEqual([])
})

test('N-01 Ctrl+Shift+click a node makes it the final output; again puts the old link back', async ({page}) => {
  const errors = await setup(page)
  await page.evaluate(() => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => {
      const s = ed.doc.strokes[0]
      let g = s.appearance
      g.nodes.push(call('nodeNew', {kind: 'Rough', id: 10, position: [150, 220]}))
      g.nodes.push(call('nodeNew', {kind: 'Glow', id: 11, position: [480, 380]}))
      g = call('graphConnect', {graph: g, from: 1, to: 10, port: 0})
      g = call('graphConnect', {graph: g, from: 10, to: 11, port: 0})
      g = call('graphConnect', {graph: g, from: 11, to: 2, port: 0})
      s.appearance = g
    })
  })
  const g0 = await graph(page)
  const rough = g0.nodes.find((n: any) => n.kind === 'Rough'), glow = g0.nodes.find((n: any) => n.kind === 'Glow')
  const output = g0.nodes.find((n: any) => n.kind === 'Output')
  const before = output.inputs[0]
  const outSrc = async () => (await graph(page)).nodes.find((n: any) => n.kind === 'Output').inputs[0]
  const click = (id: number) => page.locator(`[data-node="${id}"] .head`).click({modifiers: ['Control', 'Shift'], position: {x: 40, y: 8}})
  expect(before).toBe(glow.id)
  await click(rough.id)
  expect(await outSrc()).toBe(rough.id)
  await expect(page.getByTestId('status')).toContainText('最終出力')
  // again on the viewed node: the original link (光彩) comes back
  await click(rough.id)
  expect(await outSrc()).toBe(before)
  // one undo per click
  await page.keyboard.press('Meta+z')
  expect(await outSrc()).toBe(rough.id)
  // a number node cannot be the output
  await page.evaluate(() => { const {ed, call} = (window as any).__pw; ed.edit(() => { ed.doc.strokes[0].appearance.nodes.push(call('nodeNew', {kind: 'Value', id: 12, position: [150, 520]})) }) })
  await click(12)
  expect(await outSrc()).toBe(rough.id)
  await expect(page.getByTestId('status')).toContainText('最終出力にできません')
  expect(errors).toEqual([])
})
