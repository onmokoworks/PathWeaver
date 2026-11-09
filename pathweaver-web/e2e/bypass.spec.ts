import {test, expect} from '@playwright/test'
import {boot, pen} from './helpers'
test('N-04 bypass buttons in a chain (ラフ → 光彩) update the canvas each time', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 0, y: 100}}, {p: {x: 300, y: 100}}])
  await page.getByTestId('look-new').click()
  await page.evaluate(() => {
    const {ed, call} = (window as any).__pw
    ed.edit(() => {
      const s = ed.doc.strokes[0]
      let g = s.appearance
      g.nodes.push(call('nodeNew', {kind: 'Rough', id: 10, position: [150, 200]}))
      g.nodes.push(call('nodeNew', {kind: 'Glow', id: 11, position: [380, 200]}))
      g = call('graphConnect', {graph: g, from: 1, to: 10, port: 0})
      g = call('graphConnect', {graph: g, from: 10, to: 11, port: 0})
      g = call('graphConnect', {graph: g, from: 11, to: 2, port: 0})
      s.appearance = g
    })
  })
  const settle = () => page.waitForFunction(() => { const {ed} = (window as any).__pw; const r = ed.renders.get(ed.doc.strokes[0].id); return ed.renderPending === 0 && r && !r.stale }, null, {timeout: 20000})
  const sig = async () => { await settle(); return page.evaluate(() => { const {ed} = (window as any).__pw; const r = ed.renders.get(ed.doc.strokes[0].id); return JSON.stringify(r.layers).length }) }
  const both = await sig()
  const plain = await page.evaluate(() => { const {ed, call} = (window as any).__pw; return JSON.stringify(call('render', {stroke: {...ed.doc.strokes[0], appearance: null}}).layers).length })
  await page.locator('[data-node="11"] .bypass-btn').click()
  const roughOnly = await sig()
  expect(roughOnly).not.toBe(both)
  await page.locator('[data-node="10"] .bypass-btn').click()
  expect(await sig()).toBe(plain)
  await page.locator(`[data-node="11"] .bypass-btn`).click(); await page.waitForTimeout(300)
  expect(await sig()).not.toBe(plain)
  await page.locator('[data-node="10"] .bypass-btn').click()
  expect(await sig()).toBe(both)
})
