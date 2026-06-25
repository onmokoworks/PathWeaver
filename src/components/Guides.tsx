import type { GlyphMetrics } from '../state/types'

type Props = {
  metrics: GlyphMetrics
}

export function Guides({ metrics }: Props) {
  const leftBearing = 120
  const rightBearing = 880

  return (
    <g className="guides">
      <rect className="glyph-box" x={leftBearing} y={metrics.capHeight} width={rightBearing - leftBearing} height={metrics.baseline - metrics.capHeight} />
      <line className="guide baseline" x1="0" y1={metrics.baseline} x2={metrics.width} y2={metrics.baseline} />
      <line className="guide capline" x1="0" y1={metrics.capHeight} x2={metrics.width} y2={metrics.capHeight} />
      <line className="guide xheight" x1="0" y1={metrics.xHeight} x2={metrics.width} y2={metrics.xHeight} />
      <line className="guide sidebearing" x1={leftBearing} y1="0" x2={leftBearing} y2={metrics.height} />
      <line className="guide sidebearing" x1={rightBearing} y1="0" x2={rightBearing} y2={metrics.height} />
    </g>
  )
}
