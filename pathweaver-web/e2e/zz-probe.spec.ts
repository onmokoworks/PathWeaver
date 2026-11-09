import {test} from '@playwright/test'
import {boot, pen} from './helpers'
test('probe stack', async ({page}) => {
  await boot(page)
  await pen(page, [{p: {x: 100, y: 150}, drag: {x: 200, y: 50}}, {p: {x: 400, y: 150}}, {p: {x: 400, y: 350}}])
  const cases: Record<string, (call: any) => any> = {
    vector: (call: any) => {
      let g = call('graphDefault')
      const fill = call('nodeNew', {kind: 'Fill', id: 10, position: [150, 0]}); fill.color = [230, 60, 60]
      const tr = call('nodeNew', {kind: 'Transform', id: 11, position: [150, 200]}); tr.translation = [40, 40]
      const fill2 = call('nodeNew', {kind: 'Fill', id: 12, position: [350, 200]}); fill2.color = [60, 90, 230]
      const st = call('nodeNew', {kind: 'Stack', id: 13, position: [600, 0]})
      g.nodes.push(fill, tr, fill2, st)
      g = call('graphConnect', {graph: g, from: 1, to: 10, port: 0})
      g = call('graphConnect', {graph: g, from: 1, to: 11, port: 0})
      g = call('graphConnect', {graph: g, from: 11, to: 12, port: 0})
      g = call('graphConnect', {graph: g, from: 10, to: 13, port: 0})
      g = call('graphConnect', {graph: g, from: 12, to: 13, port: 1})
      g = call('graphConnect', {graph: g, from: 13, to: 2, port: 0})
      return g
    },
    glow: (call: any) => {
      let g = call('graphDefault')
      const glow = call('nodeNew', {kind: 'Glow', id: 10, position: [150, 0]}); glow.amount = 30
      const tr = call('nodeNew', {kind: 'Transform', id: 11, position: [150, 200]}); tr.translation = [60, 60]
      const st = call('nodeNew', {kind: 'Stack', id: 13, position: [600, 0]})
      g.nodes.push(glow, tr, st)
      g = call('graphConnect', {graph: g, from: 1, to: 10, port: 0})
      g = call('graphConnect', {graph: g, from: 1, to: 11, port: 0})
      g = call('graphConnect', {graph: g, from: 10, to: 13, port: 0})
      g = call('graphConnect', {graph: g, from: 11, to: 13, port: 1})
      g = call('graphConnect', {graph: g, from: 13, to: 2, port: 0})
      return g
    },
  }
  for (const name of Object.keys(cases)) {
    await page.evaluate((src) => {
      const {ed, call} = (window as any).__pw
      const f = new Function('call', 'return (' + src + ')(call)')
      ed.edit(() => { ed.doc.strokes[0].appearance = f(call) })
      ed.selected = []
    }, cases[name].toString())
    await page.waitForFunction(() => { const {ed} = (window as any).__pw; const r = ed.renders.get(ed.doc.strokes[0].id); return ed.renderPending === 0 && r && !r.stale })
    console.log('INFO', name, await page.evaluate(() => { const {ed} = (window as any).__pw; const r = ed.renders.get(ed.doc.strokes[0].id); return JSON.stringify(r.layers.map((l: any) => [l.color, l.opacity, !!l.image])) + (r.error || '') }))
    await page.getByTestId('canvas').screenshot({path: '/private/tmp/claude-501/-Users-student/656c2036-c9fe-4225-82cf-f2df06a2f9f7/scratchpad/stack-' + name + '.png'})
  }
})
