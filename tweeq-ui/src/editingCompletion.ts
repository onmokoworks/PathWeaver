import type {Field, Message, Transactions, Value} from './protocol'

interface CompletionOptions {
  tx: Transactions
  revision: () => string
  field: (target: EventTarget | null) => Field | undefined
  hasPopover: () => boolean
  composing: () => boolean
  revert: (change: {id: string; value: Value}) => void
  send: (message: Message) => void
}

/** Explicit text completion hands keyboard ownership back to the native canvas. */
export function editingCompletion(options: CompletionOptions) {
  return (event: KeyboardEvent) => {
    if (event.key !== 'Enter' && event.key !== 'Escape') return
    if (options.composing() || event.isComposing || event.keyCode === 229) {
      // Tweeq's Enter handlers don't check IME state. Leave the browser's
      // composition default action intact, but don't let Vue confirm it yet.
      event.stopImmediatePropagation()
      return
    }
    if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey || event.repeat) return
    const field = options.field(event.target)
    if (!field || field.type === 'select' || field.type === 'boolean' || options.hasPopover()) return
    const revision = options.revision()
    if (event.key === 'Escape') {
      const reverted = options.tx.cancel()
      if (reverted) options.revert(reverted)
      event.preventDefault()
      event.stopImmediatePropagation()
      options.send({type: 'canvas_focus', revision})
      return
    }
    // Enter on a paint swatch opens its picker; only text entry is completion.
    const input = event.target
    if (!(input instanceof HTMLInputElement || input instanceof HTMLTextAreaElement)) return
    if (input.closest('.TqInputTextBase.invalid')) {
      // Keep invalid expressions visible. Upstream confirm would otherwise
      // clear the local error and restore the previous value before native ack.
      event.preventDefault()
      event.stopImmediatePropagation()
      return
    }
    queueMicrotask(() => {
      if (options.revision() !== revision) return
      // Upstream's synchronous Enter handler has already emitted confirm.
      // This fallback also covers text inputs nested inside InputColor.
      options.tx.commit()
      options.send({type: 'canvas_focus', revision})
    })
  }
}
