import { useEffect, useMemo, useRef, useState } from 'react'
import type { PointerEvent } from 'react'
import { ControlPanel } from './components/ControlPanel'
import { GlyphCanvas } from './components/GlyphCanvas'
import { buildGlyphSvg, downloadTextFile } from './export/exportSvg'
import { createReferencedGridDots } from './geometry/grid'
import { advanceThread, createActiveThread, distance, findPinCandidate, findStartCandidate, grabThreadPin, renderThread } from './geometry/threadEngine'
import { createInitialProject } from './state/initialState'
import type { DebugState, GlyphContour, GlyphState, GridSettings, HistoryState, Point, ProjectState } from './state/types'
import './App.css'

const START_RADIUS = 12
const CLOSE_RADIUS = 14
const PIN_GRAB_RADIUS = 12
const TIP_GRAB_RADIUS = 16

function cloneProject(project: ProjectState): ProjectState {
  return structuredClone(project)
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false
  return target.matches('input, textarea, select, [contenteditable="true"]')
}

function clearActiveThreads(project: ProjectState): ProjectState {
  return {
    ...project,
    glyphs: Object.fromEntries(
      Object.entries(project.glyphs).map(([char, glyph]) => [char, { ...glyph, activeThread: undefined }]),
    ),
  }
}

function referencedPins(glyph: GlyphState) {
  return glyph.contours.flatMap((contour) => [
    { row: contour.startAnchor.row, col: contour.startAnchor.col },
    ...contour.contacts.map((contact) => ({ row: contact.row, col: contact.col })),
  ])
}

function normalizeGridSettings(settings: GridSettings): GridSettings {
  const spacing = Math.max(72, settings.spacing)
  const radius = clamp(settings.radius, 10, spacing * 0.42)
  return {
    ...settings,
    spacing,
    radius,
    visibleRows: clamp(Math.round(settings.visibleRows), 1, 15),
    visibleCols: clamp(Math.round(settings.visibleCols), 1, 15),
  }
}

function rerenderContours(glyph: GlyphState, gridSettings = glyph.gridSettings): GlyphContour[] {
  return glyph.contours.map((contour) => {
    const rendered = renderThread(contour.startAnchor, contour.contacts, gridSettings)
    return {
      ...contour,
      ...rendered,
      threadPathData: rendered.fillPathData,
    }
  })
}

function isThreadVisuallyClosed(thread: GlyphState['activeThread']) {
  if (!thread || !thread.canClose || thread.segments.length === 0) return false
  const last = thread.segments.at(-1)
  if (!last) return false
  return distance(last.to, thread.startAnchor) <= CLOSE_RADIUS
}

function hasLineIntersection(a: GlyphContour, b: GlyphContour) {
  const linesA = a.segments.filter((segment) => segment.type === 'line')
  const linesB = b.segments.filter((segment) => segment.type === 'line')
  return linesA.some((first) =>
    linesB.some((second) => {
      if (distance(first.from, second.from) < 4 || distance(first.from, second.to) < 4) return false
      if (distance(first.to, second.from) < 4 || distance(first.to, second.to) < 4) return false
      const ccw = (p1: Point, p2: Point, p3: Point) => (p3.y - p1.y) * (p2.x - p1.x) > (p2.y - p1.y) * (p3.x - p1.x)
      return ccw(first.from, second.from, second.to) !== ccw(first.to, second.from, second.to)
        && ccw(first.from, first.to, second.from) !== ccw(first.from, first.to, second.to)
    }),
  )
}

function App() {
  const [history, setHistory] = useState<HistoryState>(() => ({
    past: [],
    present: createInitialProject(),
    future: [],
  }))
  const [selectedContourId, setSelectedContourId] = useState<string | null>(null)
  const [debug, setDebug] = useState<DebugState>({ mode: 'idle' })
  const [isDraggingTip, setIsDraggingTip] = useState(false)
  const svgRef = useRef<SVGSVGElement | null>(null)

  const project = history.present
  const glyph = project.glyphs[project.currentGlyph]
  const selectedContour = useMemo(
    () => glyph.contours.find((contour) => contour.id === selectedContourId),
    [glyph.contours, selectedContourId],
  )

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (isTypingTarget(event.target)) return
      const meta = event.metaKey || event.ctrlKey

      if (meta && event.key.toLowerCase() === 'z' && event.shiftKey) {
        event.preventDefault()
        redo()
        return
      }
      if (meta && event.key.toLowerCase() === 'z') {
        event.preventDefault()
        undo()
        return
      }
      if (event.key === 'Escape') {
        event.preventDefault()
        cancelActiveThread()
        return
      }
      if ((event.key === 'Backspace' || event.key === 'Delete') && selectedContourId) {
        event.preventDefault()
        deleteSelectedContour()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  })

  function commitProject(updater: (project: ProjectState) => ProjectState) {
    setHistory((current) => {
      const previous = cloneProject(clearActiveThreads(current.present))
      const next = clearActiveThreads(updater(cloneProject(current.present)))
      return { past: [...current.past, previous], present: next, future: [] }
    })
  }

  function updatePresent(updater: (project: ProjectState) => ProjectState) {
    setHistory((current) => ({ ...current, present: updater(cloneProject(current.present)) }))
  }

  function updateGlyph(projectToUpdate: ProjectState, updater: (glyph: GlyphState) => GlyphState): ProjectState {
    const char = projectToUpdate.currentGlyph
    return {
      ...projectToUpdate,
      glyphs: { ...projectToUpdate.glyphs, [char]: updater(projectToUpdate.glyphs[char]) },
    }
  }

  function svgPoint(event: PointerEvent<SVGSVGElement>): Point {
    const svg = svgRef.current
    if (!svg) return { x: 0, y: 0 }
    const point = svg.createSVGPoint()
    point.x = event.clientX
    point.y = event.clientY
    const ctm = svg.getScreenCTM()
    if (!ctm) return { x: 0, y: 0 }
    const transformed = point.matrixTransform(ctm.inverse())
    return {
      x: clamp(transformed.x, 0, glyph.metrics.width),
      y: clamp(transformed.y, 0, glyph.metrics.height),
    }
  }

  function startThread(point: Point) {
    const start = findStartCandidate(point, glyph.gridDots, START_RADIUS)
    if (!start) {
      setDebug({ mode: 'idle' })
      return false
    }

    const activeThread = createActiveThread(start)
    setDebug({ mode: 'drawing', activePinId: start.pinId, contactCount: 0, canClose: false })
    updatePresent((current) => updateGlyph(current, (currentGlyph) => ({ ...currentGlyph, activeThread })))
    return true
  }

  function cancelActiveThread() {
    setDebug({ mode: 'idle' })
    updatePresent((current) => updateGlyph(current, (currentGlyph) => ({ ...currentGlyph, activeThread: undefined })))
  }

  function closeActiveThread(activeOverride?: GlyphState['activeThread']) {
    const activeThread = activeOverride ?? glyph.activeThread
    if (!activeThread || !isThreadVisuallyClosed(activeThread) || activeThread.invalid) return

    const rendered = renderThread(activeThread.startAnchor, activeThread.contacts, glyph.gridSettings)
    const contour: GlyphContour = {
      id: `contour-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      startAnchor: activeThread.startAnchor,
      contacts: activeThread.contacts,
      visible: true,
      ...rendered,
      threadPathData: rendered.fillPathData,
    }

    commitProject((current) =>
      updateGlyph(current, (currentGlyph) => ({
        ...currentGlyph,
        activeThread: undefined,
        contours: [...currentGlyph.contours, contour],
      })),
    )
    setSelectedContourId(contour.id)
    setDebug({ mode: 'closed' })
  }

  function updateActiveThread(point: Point) {
    const activeThread = glyph.activeThread
    if (!activeThread) return
    const hoverPin = findPinCandidate(point, glyph.gridDots, PIN_GRAB_RADIUS)
    const advanced = advanceThread(activeThread, point, glyph.gridSettings, glyph.gridDots, CLOSE_RADIUS)
    const next =
      hoverPin && !advanced.contacts.some((contact) => contact.pinId === hoverPin.id)
        ? grabThreadPin(advanced, hoverPin, glyph.gridSettings, point)
        : advanced

    const candidateContour: GlyphContour = {
      id: 'active',
      startAnchor: next.startAnchor,
      contacts: next.contacts,
      visible: true,
      segments: next.segments,
      threadPathData: next.threadPathData,
      fillPathData: next.fillPathData,
      invalid: next.invalid,
    }
    const intersectsExisting = glyph.contours.some((contour) => contour.visible && hasLineIntersection(candidateContour, contour))
    const invalid = next.invalid || intersectsExisting
    const withInvalid = { ...next, invalid }

    setDebug({
      mode: invalid ? 'invalid' : 'drawing',
      activePinId: next.activePinId,
      contactCount: next.contacts.length,
      direction: next.contacts.at(-1)?.direction,
      canClose: next.canClose,
      invalid,
    })
    updatePresent((current) => updateGlyph(current, (currentGlyph) => ({ ...currentGlyph, activeThread: withInvalid })))
  }

  function handlePointerDown(event: PointerEvent<SVGSVGElement>) {
    if (event.target !== event.currentTarget) return
    const point = svgPoint(event)
    setSelectedContourId(null)
    if (glyph.activeThread) {
      if (distance(point, glyph.activeThread.cursor) <= TIP_GRAB_RADIUS) {
        setIsDraggingTip(true)
        updateActiveThread(point)
        event.currentTarget.setPointerCapture(event.pointerId)
      }
      return
    }
    if (!glyph.activeThread && startThread(point)) {
      setIsDraggingTip(true)
      event.currentTarget.setPointerCapture(event.pointerId)
    }
  }

  function handlePointerMove(event: PointerEvent<SVGSVGElement>) {
    const point = svgPoint(event)
    if (!glyph.activeThread) {
      const start = findStartCandidate(point, glyph.gridDots, START_RADIUS)
      setDebug(
        start
          ? { mode: 'waiting-start', activePinId: start.pinId, hoverStartPinId: start.pinId, hoverStartPort: start.port }
          : { mode: 'idle' },
      )
      return
    }
    if (!isDraggingTip) {
      return
    }
    updateActiveThread(point)
  }

  function handlePointerUp(event: PointerEvent<SVGSVGElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
    const point = svgPoint(event)
    if (!glyph.activeThread) return
    setIsDraggingTip(false)
    if (!isDraggingTip) return
    const finalThread = advanceThread(glyph.activeThread, point, glyph.gridSettings, glyph.gridDots, CLOSE_RADIUS)
    if (isThreadVisuallyClosed(finalThread) && !finalThread.invalid) closeActiveThread(finalThread)
    else {
      updatePresent((current) => updateGlyph(current, (currentGlyph) => ({ ...currentGlyph, activeThread: finalThread })))
      setDebug({
        mode: finalThread.invalid ? 'invalid' : 'drawing',
        activePinId: finalThread.activePinId,
        contactCount: finalThread.contacts.length,
        direction: finalThread.contacts.at(-1)?.direction,
        canClose: finalThread.canClose,
        invalid: finalThread.invalid,
      })
      return
    }
  }

  function changeGrid<K extends keyof GridSettings>(key: K, value: GridSettings[K]) {
    commitProject((current) =>
      updateGlyph(current, (currentGlyph) => {
        const gridSettings = normalizeGridSettings({ ...currentGlyph.gridSettings, [key]: value })
        const contours = rerenderContours(currentGlyph, gridSettings)
        return {
          ...currentGlyph,
          gridSettings,
          contours,
          gridDots: createReferencedGridDots(gridSettings, referencedPins({ ...currentGlyph, contours })),
        }
      }),
    )
  }

  function deleteSelectedContour() {
    if (!selectedContourId) return
    commitProject((current) =>
      updateGlyph(current, (currentGlyph) => {
        const contours = currentGlyph.contours.filter((contour) => contour.id !== selectedContourId)
        return {
          ...currentGlyph,
          contours,
          gridDots: createReferencedGridDots(currentGlyph.gridSettings, referencedPins({ ...currentGlyph, contours })),
        }
      }),
    )
    setSelectedContourId(null)
  }

  function clearGlyph() {
    if (glyph.contours.length === 0 && !glyph.activeThread) return
    commitProject((current) => updateGlyph(current, (currentGlyph) => ({ ...currentGlyph, activeThread: undefined, contours: [] })))
    setSelectedContourId(null)
    setDebug({ mode: 'idle' })
  }

  function undo() {
    setHistory((current) => {
      if (current.past.length === 0) return current
      const past = [...current.past]
      const present = past.pop()
      if (!present) return current
      return { past, present, future: [cloneProject(clearActiveThreads(current.present)), ...current.future] }
    })
    setSelectedContourId(null)
    setDebug({ mode: 'idle' })
  }

  function redo() {
    setHistory((current) => {
      if (current.future.length === 0) return current
      const [present, ...future] = current.future
      return { past: [...current.past, cloneProject(clearActiveThreads(current.present))], present, future }
    })
    setSelectedContourId(null)
    setDebug({ mode: 'idle' })
  }

  function exportSvg() {
    downloadTextFile(`ThreadType-${glyph.char}.svg`, buildGlyphSvg(glyph), 'image/svg+xml')
  }

  return (
    <main className="app-shell">
      <section className="stage-panel">
        <div className="stage-header">
          <span>1000 x 1000</span>
          <span>origin {glyph.gridSettings.originX}, {glyph.gridSettings.originY}</span>
          <span>rows {glyph.gridSettings.visibleRows}</span>
          <span>cols {glyph.gridSettings.visibleCols}</span>
        </div>
        <div className="glyph-stage">
          <GlyphCanvas
            debug={debug}
            glyph={glyph}
            selectedContourId={selectedContourId}
            svgRef={svgRef}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onSelectContour={setSelectedContourId}
          />
        </div>
        <div className="hint-bar">
          <span>開始だけ上下左右</span>
          <span>自由区間は直線</span>
          <span>円だけに巻き付く</span>
        </div>
      </section>

      <ControlPanel
        canRedo={history.future.length > 0}
        canUndo={history.past.length > 0}
        gridSettings={glyph.gridSettings}
        selectedContour={selectedContour}
        onChangeGrid={changeGrid}
        onDeleteSelected={deleteSelectedContour}
        onClearGlyph={clearGlyph}
        onUndo={undo}
        onRedo={redo}
        onExportSvg={exportSvg}
      />
    </main>
  )
}

export default App
