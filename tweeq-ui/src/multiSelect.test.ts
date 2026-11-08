// @vitest-environment happy-dom
import {describe, expect, it, vi} from 'vitest'
import {createApp, defineComponent, h, nextTick, ref} from 'vue'
import {createPinia} from 'pinia'
// Geometry is not exercised by registration/unmount; avoid Paper's canvas
// initialization in the DOM-only lifecycle test.
vi.mock('@baku89/pave', () => ({Rect: {}}))
import {useMultiSelectStore} from '../vendor/tweeq/src/stores/multiSelect'

describe('Tweeq selected field lifecycle', () => {
  it('unregisters selection before remounting an inspector field', async () => {
    const pinia = createPinia()
    const focusing = ref(false)
    const errors: unknown[] = []
    const Control = defineComponent({setup() {
      const el = ref<HTMLInputElement | null>(null)
      const store = useMultiSelectStore()
      const registered = store.register({type:'number',el,focusing,getValue:() => 24,setValue:() => {},confirm:() => {}})
      return () => h('input', {ref: el, 'data-ready': registered.readyToBeSelected})
    }})
    const mount = () => {
      const app = createApp(Control).use(pinia)
      app.config.errorHandler = error => errors.push(error)
      const element = document.createElement('div'); document.body.appendChild(element)
      app.mount(element)
      return {app, element}
    }
    const first = mount()
    focusing.value = true
    await nextTick()
    const store = useMultiSelectStore(pinia)
    expect(store.selectedInputs).toHaveLength(1)
    first.app.unmount(); first.element.remove()
    expect(store.selectedInputs).toEqual([])
    focusing.value = false
    const second = mount()
    focusing.value = true
    await nextTick()
    expect(errors).toEqual([])
    expect(store.selectedInputs).toHaveLength(1)
    expect(second.element.querySelector('input')).not.toBeNull()
    second.app.unmount(); second.element.remove()
  })
})
