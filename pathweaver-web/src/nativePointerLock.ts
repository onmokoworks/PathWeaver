// Pointer Lock for the Mac app. WKWebView does not grant the web Pointer Lock,
// so a numeric drag (Tweeq InputNumber) stopped at the screen edge. Here the
// host does it natively: it freezes and hides the cursor
// (CGAssociateMouseAndMouseCursorPosition) and sends raw mouse deltas, which
// arrive as pointermove events with movementX/Y on the locked element, the
// same shape the web API gives. Real pointermoves are held back while locked.
type Post = (m: {type: string; value: boolean}) => void

export function installNativePointerLock(post: Post) {
  let locked: Element | null = null
  let synthetic = false
  let last = {x: 0, y: 0}
  const changed = () => queueMicrotask(() => document.dispatchEvent(new Event('pointerlockchange')))

  try {
    Object.defineProperty(Document.prototype, 'pointerLockElement', {configurable: true, get: () => locked})
  } catch { return } // cannot override: leave the platform behaviour
  Element.prototype.requestPointerLock = function (this: Element) {
    locked = this
    post({type: 'pointerLock', value: true})
    changed()
    return Promise.resolve()
  } as typeof Element.prototype.requestPointerLock
  Document.prototype.exitPointerLock = function () {
    if (!locked) return
    locked = null
    post({type: 'pointerLock', value: false})
    changed()
  }

  window.addEventListener('pointermove', e => {
    if (!synthetic) last = {x: e.clientX, y: e.clientY}
    if (locked && !synthetic) e.stopImmediatePropagation()
  }, true)

  const w = window as unknown as Record<string, unknown>
  w.__pwPointerDelta = (dx: number, dy: number) => {
    if (!locked) return
    const ev = new PointerEvent('pointermove', {
      bubbles: true, cancelable: true, composed: true, isPrimary: true, pointerId: 1, pointerType: 'mouse',
      buttons: 1, clientX: last.x, clientY: last.y,
    })
    Object.defineProperty(ev, 'movementX', {value: dx})
    Object.defineProperty(ev, 'movementY', {value: dy})
    synthetic = true
    try { locked.dispatchEvent(ev) } finally { synthetic = false }
  }
  // The host released the lock by itself (mouse up, window lost focus).
  w.__pwPointerUnlocked = () => {
    if (!locked) return
    locked = null
    changed()
  }
}
