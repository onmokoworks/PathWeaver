// Renders paths with appearance graphs off the main thread, so a heavy effect
// (mezzotint → blur, …) never freezes the editor. Same core, same results.
import {call, initCore} from './wasm'

self.onmessage = async (e: MessageEvent) => {
  const m = e.data
  if (m.type === 'init') {
    await initCore(m.bytes)
    ;(self as unknown as Worker).postMessage({type: 'ready'})
    return
  }
  if (m.type === 'render') {
    let render
    try {
      render = call('render', {stroke: m.stroke})
    } catch (err) {
      render = {layers: [], hit: '', center: '', error: err instanceof Error ? err.message : String(err)}
    }
    ;(self as unknown as Worker).postMessage({type: 'rendered', id: m.id, key: m.key, render})
  }
}
