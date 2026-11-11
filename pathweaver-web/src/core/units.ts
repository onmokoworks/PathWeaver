// Measurement units (Blender's unit system): every length in the document is
// stored in one internal unit — 1 unit = 1 pt = 1 px at 72 ppi, the export's
// 1× — and only shown and typed in the chosen unit. Fields accept a number in
// that unit, a number with its own unit (`10mm`, `0.5in`, `12pt`) and simple
// arithmetic (`10mm + 2pt`, `x * 2`). Pure functions, no DOM: unit-tested.
import {reactive} from 'vue'

export type Unit = 'pt' | 'px' | 'mm' | 'cm' | 'in' | 'pc'
export const UNITS: Unit[] = ['pt', 'px', 'mm', 'cm', 'in', 'pc']

/** Internal units (pt) per one of each unit. px = pt (72 ppi, like Illustrator). */
export const PT_PER: Record<Unit, number> = {pt: 1, px: 1, mm: 72 / 25.4, cm: 720 / 25.4, in: 72, pc: 12}

export const UNIT_LABEL: Record<Unit, string> = {pt: 'ポイント', px: 'ピクセル', mm: 'ミリ', cm: 'センチ', in: 'インチ', pc: 'パイカ'}

/** Decimals shown in fields: about 1/100 pt or finer, never noisy. */
export const UNIT_DIGITS: Record<Unit, number> = {pt: 2, px: 2, mm: 2, cm: 3, in: 3, pc: 3}
/** Field drag: how much of the unit one pixel of mouse movement adds (≈ 1 pt). */
const DRAG: Record<Unit, number> = {pt: 1, px: 1, mm: 0.5, cm: 0.05, in: 0.01, pc: 0.1}
/** Arrow keys in a field: one press. */
const KEY: Record<Unit, number> = {pt: 1, px: 1, mm: 1, cm: 0.1, in: 0.1, pc: 1}

export const isUnit = (v: unknown): v is Unit => typeof v === 'string' && (UNITS as string[]).includes(v)

/** Document units (pt) → the unit. */
export const toUnit = (pt: number, u: Unit) => pt / PT_PER[u]
/** The unit → document units (pt). */
export const fromUnit = (v: number, u: Unit) => v * PT_PER[u]
export const roundTo = (v: number, digits: number) => {
  const k = 10 ** digits
  const r = Math.round(v * k) / k
  return Object.is(r, -0) ? 0 : r
}
/** A length for display in the unit: `12.7 mm`. */
export function formatLength(pt: number, u: Unit, digits = UNIT_DIGITS[u]) {
  return `${roundTo(toUnit(pt, u), digits)} ${u}`
}
/** W×H for display: `210×297 mm` (whole pt / px; mm 1 decimal, in 2). `bare`: no unit word. */
export function formatSize(w: number, h: number, u: Unit, bare = false) {
  const digits = u === 'pt' || u === 'px' ? 0 : UNIT_DIGITS[u] - 1
  const s = `${roundTo(toUnit(w, u), digits)}×${roundTo(toUnit(h, u), digits)}`
  return bare ? s : `${s} ${u}`
}

// Unit words a field understands after a number (ASCII, full-width and Japanese).
const ALIAS: Record<string, Unit> = {
  pt: 'pt', px: 'px', mm: 'mm', cm: 'cm', in: 'in', inch: 'in', inches: 'in', '"': 'in', '″': 'in', pc: 'pc',
  'ポイント': 'pt', 'ピクセル': 'px', 'ミリ': 'mm', 'センチ': 'cm', 'インチ': 'in', 'パイカ': 'pc',
}
const NUM_UNIT = /(\d+(?:\.\d*)?|\.\d+)\s*(inches|inch|pt|px|mm|cm|in|pc|"|″|ポイント|ピクセル|ミリ|センチ|インチ|パイカ)(?![A-Za-z_$0-9])/g

/** Full-width digits / signs (Japanese IME) → ASCII. */
function halfWidth(s: string) {
  return s.replace(/[０-９．＋－＊／（）　]/g, c => (c === '　' ? ' ' : String.fromCharCode(c.charCodeAt(0) - 0xfee0))).replace(/[×]/g, '*').replace(/[÷]/g, '/').replace(/−/g, '-')
}

/** Rewrite a typed expression so it evaluates in `u`: each `<number><unit>`
 *  becomes that amount in `u` (`10mm` in cm → `(10*0.1)`). Plain numbers are
 *  already in `u`. The result is a JS expression (Tweeq evaluates it). */
export function unitExpression(src: string, u: Unit): string {
  return halfWidth(src).replace(NUM_UNIT, (_, n: string, w: string) => {
    const k = PT_PER[ALIAS[w]] / PT_PER[u]
    return k === 1 ? `(${n})` : `(${n}*${k})`
  })
}

/** Evaluate typed text to document units (pt), or null. `x` = the current
 *  value (pt) for expressions like `x / 2`. Mirrors what a field does. */
export function parseLength(text: string, u: Unit, x = 0): number | null {
  const src = unitExpression(text, u)
  if (!/^[\d\s.+\-*/()x,e]*$/.test(src)) return null
  try {
    const v = Function('x', `"use strict"; return (${src})`)(toUnit(x, u)) as unknown
    return typeof v === 'number' && Number.isFinite(v) ? fromUnit(v, u) : null
  } catch {
    return null
  }
}

/** What a Tweeq number field inside a length field gets (see InputNumber/context.ts). */
export function unitContext(u: Unit) {
  return {
    expression: (src: string) => unitExpression(src, u),
    suffix: ` ${u}`,
    precision: UNIT_DIGITS[u],
    speed: DRAG[u],
    keyStep: KEY[u],
  }
}

/** A 1-2-5 step in the unit at least `minPt` document units long (rulers). */
export function niceStep(minPt: number, u: Unit) {
  const raw = toUnit(minPt, u)
  const p = Math.pow(10, Math.floor(Math.log10(raw)))
  for (const m of [1, 2, 5, 10]) if (m * p >= raw * (1 - 1e-9)) return m * p
  return 10 * p
}

/** The unit in use: the document's own, else the viewer's default (kept in sync by the editor). */
export const display = reactive<{unit: Unit}>({unit: 'pt'})

/** A live readout on the canvas / status bar in the unit in use (pt 1 decimal, mm 2, in 3). */
export const readout = (pt: number, u: Unit = display.unit) => formatLength(pt, u, u === 'pt' || u === 'px' ? 1 : UNIT_DIGITS[u])
