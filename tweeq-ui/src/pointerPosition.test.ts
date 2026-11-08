import {describe, expect, it} from 'vitest'
import {pointerPosition} from '../vendor/tweeq/src/use/pointerPosition'

describe('embedded WebView pointer coordinates', () => {
  it('uses client movement when unlocked even when movementX/Y are zero', () => {
    expect(pointerPosition({clientX:120,clientY:40,movementX:0,movementY:0}, [10,10], false)).toEqual([120,40])
  })
  it('uses client coordinates while a pointer-lock request is still pending', () => {
    expect(pointerPosition({clientX:25,clientY:16,movementX:200,movementY:300}, [20,10], false)).toEqual([25,16])
  })
  it('accumulates CSS pixel deltas only while this target owns the lock', () => {
    expect(pointerPosition({clientX:10,clientY:10,movementX:5,movementY:-2}, [120,40], true)).toEqual([125,38])
  })
  it('keeps a zero locked delta stationary instead of jumping to frozen client coordinates', () => {
    expect(pointerPosition({clientX:10,clientY:10,movementX:0,movementY:0}, [120,40], true)).toEqual([120,40])
  })
  it('does not let missing or non-finite locked movement poison later values', () => {
    expect(pointerPosition({clientX:10,clientY:10,movementX:NaN,movementY:Infinity}, [120,40], true)).toEqual([120,40])
    expect(pointerPosition({clientX:10,clientY:10}, [120,40], true)).toEqual([120,40])
  })
})
