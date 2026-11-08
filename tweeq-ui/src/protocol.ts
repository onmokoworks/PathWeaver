export type Value = string | number | boolean | null
export interface Field {
  id: string; label: string; type: 'number' | 'color' | 'text' | 'boolean' | 'select'; value: Value
  min?: number; max?: number; step?: number; suffix?: string; disabled?: boolean; nullable?: boolean
  options?: string[]; labels?: string[]; hint?: string
}
export interface Snapshot {
  revision: string; theme: 'light' | 'dark'; title: string; subtitle?: string; error?: string
  sections: {title: string; fields: Field[]}[]
  pending?: {id: string; revision: string}
  actions?: {id: string; label: string; icon?: string; disabled?: boolean}[]
}
export interface Message {type: string; revision?: string; id?: string; value?: Value; message?: string}
type Active = {revision: string; id: string; before: Value; value: Value}
export class Transactions {
  active: Active | null = null
  submitted = new Map<string, Active>()
  constructor(readonly send: (message: Message) => void) {}
  begin(revision: string, field: Field) {
    if (this.active?.id === field.id && this.active.revision === revision) return
    this.commit()
    this.active = {revision, id: field.id, before: field.value, value: field.value}
    this.send({type: 'begin', revision, id: field.id})
  }
  change(revision: string, field: Field, value: Value) {
    if (field.disabled || (typeof value === 'number' && !Number.isFinite(value))) return
    this.begin(revision, field)
    field.value = value
    this.active!.value = value
    this.send({type: 'change', revision, id: field.id, value})
  }
  commit() {
    if (!this.active) return
    const {revision, id} = this.active
    this.submitted.set(`${revision}:${id}`, {...this.active})
    if (this.submitted.size > 16) this.submitted.delete(this.submitted.keys().next().value!)
    this.active = null
    this.send({type: 'commit', revision, id})
  }
  cancel(): {id: string; value: Value} | null {
    if (!this.active) return null
    const {revision, id, before} = this.active
    this.submitted.delete(`${revision}:${id}`)
    this.active = null
    this.send({type: 'cancel', revision, id})
    return {id, value: before}
  }
  receive(snapshot: Snapshot) {
    // An external selection/document change invalidates the old edit context.
    // Never let a stale blur write into the new selection.
    if (this.active && this.active.revision !== snapshot.revision) this.active = null
    for (const [key, record] of this.submitted) {
      if (record.revision !== snapshot.revision) this.submitted.delete(key)
    }
    const pending = snapshot.pending
    if (snapshot.error && pending && !(this.active?.revision === pending.revision && this.active?.id === pending.id)) {
      const submitted = this.submitted.get(`${pending.revision}:${pending.id}`)
      if (submitted) this.active = {...submitted}
    }
  }
}
declare global {
  interface Window {
    pathweaverUpdate: (snapshot: Snapshot) => void
    pathweaverCommand: (command: string, text?: string) => void
    pathweaverFocusField: (id: string, revision?: string) => void
    webkit?: {messageHandlers?: {pathweaver?: {postMessage(message: string): void}}}
  }
}
export function send(message: Message) {
  window.webkit?.messageHandlers?.pathweaver?.postMessage(JSON.stringify(message))
}
