// @vitest-environment happy-dom
import {afterEach, describe, expect, it, vi} from 'vitest'
import {createApp, defineComponent, h, nextTick, ref} from 'vue'
import {createPinia} from 'pinia'
vi.mock('@baku89/pave', () => ({Rect: {}}))
import './icons'
import InputNumber from '../vendor/tweeq/src/InputNumber/InputNumber.vue'
import {editingCompletion} from './editingCompletion'
import {Transactions, type Field, type Message} from './protocol'

const cleanup: (() => void)[] = []
afterEach(() => { while(cleanup.length) cleanup.pop()!() })
function mountNumber() {
  const messages: Message[] = []
  const tx = new Transactions(message => messages.push(message))
  const field: Field = {id:'path.width',label:'幅',type:'number',value:24}
  const value = ref(24)
  const state = {revision:'A', composing:false, popover:false}
  const handler = editingCompletion({tx, revision:() => state.revision,field:() => field,
    hasPopover:() => state.popover,composing:() => state.composing,
    revert:change => {field.value = change.value;value.value = Number(change.value)},
    send:message => messages.push(message),
  })
  window.addEventListener('keydown',handler,true)
  const Control = defineComponent({setup() {return () => h(InputNumber, {
    modelValue:value.value,bar:false,
    onFocus:() => tx.begin(state.revision,field),
    'onUpdate:modelValue':(next: number) => {tx.change(state.revision,field,next);value.value=next},
    onConfirm:() => tx.commit(),
  })}})
  const app = createApp(Control).use(createPinia())
  const element = document.createElement('div');document.body.appendChild(element);app.mount(element)
  cleanup.push(() => {window.removeEventListener('keydown',handler,true);app.unmount();element.remove()})
  const input = element.querySelector('input')!
  input.focus()
  const type = (text:string) => {input.value=text;input.dispatchEvent(new Event('input',{bubbles:true}))}
  const key = (key:string,options:KeyboardEventInit={}) => {
    const event = new KeyboardEvent('keydown',{key,bubbles:true,cancelable:true,...options})
    input.dispatchEvent(event);return event
  }
  return {messages,tx,field,value,state,input,type,key}
}

describe('Tweeq keyboard completion handoff', () => {
  it('actual InputNumber Enter commits before requesting native canvas focus', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    control.key('Enter');await nextTick()
    expect(control.messages.map(m => m.type)).toEqual(['begin','change','commit','canvas_focus'])
    expect(control.messages.at(-1)).toEqual({type:'canvas_focus',revision:'A'})
    // JavaScript must not blur before native validation accepts the request.
    expect(document.activeElement).toBe(control.input)
  })
  it('Escape cancels the proposal before requesting focus', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    control.key('Escape');await nextTick()
    expect(control.messages.map(m => m.type)).toEqual(['begin','change','cancel','canvas_focus'])
    expect(control.value.value).toBe(24)
  })
  it('keeps composition Enter from confirming while preserving the browser default', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    const event = control.key('Enter',{isComposing:true});await nextTick()
    expect(event.defaultPrevented).toBe(false)
    expect(control.messages.map(m => m.type)).toEqual(['begin','change'])
    expect(document.activeElement).toBe(control.input)
  })
  it('leaves Tab navigation, open-popover Enter, and scrub confirmations in the inspector', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    control.key('Tab');await nextTick()
    expect(control.messages.some(m => m.type === 'canvas_focus')).toBe(false)
    control.state.popover=true;control.key('Enter');await nextTick()
    expect(control.messages.some(m => m.type === 'canvas_focus')).toBe(false)
    control.tx.commit()
    expect(control.messages.some(m => m.type === 'canvas_focus')).toBe(false)
  })
  it('lets open popovers own Escape without canceling the underlying edit', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    const before = [...control.messages]
    control.state.popover=true;control.key('Escape');await nextTick()
    expect(control.messages).toEqual(before)
  })
  it('keeps an invalid local expression visible and focused', async () => {
    const control = mountNumber();control.type('not_a_number');await nextTick()
    expect(control.input.closest('.TqInputTextBase.invalid')).not.toBeNull()
    control.key('Enter');await nextTick()
    expect(control.messages.map(m => m.type)).toEqual(['begin'])
    expect(control.input.value).toBe('not_a_number')
    expect(document.activeElement).toBe(control.input)
  })
  it('does not hand off a deferred Enter after its target revision changes', async () => {
    const control = mountNumber();control.type('60');await nextTick()
    control.key('Enter');control.state.revision='B';await nextTick()
    expect(control.messages.some(m => m.type === 'canvas_focus')).toBe(false)
  })
})
