import type {Field, Value} from './protocol'
import {Transactions} from './protocol'

/** Tweeq's step is validation quantization, not drag sensitivity. */
export function numericStep(field: Field): number | undefined {
  return field.id === 'param:Seed' ? 1 : undefined
}

export function changeFromControl(tx: Transactions, revision: string, field: Field, value: Value, focusedNumeric: string | null) {
  // InputNumber may emit while reconciling external props through its validator.
  // Only the number a user is actually editing may change the Rust document.
  if (field.type === 'number' && tx.active?.id !== field.id && focusedNumeric !== field.id) return
  tx.change(revision, field, value)
}
