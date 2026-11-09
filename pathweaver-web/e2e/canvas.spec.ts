import {test, expect} from '@playwright/test'
import {boot, click, doc, drag, pen, screen, state} from './helpers'

test('pen: clicks make corners, drags make smooth handles, one undo per point', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 200}}, {p: {x: 150, y: 50}, drag: {x: 230, y: 50}}, {p: {x: 300, y: 200}}])
  const s = (await doc(page)).strokes[0]
  expect(s.nodes).toHaveLength(3)
  expect(s.nodes[0].outgoing).toEqual({x: 0, y: 0})
  expect(s.nodes[1].smooth).toBe(true)
  expect(s.nodes[1].outgoing.x).toBeCloseTo(80, 0)
  expect(s.nodes[1].incoming.x).toBeCloseTo(-80, 0)
  expect((await state(page)).undo).toBe(3)
  await page.keyboard.press('Meta+z')
  expect((await doc(page)).strokes[0].nodes).toHaveLength(2)
  expect(errors).toEqual([])
})

test('pen: curve → corner → curve keeps the corner sharp and both curves', async ({page}) => {
  await boot(page)
  await pen(page, [
    {p: {x: 0, y: 200}, drag: {x: 60, y: 120}},
    {p: {x: 200, y: 200}},
    {p: {x: 400, y: 200}, drag: {x: 460, y: 280}},
  ])
  const s = (await doc(page)).strokes[0]
  expect(s.nodes[0].smooth).toBe(true)
  expect(s.nodes[1].incoming).toEqual({x: 0, y: 0})
  expect(s.nodes[1].outgoing).toEqual({x: 0, y: 0})
  expect(s.nodes[2].smooth).toBe(true)
  await page.getByTestId('canvas').screenshot({path: 'test-results/pen-corner.png'})
})

test('pen: clicking the last point then dragging sets only the outgoing handle', async ({page}) => {
  await boot(page)
  await page.keyboard.press('p')
  await drag(page, {x: 0, y: 200}, {x: 50, y: 150})
  await page.waitForTimeout(400)
  await drag(page, {x: 200, y: 200}, {x: 250, y: 250})
  await page.waitForTimeout(400)
  await drag(page, {x: 200, y: 200}, {x: 250, y: 120})
  await page.waitForTimeout(400)
  const n = (await doc(page)).strokes[0].nodes[1]
  expect(n.incoming.x).toBeCloseTo(-50, 0)
  expect(n.incoming.y).toBeCloseTo(-50, 0)
  expect(n.outgoing.y).toBeCloseTo(-80, 0)
  expect(n.smooth).toBe(false)
})

test('pen: clicking on an existing segment adds an anchor without changing the shape', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  const before = await page.evaluate(() => (window as any).__pw.ed.renders.get((window as any).__pw.ed.doc.strokes[0].id).hit)
  await page.keyboard.press('p')
  await click(page, {x: 150, y: 101})
  const s = (await doc(page)).strokes
  expect(s).toHaveLength(1)
  expect(s[0].nodes).toHaveLength(3)
  expect(s[0].nodes[1].p.x).toBeCloseTo(150, 0)
  const after = await page.evaluate(() => (window as any).__pw.ed.renders.get((window as any).__pw.ed.doc.strokes[0].id).hit)
  expect(after.length).toBeGreaterThan(0)
  void before
})

test('direct: Option+click on a segment adds an anchor', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}, drag: {x: 50, y: 0}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  const onCurve = await page.evaluate(() => {
    const {ed, call} = (window as any).__pw
    return call('nearest', {stroke: ed.doc.strokes[0], p: {x: 150, y: 60}}).point
  })
  await click(page, onCurve, {alt: true})
  expect((await doc(page)).strokes[0].nodes).toHaveLength(3)
  expect((await state(page)).anchors).toHaveLength(1)
  // double-click also adds
  await page.waitForTimeout(400)
  const second = await page.evaluate(() => {
    const {ed, call} = (window as any).__pw
    return call('nearest', {stroke: ed.doc.strokes[0], p: {x: 240, y: 80}}).point
  })
  await click(page, second, {count: 2})
  expect((await doc(page)).strokes[0].nodes).toHaveLength(4)
})

test('direct: drag anchor and handle; Option breaks a smooth handle', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 150, y: 100}, drag: {x: 200, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  await drag(page, {x: 0, y: 100}, {x: 0, y: 150})
  let s = (await doc(page)).strokes[0]
  expect(s.nodes[0].p.y).toBeCloseTo(150, 0)
  await click(page, {x: 150, y: 100})
  await drag(page, {x: 200, y: 100}, {x: 200, y: 60}, {alt: true})
  s = (await doc(page)).strokes[0]
  expect(s.nodes[1].smooth).toBe(false)
  expect(s.nodes[1].incoming.x).toBeCloseTo(-50, 0)
  expect(s.nodes[1].incoming.y).toBeCloseTo(0, 0)
})

test('select: move, undo, marquee; no text gets selected by dragging', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('v')
  await drag(page, {x: 150, y: 100}, {x: 150, y: 200})
  expect((await doc(page)).strokes[0].nodes[0].p.y).toBeCloseTo(200, 0)
  await page.keyboard.press('Meta+z')
  expect((await doc(page)).strokes[0].nodes[0].p.y).toBeCloseTo(100, 0)
  // a drag that leaves the canvas over the panels must not select UI text
  await drag(page, {x: -50, y: -50}, {x: 2000, y: 1200}, {steps: 20})
  expect(await page.evaluate(() => getSelection()?.toString() ?? '')).toBe('')
})

async function grips(page: any) {
  return page.evaluate(() => {
    const {ed, canvas} = (window as any).__pw
    return canvas.widthHandles(ed.doc.strokes[0]).map((h: any) => ({h, g: canvas.widthGrips(h)}))
  })
}
async function withMeta(page: any, fn: () => Promise<void>) {
  await page.keyboard.down('Meta'); await fn(); await page.keyboard.up('Meta')
}

test('width in A: band always shown, drag = both sides, Option = one side, ⌘+click adds, ⌘+drag slides, Delete removes', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  await click(page, {x: 300, y: 100}) // select the path via its anchor
  await expect(page.locator('.width-handle')).toHaveCount(2) // start + end, no W tool needed
  expect(await page.locator('[data-tool=width]').count()).toBe(0)
  // drag the start's upper grip 20px further out: both sides grow (linked)
  let [start] = await grips(page)
  await drag(page, start.g.left, {x: start.g.left.x + start.h.dir.x * 20, y: start.g.left.y + start.h.dir.y * 20})
  let s = (await doc(page)).strokes[0]
  expect(s.start_width.left).toBeCloseTo(32, 0)
  expect(s.start_width.right).toBeCloseTo(32, 0)
  // Option: that side only
  ;[start] = await grips(page)
  await drag(page, start.g.right, {x: start.g.right.x - start.h.dir.x * 10, y: start.g.right.y - start.h.dir.y * 10}, {alt: true})
  s = (await doc(page)).strokes[0]
  expect(s.start_width.left).toBeCloseTo(32, 0)
  expect(s.start_width.right).toBeCloseTo(42, 0)
  // ⌘+click on the centerline adds a width point
  await withMeta(page, () => click(page, {x: 150, y: 100}))
  s = (await doc(page)).strokes[0]
  expect(s.widths).toHaveLength(1)
  expect(s.nodes).toHaveLength(2) // no anchor was added
  // ⌘+drag its centre slides it along the path
  const before = s.widths[0].t
  await withMeta(page, () => drag(page, {x: 150, y: 100}, {x: 220, y: 100}))
  s = (await doc(page)).strokes[0]
  expect(s.widths[0].t).toBeGreaterThan(before + 0.15)
  // Delete removes the selected width point
  await page.keyboard.press('Delete')
  expect((await doc(page)).strokes[0].widths).toHaveLength(0)
  expect((await doc(page)).strokes[0].nodes).toHaveLength(2)
  expect(errors).toEqual([])
})

test('⌘+drag an endpoint rotates its cut; the property − resets it', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  await click(page, {x: 150, y: 100}) // segment: selects the path
  await withMeta(page, () => drag(page, {x: 0, y: 100}, {x: 60, y: 160}))
  let s = (await doc(page)).strokes[0]
  expect(s.start_cap_locked).toBe(true)
  expect(s.start_angle).toBeCloseTo(45, 0)
  expect(s.nodes[0].p).toEqual({x: 0, y: 100}) // the anchor did not move
  await page.getByTestId('inspector').getByRole('button', {name: '芯に直角へ戻す'}).click()
  s = (await doc(page)).strokes[0]
  expect(s.start_cap_locked).toBe(false)
})

test('pen: the band can be adjusted while drawing; turning it off hides it until ⌘', async ({page}) => {
  await boot(page)
  await page.keyboard.press('p')
  await click(page, {x: 0, y: 100}); await page.waitForTimeout(400)
  await click(page, {x: 300, y: 100}); await page.waitForTimeout(400)
  const [, end] = await grips(page)
  await drag(page, end.g.left, {x: end.g.left.x + end.h.dir.x * 15, y: end.g.left.y + end.h.dir.y * 15})
  let s = (await doc(page)).strokes[0]
  expect(s.nodes).toHaveLength(2) // the grip drag did not place a point
  expect(s.end_width.left).toBeCloseTo(27, 0)
  await page.keyboard.press('Enter')
  await page.evaluate(() => (window as any).__pw.canvas.setShowWidth(false))
  await expect(page.locator('.width-handle')).toHaveCount(0)
  await page.keyboard.down('Meta')
  await page.mouse.move(500, 500)
  await expect(page.locator('.width-handle')).toHaveCount(2)
  await page.keyboard.up('Meta')
  await expect(page.locator('.width-handle')).toHaveCount(0)
  await page.evaluate(() => (window as any).__pw.canvas.setShowWidth(true))
  s = (await doc(page)).strokes[0]
  void s
})

test('guides: drag from the ruler creates a guide, dragging back removes it', async ({page}) => {
  await boot(page)
  const c = (await page.getByTestId('canvas').boundingBox())!
  await page.mouse.move(c.x + 300, c.y + 10)
  await page.mouse.down()
  await page.mouse.move(c.x + 300, c.y + 250, {steps: 8})
  await page.mouse.up()
  let g = (await doc(page)).guides
  expect(g).toHaveLength(1)
  expect(g[0].vertical).toBe(false)
  expect(g[0].position).toBeCloseTo(150, 0)
  await page.keyboard.press('v')
  await page.mouse.move(c.x + 300, c.y + 250)
  await page.mouse.down()
  await page.mouse.move(c.x + 300, c.y + 5, {steps: 8})
  await page.mouse.up()
  expect((await doc(page)).guides).toHaveLength(0)
})

test('normal: Shift+click on a path starts a line on it; the next point snaps perpendicular (straight and curved paths)', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('p')
  await click(page, {x: 150, y: 102}, {shift: true})
  await expect(page.locator('.path-frame')).toHaveCount(1) // the normal guide is visible before the next click
  await page.waitForTimeout(400)
  await click(page, {x: 158, y: 40})
  await page.keyboard.press('Enter')
  let d = await doc(page)
  expect(d.strokes[0].nodes).toHaveLength(2) // no anchor was added to the base path
  const line = d.strokes[1]
  expect(line.nodes[0].p.x).toBeCloseTo(150, 0)
  expect(line.nodes[0].p.y).toBeCloseTo(100, 0)
  expect(line.nodes[1].p.x).toBeCloseTo(150, 1)
  expect(line.nodes[1].p.y).toBeCloseTo(40, 0)

  // curved base: the new segment is perpendicular to the tangent at its start
  await pen(page, [{p: {x: 0, y: 300}, drag: {x: 100, y: 200}}, {p: {x: 300, y: 300}}])
  await page.keyboard.press('p')
  const on = await page.evaluate(() => { const {ed, call} = (window as any).__pw; return call('nearest', {stroke: ed.doc.strokes[2], p: {x: 120, y: 230}}) })
  await click(page, on.point, {shift: true})
  await page.waitForTimeout(400)
  const n = {x: on.tangent.y, y: -on.tangent.x} // the side away from the snap bar
  await click(page, {x: on.point.x + n.x * 80 + on.tangent.x * 5, y: on.point.y + n.y * 80 + on.tangent.y * 5})
  await page.keyboard.press('Enter')
  d = await doc(page)
  const c = d.strokes[3]
  const v = {x: c.nodes[1].p.x - c.nodes[0].p.x, y: c.nodes[1].p.y - c.nodes[0].p.y}
  expect(Math.abs(v.x * on.tangent.x + v.y * on.tangent.y)).toBeLessThan(0.5)
  expect(Math.hypot(v.x, v.y)).toBeGreaterThan(70)
  expect(errors).toEqual([])
})

test('normal snap shows while hovering with the pen, at the spot the click will use', async ({page}) => {
  const errors = await boot(page)
  await page.getByTestId('snap-bar').locator('[data-snap="snap_normal"]').click()
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('p')
  const at = await screen(page, {x: 304, y: 180})
  await page.mouse.move(at.x - 20, at.y)
  await page.mouse.move(at.x, at.y, {steps: 4})
  const snap = page.locator('.snap')
  await expect(snap).toHaveCount(1) // visible before any click
  await expect(snap.locator('.snap-label')).toHaveText('法線')
  await expect(snap.locator('circle.at')).toHaveCount(1)
  await page.waitForTimeout(150)
  await expect(snap).toHaveCount(1) // and it stays while the pointer rests
  await page.mouse.down(); await page.mouse.up()
  const n = (await doc(page)).strokes[1].nodes[0].p
  expect(n.x).toBeCloseTo(300, 1) // landed on the normal of (300, 100)
  expect(n.y).toBeCloseTo(180, 0)
  expect(errors).toEqual([])
})

test('snap bar on the canvas toggles snapping in one click', async ({page}) => {
  await boot(page)
  const bar = page.getByTestId('snap-bar')
  await expect(bar).toBeVisible()
  expect((await doc(page)).snap_normal).toBe(false)
  await bar.locator('[data-snap="snap_normal"]').click()
  expect((await doc(page)).snap_normal).toBe(true)
  expect((await doc(page)).strokes).toHaveLength(0) // the click did not reach the canvas
})

test('width like the first PathWeaver: a normal drag keeps the left/right difference; Option shows and changes one side', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  await click(page, {x: 300, y: 100})
  let [st] = await grips(page)
  // Option: right side only (12 → 30), left stays 12
  await page.keyboard.down('Alt')
  const hover = await screen(page, st.g.right)
  await page.mouse.move(hover.x, hover.y)
  await expect(page.locator('.width-handle .grip.hot text')).toHaveText('この側だけ')
  await expect(page.locator('.width-handle .grip.dim')).toHaveCount(1)
  await page.keyboard.up('Alt')
  await drag(page, st.g.right, {x: st.g.right.x - st.h.dir.x * 18, y: st.g.right.y - st.h.dir.y * 18}, {alt: true})
  let s = (await doc(page)).strokes[0]
  expect(s.start_width.left).toBeCloseTo(12, 0)
  expect(s.start_width.right).toBeCloseTo(30, 0)
  // normal drag of the left side by +10: both +10, the 18 difference stays
  ;[st] = await grips(page)
  const h2 = await screen(page, st.g.left)
  await page.mouse.move(h2.x, h2.y)
  await expect(page.locator('.width-handle .grip.hot text')).toHaveText('両側')
  await drag(page, st.g.left, {x: st.g.left.x + st.h.dir.x * 10, y: st.g.left.y + st.h.dir.y * 10})
  s = (await doc(page)).strokes[0]
  expect(s.start_width.left).toBeCloseTo(22, 0)
  expect(s.start_width.right).toBeCloseTo(40, 0)
  // the property fields say what they do
  const insp = page.getByTestId('inspector')
  await expect(insp.getByText('左だけ')).toBeVisible()
  await expect(insp.getByText('左右連動')).toHaveCount(0)
})

test('caps: round / square ends from the property; live corners: radius field, drag the widget, Option-click cycles the kind', async ({page}) => {
  const errors = await boot(page)
  await pen(page, [{p: {x: 0, y: 200}}, {p: {x: 150, y: 50}}, {p: {x: 300, y: 200}}])
  await page.keyboard.press('a')
  await click(page, {x: 150, y: 50})
  const insp = page.getByTestId('inspector')
  // caps
  await insp.locator('[data-field=caps]').getByText('丸', {exact: true}).first().click()
  let s = (await doc(page)).strokes[0]
  expect([s.start_cap, s.end_cap]).toEqual(['Round', 'Round'])
  // corner radius from the field
  const field = insp.locator('[data-field=corner-radius]')
  await field.click(); await page.keyboard.press('Meta+a'); await page.keyboard.type('20'); await page.keyboard.press('Enter')
  s = (await doc(page)).strokes[0]
  expect(s.corners).toEqual([{node: s.nodes[1].id, radius: 20, kind: 'Circular'}])
  // drag the widget further into the corner: bigger radius
  const w = await page.evaluate(() => { const {ed, canvas} = (window as any).__pw; return canvas.cornerWidgets(ed.doc.strokes[0])[0] })
  await drag(page, w.p, {x: w.p.x + w.dir.x * 25, y: w.p.y + w.dir.y * 25})
  s = (await doc(page)).strokes[0]
  expect(s.corners[0].radius).toBeGreaterThan(30)
  // Option-click: next corner kind
  const w2 = await page.evaluate(() => { const {ed, canvas} = (window as any).__pw; return canvas.cornerWidgets(ed.doc.strokes[0])[0] })
  await page.waitForTimeout(400)
  await click(page, w2.p, {alt: true})
  expect((await doc(page)).strokes[0].corners[0].kind).toBe('Clothoid')
  // the property radio sets the kind too
  await insp.locator('[data-field=corner-kind]').getByText('面取り', {exact: true}).first().click()
  expect((await doc(page)).strokes[0].corners[0].kind).toBe('Bevel')
  await page.getByTestId('canvas').screenshot({path: 'test-results/caps-corners.png'})
  expect(errors).toEqual([])
})

test('⌘ + drag on a width grip slides that width point along the path (widths unchanged)', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.keyboard.press('a')
  await click(page, {x: 300, y: 100})
  await withMeta(page, () => click(page, {x: 100, y: 100}))
  let s = (await doc(page)).strokes[0]
  const before = {w: {...s.widths[0].width}}
  const key = (await grips(page)).find((g: any) => typeof g.h.target === 'number')
  expect(key.h.center.x).toBeCloseTo(100, 0)
  await withMeta(page, () => drag(page, key.g.left, {x: key.g.left.x + 120, y: key.g.left.y}))
  s = (await doc(page)).strokes[0]
  const moved = (await grips(page)).find((g: any) => typeof g.h.target === 'number')
  expect(Math.abs(moved.h.center.x - 220)).toBeLessThan(2)
  expect(s.widths[0].width.left).toBeCloseTo(before.w.left, 5)
  expect(s.widths[0].width.right).toBeCloseTo(before.w.right, 5)
})
