import type { GridDot } from '../state/types'

type Props = {
  dots: GridDot[]
  visible: boolean
}

export function GridDots({ dots, visible }: Props) {
  if (!visible) return null

  return (
    <g className="grid-dots">
      {dots.map((dot) => (
        <circle key={dot.id} cx={dot.x} cy={dot.y} r={dot.radius} />
      ))}
    </g>
  )
}
