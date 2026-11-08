import {describe, expect, it} from 'vitest'
import {Transactions, type Message, type Field} from './protocol'
const field = (): Field => ({id:'path.width',label:'幅',type:'number',value:10})
describe('native document transaction bridge', () => {
  it('coalesces a scrub into one begin/commit with ordered previews', () => {
    const events: Message[] = []; const tx = new Transactions(m => events.push(m)); const f = field()
    tx.change('A', f, 12); tx.change('A', f, 14); tx.commit(); tx.commit()
    expect(events.map(m => m.type)).toEqual(['begin','change','change','commit'])
    expect(events.every(m => m.revision === 'A')).toBe(true)
  })
  it('restores the original value on Escape rather than the last preview', () => {
    const tx = new Transactions(() => {}); const f = field()
    tx.change('A', f, 12); tx.change('A', f, 15)
    expect(tx.cancel()).toEqual({id: 'path.width', value: 10})
    expect(tx.cancel()).toBeNull()
  })
  it('does not commit a stale field after selection changes', () => {
    const events: Message[] = []; const tx = new Transactions(m => events.push(m))
    tx.change('A', field(), 12)
    tx.receive({revision:'B', theme:'light', title:'別のパス', sections:[]}); tx.commit()
    expect(events.map(m => m.type)).toEqual(['begin','change'])
  })
  it('rejects disabled and nonfinite number updates', () => {
    const events: Message[] = []; const tx = new Transactions(m => events.push(m)); const f = field()
    tx.change('A', {...f, disabled:true}, 12); tx.change('A', f, Infinity)
    expect(events).toEqual([])
  })
  it('commits one field before beginning another', () => {
    const events: Message[] = []; const tx = new Transactions(m => events.push(m))
    tx.change('A', field(), 12); tx.change('A', {...field(),id:'path.rotation'}, 20)
    expect(events.map(m => `${m.type}:${m.id}`)).toEqual(['begin:path.width','change:path.width','commit:path.width','begin:path.rotation','change:path.rotation'])
  })
})

describe('rejected native commit', () => {
  it('retains the submitted proposal and original value until correction or Escape', () => {
    const events: Message[] = []; const tx = new Transactions(m => events.push(m))
    const f: Field = {id:'path.name',label:'名前',type:'text',value:'パス 1'}
    tx.change('A', f, ''); tx.commit()
    expect(tx.active).toBeNull()
    tx.receive({revision:'A',theme:'light',title:'パス 1',sections:[],error:'名前を入力してください',pending:{id:'path.name',revision:'A'}})
    expect(tx.active?.value).toBe('')
    expect(tx.cancel()).toEqual({id:'path.name',value:'パス 1'})
    expect(events.at(-1)?.type).toBe('cancel')
  })
})


it('recovers invalid A even if B was submitted before native rejection arrived', () => {
  const tx = new Transactions(() => {})
  const name: Field = {id:'path.name',label:'名前',type:'text',value:'パス 1'}
  tx.change('A', name, ''); tx.commit()
  tx.change('A', field(), 30); tx.commit()
  tx.receive({revision:'A',theme:'light',title:'パス 1',sections:[],error:'名前を入力してください',pending:{id:'path.name',revision:'A'}})
  expect(tx.active?.id).toBe('path.name')
  expect(tx.active?.value).toBe('')
  expect(tx.cancel()).toEqual({id:'path.name',value:'パス 1'})
})
