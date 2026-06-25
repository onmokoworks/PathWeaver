import type { GridDot, GridSettings, Point } from '../state/types'

export function pinId(row: number, col: number) {
  return `r${row}:c${col}`
}

export function pinCenter(settings: GridSettings, row: number, col: number): Point {
  return {
    x: settings.originX + col * settings.spacing,
    y: settings.originY + row * settings.spacing,
  }
}

export function createGridDot(settings: GridSettings, row: number, col: number): GridDot {
  const center = pinCenter(settings, row, col)
  return {
    id: pinId(row, col),
    row,
    col,
    x: center.x,
    y: center.y,
    radius: settings.radius,
  }
}

export function createGridDots(settings: GridSettings): GridDot[] {
  const dots: GridDot[] = []
  const rowMin = -Math.floor(settings.visibleRows / 2)
  const colMin = -Math.floor(settings.visibleCols / 2)

  for (let row = rowMin; row < rowMin + settings.visibleRows; row += 1) {
    for (let col = colMin; col < colMin + settings.visibleCols; col += 1) {
      dots.push(createGridDot(settings, row, col))
    }
  }

  return dots
}

export function createReferencedGridDots(settings: GridSettings, refs: Array<{ row: number; col: number }>) {
  const dots = createGridDots(settings)
  const seen = new Set(dots.map((dot) => dot.id))

  refs.forEach(({ row, col }) => {
    const id = pinId(row, col)
    if (!seen.has(id)) {
      dots.push(createGridDot(settings, row, col))
      seen.add(id)
    }
  })

  return dots
}
