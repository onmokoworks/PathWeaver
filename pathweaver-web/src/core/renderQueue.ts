// Off-main-thread rendering for paths that use an appearance graph.
// Only the newest state of each path is computed; older requests are dropped.
import RenderWorker from './renderWorker?worker&inline'
import {coreBytes} from './wasm'
import type {Render, Stroke} from './types'

type Done = (id: number, key: string, render: Render) => void

export function createRenderQueue(done: Done) {
  let worker: Worker | null = null
  let ready = false
  const inFlight = new Set<number>()
  const waiting = new Map<number, {key: string; stroke: Stroke}>()
  const latest = new Map<number, string>()
  try {
    if (coreBytes) {
      worker = new RenderWorker()
      worker.onmessage = (e: MessageEvent) => {
        const m = e.data
        if (m.type === 'ready') { ready = true; for (const id of waiting.keys()) pump(id); return }
        if (m.type !== 'rendered') return
        inFlight.delete(m.id)
        done(m.id, m.key, m.render)
        pump(m.id)
      }
      worker.onerror = () => { worker = null }
      worker.postMessage({type: 'init', bytes: coreBytes.slice(0)})
    }
  } catch {
    worker = null
  }
  function pump(id: number) {
    if (!worker || !ready || inFlight.has(id)) return
    const job = waiting.get(id)
    if (!job) return
    waiting.delete(id)
    inFlight.add(id)
    worker.postMessage({type: 'render', id, key: job.key, stroke: JSON.parse(JSON.stringify(job.stroke))})
  }
  return {
    get available() { return worker !== null },
    /** Ask for a render of this exact state (deduplicated per path). */
    request(id: number, key: string, stroke: Stroke) {
      if (latest.get(id) === key && (inFlight.has(id) || waiting.has(id))) return
      latest.set(id, key)
      waiting.set(id, {key, stroke})
      pump(id)
    },
    get busy() { return inFlight.size + waiting.size },
  }
}
