import {createApp} from 'vue'
import {createPinia} from 'pinia'
import './icons.generated'
import {initCore} from './core/wasm'
import App from './App.vue'
import './quiet.css'

// WebKit returns a Promise from requestPointerLock and rejects it when the lock
// is refused (WKWebView, no gesture). VueUse ignores that Promise, which would
// surface as an unhandled rejection. Tweeq's scrub already falls back to
// client coordinates, so a refusal is not an error.
const requestLock = Element.prototype.requestPointerLock
Element.prototype.requestPointerLock = function (this: Element, ...args: unknown[]) {
  const result = (requestLock as (...a: unknown[]) => unknown).apply(this, args)
  if (result instanceof Promise) result.catch(() => {})
  return result as never
} as typeof requestLock

// Report runtime errors to the native host (stderr) as well as the console.
const report = (e: unknown) => window.webkit?.messageHandlers?.pathweaver?.postMessage(JSON.stringify({type: 'error', message: String(e instanceof Error ? `${e.name}: ${e.message}` : e).slice(0, 400)}))
window.addEventListener('error', e => report(e.error ?? e.message))
window.addEventListener('unhandledrejection', e => report(e.reason))

await initCore()
const app = createApp(App).use(createPinia())
app.mount('#app')
