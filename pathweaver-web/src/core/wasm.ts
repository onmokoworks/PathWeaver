// Synchronous JSON calls into pathweaver-core compiled to wasm32 (no bindgen).
// A panic in the core traps the instance (panic=abort); we read the message
// the core's panic hook left in memory, start a fresh instance and report a
// normal error, so one bad input never takes the whole editor down.
import wasmUrl from './pathweaver.wasm?url'

interface Exports {
  memory: WebAssembly.Memory
  pw_alloc(len: number): number
  pw_free(ptr: number, len: number): void
  pw_call(ptr: number, len: number): bigint
  pw_panic_buf(): number
  pw_panic_len_ptr(): number
}
let module: WebAssembly.Module | null = null
/** The wasm bytes, shared with the render worker. */
export let coreBytes: ArrayBuffer | null = null
let core: Exports | null = null
let restarts = 0
const encoder = new TextEncoder()
const decoder = new TextDecoder()

export async function initCore(bytes?: BufferSource) {
  if (core) return
  const source = bytes ?? await (await fetch(wasmUrl)).arrayBuffer()
  coreBytes = source instanceof ArrayBuffer ? source : null
  module = await WebAssembly.compile(source)
  core = (await WebAssembly.instantiate(module, {})).exports as unknown as Exports
}
function restart() {
  core = new WebAssembly.Instance(module!, {}).exports as unknown as Exports
  restarts++
}
export const coreRestarts = () => restarts

export class CoreError extends Error {}

// Placed images can be megabytes: only export and save need their pixels, so
// every other call sends the document without them (the core accepts that).
const NEEDS_ASSETS = new Set(['svg', 'save'])
function lean(op: string, args: Record<string, unknown>) {
  const d = args.doc as {assets?: Record<string, string>} | undefined
  if (NEEDS_ASSETS.has(op) || !d || typeof d !== 'object' || !d.assets || !Object.keys(d.assets).length) return args
  return {...args, doc: {...d, assets: {}}}
}

export function call<T = unknown>(op: string, args: Record<string, unknown> = {}): T {
  if (!core) throw new Error('core not initialized')
  const c = core
  const input = encoder.encode(JSON.stringify({op, ...lean(op, args)}))
  let packed: bigint
  try {
    const ptr = c.pw_alloc(input.length)
    new Uint8Array(c.memory.buffer, ptr, input.length).set(input)
    packed = c.pw_call(ptr, input.length)
    c.pw_free(ptr, input.length)
  } catch (e) {
    if (!(e instanceof WebAssembly.RuntimeError)) throw e
    let detail = ''
    try {
      const len = new Uint32Array(c.memory.buffer, c.pw_panic_len_ptr(), 1)[0]
      detail = decoder.decode(new Uint8Array(c.memory.buffer, c.pw_panic_buf(), Math.min(len, 1024)))
    } catch { /* message unavailable */ }
    restart()
    console.error('pathweaver-core panic:', op, detail)
    throw new CoreError(`計算中に内部エラーが起きたため、この操作は反映しませんでした（${op}${detail ? `：${detail.slice(0, 120)}` : ''}）`)
  }
  const outPtr = Number(packed >> 32n)
  const outLen = Number(packed & 0xffffffffn)
  const text = decoder.decode(new Uint8Array(c.memory.buffer, outPtr, outLen))
  c.pw_free(outPtr, outLen)
  const result = JSON.parse(text)
  if ('err' in result) throw new CoreError(result.err)
  return result.ok as T
}
