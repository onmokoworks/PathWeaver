import { ContourLayer } from './ContourLayer'
import { GridDots } from './GridDots'
import { Guides } from './Guides'
import { getStartCandidates } from '../geometry/threadEngine'
import type { PointerEvent, RefObject } from 'react'
import type { DebugState, GlyphState } from '../state/types'

type Props = {
  debug: DebugState
  glyph: GlyphState
  selectedContourId: string | null
  svgRef: RefObject<SVGSVGElement | null>
  onPointerDown: (event: PointerEvent<SVGSVGElement>) => void
  onPointerMove: (event: PointerEvent<SVGSVGElement>) => void
  onPointerUp: (event: PointerEvent<SVGSVGElement>) => void
  onSelectContour: (id: string) => void
}

export function GlyphCanvas({
  debug,
  glyph,
  selectedContourId,
  svgRef,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onSelectContour,
}: Props) {
  const activeThread = glyph.activeThread
  const shouldShowStartCandidates = !activeThread && debug.hoverStartPinId

  return (
    <svg
      ref={svgRef}
      className="editor-svg"
      viewBox={`0 0 ${glyph.metrics.width} ${glyph.metrics.height}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <Guides metrics={glyph.metrics} />
      <GridDots dots={glyph.gridDots} visible={glyph.gridSettings.visible} />
      {shouldShowStartCandidates && (
        <g className="start-candidates">
          {glyph.gridDots.flatMap((dot) =>
            getStartCandidates(dot).map((candidate) => (
              candidate.pinId === debug.hoverStartPinId && candidate.port === debug.hoverStartPort ? (
                <circle
                  className="start-candidate active"
                  cx={candidate.x}
                  cy={candidate.y}
                  key={`${candidate.pinId}-${candidate.port}`}
                  r="3.6"
                />
              ) : null
            )),
          )}
        </g>
      )}
      <ContourLayer
        contours={glyph.contours}
        metrics={glyph.metrics}
        selectedContourId={selectedContourId}
        onSelectContour={onSelectContour}
      />

      {activeThread && (
        <g className={activeThread.invalid ? 'active-thread invalid' : 'active-thread'}>
          {activeThread.canClose && !activeThread.invalid && <path className="active-fill-preview" d={activeThread.fillPathData} />}
          <path className="active-thread-line" d={activeThread.threadPathData} />
          {activeThread.previewPathData && <path className="active-preview-line" d={activeThread.previewPathData} />}
          {activeThread.canClose && (
            <circle className="close-anchor" cx={activeThread.startAnchor.x} cy={activeThread.startAnchor.y} r="4" />
          )}
          <circle className="thread-tip" cx={activeThread.cursor.x} cy={activeThread.cursor.y} r="3.2" />
        </g>
      )}
    </svg>
  )
}
