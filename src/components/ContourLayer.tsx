import type { GlyphContour, GlyphMetrics } from '../state/types'

type Props = {
  contours: GlyphContour[]
  metrics: GlyphMetrics
  selectedContourId: string | null
  onSelectContour: (id: string) => void
}

export function ContourLayer({ contours, metrics, selectedContourId, onSelectContour }: Props) {
  const visibleContours = contours.filter((contour) => contour.visible)
  void metrics

  return (
    <g className="contour-layer">
      {visibleContours.map((contour) => (
        <g key={contour.id}>
          <path className={contour.invalid ? 'contour-fill invalid' : 'contour-fill'} d={contour.fillPathData} />
          <path
            className={`contour-thread ${contour.invalid ? 'invalid' : ''} ${selectedContourId === contour.id ? 'selected' : ''}`}
            d={contour.fillPathData}
          />
          <path
            className="contour-hit"
            d={contour.fillPathData}
            onPointerDown={(event) => {
              event.stopPropagation()
              onSelectContour(contour.id)
            }}
          />
        </g>
      ))}
    </g>
  )
}
