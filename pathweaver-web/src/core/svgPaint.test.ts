// @vitest-environment happy-dom
import {describe, expect, it} from 'vitest'
import {parseColor, parseTransform, readPaint, type GradientContext} from './svgPaint'
import type {Affine} from './svgPath'

function ctx(svgText: string, bbox = {x: 0, y: 0, width: 100, height: 50}, ctm: Affine = [1, 0, 0, 1, 0, 0]): GradientContext {
  const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml')
  return {byId: id => doc.getElementById(id), bbox: () => bbox, viewport: {w: 200, h: 200}, ctm}
}
const at = (m: number[], x: number, y: number) => ({x: m[0] * x + m[2] * y + m[4], y: m[1] * x + m[3] * y + m[5]})

describe('svg paint', () => {
  it('reads colours with alpha and multiplies the opacities', () => {
    expect(parseColor('rgba(10, 20, 30, 0.5)')).toEqual({color: [10, 20, 30], alpha: 0.5})
    expect(parseColor('#ff000080')!.alpha).toBeCloseTo(0.5, 2)
    expect(parseColor('none')).toBeNull()
    const p = readPaint('rgba(0, 0, 255, 0.5)', '0.5', 0.8, ctx('<svg xmlns="http://www.w3.org/2000/svg"/>'))!
    expect(p.color).toEqual([0, 0, 255])
    expect(p.opacity).toBeCloseTo(0.2)
    expect(readPaint('red', '0', 1, ctx('<svg xmlns="http://www.w3.org/2000/svg"/>'))).toBeNull()
  })
  it('linear gradients in bounding-box units follow href, stops and transforms', () => {
    const c = ctx(`<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
      <linearGradient id="base"><stop offset="0" stop-color="#ff0000"/><stop offset="100%" stop-color="#0000ff" stop-opacity=".5"/></linearGradient>
      <linearGradient id="g" xlink:href="#base" x1="0" y1="0" x2="1" y2="0"/>
    </svg>`, {x: 10, y: 20, width: 100, height: 50}, [1, 0, 0, 1, 5, 0])
    const p = readPaint('url(#g)', '1', 0.5, c)!
    expect(p.gradient!.kind).toBe('Linear')
    expect(p.gradient!.stops.map(s => s.color)).toEqual([[255, 0, 0], [0, 0, 255]])
    // group opacity folded into the stops
    expect(p.gradient!.stops.map(s => s.opacity)).toEqual([0.5, 0.25])
    expect(p.opacity).toBe(1)
    const m = p.gradient!.matrix
    expect(at(m, 0, 0)).toEqual({x: 15, y: 20})
    expect(at(m, 1, 0)).toEqual({x: 115, y: 20})
  })
  it('radial gradients in user space with gradientTransform; one stop is a solid colour', () => {
    const c = ctx(`<svg xmlns="http://www.w3.org/2000/svg">
      <radialGradient id="r" gradientUnits="userSpaceOnUse" cx="50" cy="40" r="10" gradientTransform="translate(100 0)">
        <stop offset="0" style="stop-color: rgb(0, 128, 0)"/><stop offset="0.2" stop-color="white"/><stop offset="0.1" stop-color="black"/>
      </radialGradient>
      <linearGradient id="one"><stop offset="0.3" stop-color="#123456"/></linearGradient>
    </svg>`)
    const p = readPaint('url("#r")', '1', 1, c)!
    expect(p.gradient!.kind).toBe('Radial')
    expect(p.gradient!.matrix).toEqual([10, 0, 0, 10, 150, 40])
    expect(p.gradient!.stops[0].color).toEqual([0, 128, 0])
    expect(p.gradient!.stops.map(s => s.offset)).toEqual([0, 0.2, 0.2]) // never goes back
    const one = readPaint('url(#one)', '1', 1, c)!
    expect(one.gradient).toBeNull()
    expect(one.color).toEqual([0x12, 0x34, 0x56])
  })
  it('parses transform lists', () => {
    const m = parseTransform('translate(10 20) scale(2)')
    expect(at(m, 1, 1)).toEqual({x: 12, y: 22})
    const r = parseTransform('rotate(90 10 10)')
    const p = at(r, 20, 10)
    expect(p.x).toBeCloseTo(10); expect(p.y).toBeCloseTo(20)
  })
})
