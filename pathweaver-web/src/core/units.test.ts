import {describe, expect, it} from 'vitest'
import {formatLength, formatSize, fromUnit, niceStep, parseLength, readout, toUnit, unitExpression, UNITS} from './units'

describe('units', () => {
  it('converts relative to 1 pt (px = pt at 72 ppi)', () => {
    expect(toUnit(72, 'in')).toBe(1)
    expect(toUnit(72, 'px')).toBe(72)
    expect(toUnit(12, 'pc')).toBe(1)
    expect(fromUnit(25.4, 'mm')).toBeCloseTo(72, 10)
    expect(fromUnit(2.54, 'cm')).toBeCloseTo(72, 10)
    for (const u of UNITS) expect(fromUnit(toUnit(123.456, u), u)).toBeCloseTo(123.456, 10)
  })

  it('formats in the unit with its precision', () => {
    expect(formatLength(fromUnit(10, 'mm'), 'mm')).toBe('10 mm')
    expect(formatLength(100, 'in')).toBe('1.389 in')
    expect(formatSize(fromUnit(210, 'mm'), fromUnit(297, 'mm'), 'mm')).toBe('210×297 mm')
    expect(formatSize(595.3, 841.9, 'pt')).toBe('595×842 pt')
    expect(formatSize(612, 792, 'in', true)).toBe('8.5×11')
    expect(readout(12.345, 'pt')).toBe('12.3 pt')
    expect(readout(fromUnit(3.14159, 'mm'), 'mm')).toBe('3.14 mm')
  })

  it('reads a plain number in the current unit', () => {
    expect(parseLength('10', 'mm')).toBeCloseTo(fromUnit(10, 'mm'), 10)
    expect(parseLength('12.5', 'pt')).toBe(12.5)
  })

  it('reads a number with its own unit, whatever the field shows', () => {
    expect(parseLength('10mm', 'pt')).toBeCloseTo(28.3465, 3)
    expect(parseLength('0.5in', 'mm')).toBeCloseTo(36, 10)
    expect(parseLength('12pt', 'in')).toBeCloseTo(12, 10)
    expect(parseLength('1 cm', 'mm')).toBeCloseTo(fromUnit(10, 'mm'), 10)
    expect(parseLength('2"', 'pt')).toBeCloseTo(144, 10)
    expect(parseLength('1pc', 'pt')).toBeCloseTo(12, 10)
    expect(parseLength('5ミリ', 'pt')).toBeCloseTo(fromUnit(5, 'mm'), 10)
    expect(parseLength('１０ｍｍ'.replace('ｍｍ', 'mm'), 'pt')).toBeCloseTo(fromUnit(10, 'mm'), 10) // full-width digits
  })

  it('mixes units with arithmetic (Blender style)', () => {
    expect(parseLength('10mm + 2pt', 'pt')).toBeCloseTo(fromUnit(10, 'mm') + 2, 10)
    expect(parseLength('1in - 1cm', 'mm')).toBeCloseTo(fromUnit(15.4, 'mm'), 10)
    expect(parseLength('(1in + 1in) / 2', 'pt')).toBeCloseTo(72, 10)
    expect(parseLength('2 * 3mm', 'mm')).toBeCloseTo(fromUnit(6, 'mm'), 10)
    // x = the current value
    expect(parseLength('x / 2', 'mm', fromUnit(10, 'mm'))).toBeCloseTo(fromUnit(5, 'mm'), 10)
    expect(parseLength('x + 1mm', 'pt', 10)).toBeCloseTo(10 + fromUnit(1, 'mm'), 10)
  })

  it('rejects text that is not a length', () => {
    expect(parseLength('10 furlong', 'pt')).toBeNull()
    expect(parseLength('alert(1)', 'pt')).toBeNull()
    expect(parseLength('1/0', 'pt')).toBeNull()
    expect(parseLength('', 'pt')).toBeNull()
  })

  it('rewrites only number+unit pairs (the field evaluates the rest)', () => {
    expect(unitExpression('10', 'mm')).toBe('10')
    expect(unitExpression('10mm', 'mm')).toBe('(10)')
    expect(unitExpression('1in', 'pt')).toBe('(1*72)')
    expect(unitExpression('x*2', 'pt')).toBe('x*2')
  })

  it('picks 1-2-5 ruler steps in the unit', () => {
    expect(niceStep(70, 'pt')).toBe(100)
    expect(niceStep(20, 'pt')).toBe(20)
    expect(niceStep(fromUnit(7, 'mm'), 'mm')).toBe(10)
    expect(niceStep(fromUnit(0.3, 'in'), 'in')).toBe(0.5)
    expect(niceStep(fromUnit(1, 'cm'), 'cm')).toBe(1)
  })
})
