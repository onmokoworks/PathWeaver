import {describe, expect, it} from 'vitest'
import {knockPixels} from './knockout'

const px = (...rgba: number[]) => new Uint8ClampedArray(rgba)

describe('knockPixels', () => {
  it('black: black goes clear, white stays, colour keeps its hue', () => {
    const p = px(0, 0, 0, 255, 255, 255, 255, 255, 128, 64, 0, 255, 0, 0, 0, 0)
    knockPixels(p, 'black')
    expect(p[3]).toBe(0)
    expect([...p.slice(4, 8)]).toEqual([255, 255, 255, 255])
    // dark orange → full orange at half coverage (over black it looks the same)
    expect([...p.slice(8, 12)]).toEqual([255, 128, 0, 128])
    expect(p[15]).toBe(0)
  })
  it('white: paper goes clear, black ink stays, grey becomes half ink', () => {
    const p = px(255, 255, 255, 255, 0, 0, 0, 255, 128, 128, 128, 255, 255, 128, 128, 255)
    knockPixels(p, 'white')
    expect(p[3]).toBe(0)
    expect([...p.slice(4, 8)]).toEqual([0, 0, 0, 255])
    expect(p[11]).toBeGreaterThan(120)
    expect(p[11]).toBeLessThan(135)
    expect([...p.slice(8, 11)]).toEqual([0, 0, 0])
    // pink → full red at half coverage (over white it looks the same)
    expect([...p.slice(12, 16)]).toEqual([255, 0, 0, 127])
  })
  it('keeps existing transparency', () => {
    const p = px(255, 255, 255, 128)
    knockPixels(p, 'black')
    expect(p[3]).toBe(128)
  })
})
