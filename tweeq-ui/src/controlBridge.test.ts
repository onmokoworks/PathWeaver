// @vitest-environment happy-dom
import {describe, expect, it, vi} from 'vitest'
import {createApp, defineComponent, h, nextTick, ref} from 'vue'
import {createPinia} from 'pinia'
vi.mock('@baku89/pave', () => ({Rect: {}}))
import './icons'
import InputNumber from '../vendor/tweeq/src/InputNumber/InputNumber.vue'
import {changeFromControl, numericStep} from './controlBridge'
import {Transactions, type Field, type Message} from './protocol'

const coordinate = (): Field => ({id:'path.anchor_x',label:'X',type:'number',value:10.25,step:1,min:-1e6,max:1e6})
describe('continuous numeric snapshot boundary', () => {
  it('uses hard step only for integer Seed', () => {
    for (const id of ['path.anchor_x','path.width','path.rotation','path.scale','param:Amount']) {
      expect(numericStep({...coordinate(),id})).toBeUndefined()
    }
    expect(numericStep({...coordinate(),id:'param:Seed'})).toBe(1)
  })
  it('ignores a coordinate validator echo during a rotation gesture', () => {
    const messages: Message[] = []; const tx = new Transactions(message => messages.push(message))
    const rotation: Field = {...coordinate(),id:'path.rotation',value:0}
    tx.begin('A',rotation)
    changeFromControl(tx,'A',rotation,3,'path.rotation')
    changeFromControl(tx,'A',coordinate(),10,'path.rotation')
    changeFromControl(tx,'A',rotation,30,'path.rotation')
    tx.commit()
    expect(messages.map(message => `${message.type}:${message.id}`)).toEqual(['begin:path.rotation','change:path.rotation','change:path.rotation','commit:path.rotation'])
  })
  it('allows continued typing after Enter while the same number remains focused', () => {
    const messages: Message[] = []; const tx = new Transactions(message => messages.push(message)); const field = coordinate()
    tx.begin('A',field); changeFromControl(tx,'A',field,10.5,field.id); tx.commit()
    changeFromControl(tx,'A',field,12.75,field.id)
    expect(tx.active?.value).toBe(12.75)
  })
  it('mounts and updates actual Tweeq InputNumber with fractional coordinates without document edits', async () => {
    const model = ref(10.25)
    const field = coordinate()
    const emitted: number[] = []
    const errors: unknown[] = []
    const Control = defineComponent({setup() {return () => h(InputNumber, {
      modelValue: model.value, min:field.min, max:field.max, step:numericStep(field), bar:false,
      'onUpdate:modelValue': (value: number) => emitted.push(value),
    })}})
    const app = createApp(Control).use(createPinia())
    app.config.errorHandler = error => errors.push(error)
    const element = document.createElement('div'); document.body.appendChild(element)
    app.mount(element); await nextTick()
    model.value = 10.987654321; await nextTick()
    expect(emitted).toEqual([])
    expect(errors).toEqual([])
    expect(element.querySelector('input')).not.toBeNull()
    app.unmount(); element.remove()
  })
})
