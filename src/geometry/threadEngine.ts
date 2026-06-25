import { createGridDot } from './grid'
import type {
  ActiveThread,
  GridDot,
  GridSettings,
  Point,
  RenderedThread,
  StartAnchor,
  StartPort,
  ThreadContact,
  ThreadSegment,
} from '../state/types'

export type StartCandidate = StartAnchor & {
  index: number
}

const EPSILON = 0.0001
const MIN_SEGMENT = 1.5
const MIN_ARC = 0.035

function format(value: number) {
  return Number(value.toFixed(2))
}

export function distance(a: Point, b: Point) {
  return Math.hypot(a.x - b.x, a.y - b.y)
}

function angleOf(point: Point, circle: GridDot) {
  return Math.atan2(point.y - circle.y, point.x - circle.x)
}

function pointOnCircle(circle: GridDot, angle: number): Point {
  return {
    x: circle.x + Math.cos(angle) * circle.radius,
    y: circle.y + Math.sin(angle) * circle.radius,
  }
}

function normalizeAngleDelta(delta: number) {
  let next = delta
  while (next > Math.PI) next -= Math.PI * 2
  while (next < -Math.PI) next += Math.PI * 2
  return next
}

function normalizeAngle(angle: number) {
  let next = angle
  while (next > Math.PI) next -= Math.PI * 2
  while (next <= -Math.PI) next += Math.PI * 2
  return next
}

function directedArcDelta(startAngle: number, endAngle: number, direction: 'cw' | 'ccw') {
  const raw = normalizeAngleDelta(endAngle - startAngle)
  if (direction === 'cw') return raw >= 0 ? raw : raw + Math.PI * 2
  return raw <= 0 ? Math.abs(raw) : Math.PI * 2 - raw
}

function continuousArcDelta(startAngle: number, endAngle: number, direction: 'cw' | 'ccw') {
  let delta = direction === 'cw' ? endAngle - startAngle : startAngle - endAngle
  while (delta < 0) delta += Math.PI * 2
  return delta
}

function shortestDirection(startAngle: number, endAngle: number): 'cw' | 'ccw' {
  return directedArcDelta(startAngle, endAngle, 'cw') <= directedArcDelta(startAngle, endAngle, 'ccw') ? 'cw' : 'ccw'
}

function startPortAngle(port: StartPort) {
  if (port === 'top') return -Math.PI / 2
  if (port === 'right') return 0
  if (port === 'bottom') return Math.PI / 2
  return Math.PI
}

function startAnchorPoint(pin: GridDot, port: StartPort): StartAnchor {
  const angle = startPortAngle(port)
  const point = pointOnCircle(pin, angle)
  return {
    ...point,
    pinId: pin.id,
    row: pin.row,
    col: pin.col,
    port,
    angle,
  }
}

function resolveStartAnchor(startAnchor: StartAnchor, gridSettings: GridSettings): StartAnchor {
  return startAnchorPoint(createGridDot(gridSettings, startAnchor.row, startAnchor.col), startAnchor.port)
}

export function getStartCandidates(pin: GridDot): StartCandidate[] {
  return (['top', 'right', 'bottom', 'left'] as const).map((port, index) => ({
    ...startAnchorPoint(pin, port),
    index,
  }))
}

export function findStartCandidate(point: Point, gridDots: GridDot[], radius: number): StartCandidate | undefined {
  return gridDots
    .flatMap(getStartCandidates)
    .map((candidate) => ({ candidate, distance: distance(point, candidate) }))
    .filter((candidate) => candidate.distance <= radius)
    .sort((a, b) => a.distance - b.distance)[0]?.candidate
}

export function findPinCandidate(point: Point, gridDots: GridDot[], radius: number): GridDot | undefined {
  return gridDots
    .map((pin) => ({ pin, distance: distance(point, pin) }))
    .filter(({ pin, distance }) => distance <= pin.radius + radius)
    .sort((a, b) => a.distance - b.distance)[0]?.pin
}

function lineCircleIntersections(from: Point, to: Point, circle: GridDot) {
  const dx = to.x - from.x
  const dy = to.y - from.y
  const fx = from.x - circle.x
  const fy = from.y - circle.y
  const a = dx * dx + dy * dy
  if (a <= EPSILON) return []

  const b = 2 * (fx * dx + fy * dy)
  const c = fx * fx + fy * fy - circle.radius * circle.radius
  const discriminant = b * b - 4 * a * c
  if (discriminant < -EPSILON) return []

  const root = Math.sqrt(Math.max(0, discriminant))
  return [(-b - root) / (2 * a), (-b + root) / (2 * a)]
    .filter((t) => t > 0.006 && t <= 1)
    .map((t) => ({
      t,
      point: {
        x: from.x + dx * t,
        y: from.y + dy * t,
      },
    }))
    .sort((aHit, bHit) => aHit.t - bHit.t)
}

function findFirstPinHit(from: Point, to: Point, pins: GridDot[], ignoredPinId?: string) {
  return pins
    .flatMap((pin) => {
      if (pin.id === ignoredPinId) return []
      return lineCircleIntersections(from, to, pin).map((hit) => ({ ...hit, pin }))
    })
    .sort((a, b) => a.t - b.t)[0]
}

function findFirstUncapturedPinHit(from: Point, to: Point, pins: GridDot[], contacts: ThreadContact[]) {
  const captured = new Set(contacts.map((contact) => contact.pinId))
  return pins
    .flatMap((pin) => {
      if (captured.has(pin.id)) return []
      return lineCircleIntersections(from, to, pin).map((hit) => ({ ...hit, pin }))
    })
    .sort((a, b) => a.t - b.t)[0]
}

function unwrapInDirection(reference: number, angle: number, direction: 'cw' | 'ccw') {
  let next = angle
  if (direction === 'cw') {
    while (next < reference - EPSILON) next += Math.PI * 2
  } else {
    while (next > reference + EPSILON) next -= Math.PI * 2
  }
  return next
}

function pointCircleTangents(from: Point, circle: GridDot) {
  const dx = from.x - circle.x
  const dy = from.y - circle.y
  const d = Math.hypot(dx, dy)
  if (d <= circle.radius + EPSILON) return []

  const theta = Math.atan2(dy, dx)
  const spread = Math.acos(circle.radius / d)
  return [theta + spread, theta - spread].map((angle) => ({
    angle,
    point: pointOnCircle(circle, angle),
  }))
}

function exitAngleForTarget(circle: GridDot, contact: ThreadContact, target: Point) {
  const tangentCandidates = pointCircleTangents(target, circle)
  const rawCandidates = tangentCandidates.length > 0 ? tangentCandidates.map((candidate) => candidate.angle) : [angleOf(target, circle)]

  if (!contact.direction) {
    return rawCandidates
      .flatMap((rawAngle) => {
        const direction = shortestDirection(contact.entryAngle, rawAngle)
        const angle = unwrapInDirection(contact.entryAngle, rawAngle, direction)
        return { angle, direction, score: Math.abs(angle - contact.entryAngle) }
      })
      .sort((a, b) => a.score - b.score)[0]
  }

  const currentAngle = unwrapInDirection(contact.entryAngle, normalizeAngle(contact.exitAngle), contact.direction)
  return rawCandidates
    .map((rawAngle) => {
      const angle = unwrapInDirection(contact.entryAngle, rawAngle, contact.direction!)
      return {
        angle,
        direction: contact.direction!,
        score: Math.abs(angle - currentAngle),
      }
    })
    .sort((a, b) => a.score - b.score)[0]
}

function normalizeContact(contact: ThreadContact): ThreadContact {
  if (!contact.direction) return contact
  return {
    ...contact,
    exitAngle: unwrapInDirection(contact.entryAngle, normalizeAngle(contact.exitAngle), contact.direction),
  }
}

function clampPointOutsidePins(point: Point, pins: GridDot[]) {
  const containingPin = pins.find((pin) => distance(point, pin) < pin.radius)
  if (!containingPin) return point
  return pointOnCircle(containingPin, angleOf(point, containingPin))
}

function makeLine(from: Point, to: Point): ThreadSegment | undefined {
  if (distance(from, to) < MIN_SEGMENT) return undefined
  return { type: 'line', from, to }
}

function makeArc(pin: GridDot, fromAngle: number, toAngle: number, lockedDirection?: 'cw' | 'ccw'): ThreadSegment | undefined {
  const direction = lockedDirection ?? shortestDirection(fromAngle, toAngle)
  const amount = lockedDirection ? continuousArcDelta(fromAngle, toAngle, direction) : directedArcDelta(fromAngle, toAngle, direction)
  if (amount < MIN_ARC || amount > Math.PI * 2 - MIN_ARC) return undefined
  return {
    type: 'arc',
    pinId: pin.id,
    cx: pin.x,
    cy: pin.y,
    radius: pin.radius,
    from: pointOnCircle(pin, fromAngle),
    to: pointOnCircle(pin, toAngle),
    startAngle: fromAngle,
    endAngle: toAngle,
    direction,
    largeArc: amount > Math.PI,
  }
}

function lineCommand(to: Point) {
  return `L ${format(to.x)} ${format(to.y)}`
}

function segmentCommand(segment: ThreadSegment) {
  if (segment.type === 'line') return lineCommand(segment.to)
  return [
    `A ${format(segment.radius)} ${format(segment.radius)}`,
    '0',
    segment.largeArc ? '1' : '0',
    segment.direction === 'cw' ? '1' : '0',
    `${format(segment.to.x)} ${format(segment.to.y)}`,
  ].join(' ')
}

function materializeContacts(_startAnchor: StartAnchor, contacts: ThreadContact[], _gridSettings: GridSettings): ThreadContact[] {
  if (contacts.length === 0) return []
  return contacts.map((contact) => normalizeContact({ ...contact }))
}

function appendContact(
  startAnchor: StartAnchor,
  contacts: ThreadContact[],
  pin: GridDot,
  hitPoint: Point,
  gridSettings: GridSettings,
  cursor: Point,
) {
  const next = contacts.map((contact) => ({ ...contact }))
  const hitAngle = angleOf(hitPoint, pin)

  if (next.length === 0) {
    const resolvedStart = resolveStartAnchor(startAnchor, gridSettings)
    const entryAngle = pin.id === resolvedStart.pinId ? resolvedStart.angle : hitAngle
    const exit = exitAngleForTarget(pin, { pinId: pin.id, row: pin.row, col: pin.col, entryAngle, exitAngle: entryAngle }, cursor)
    return [
      {
        pinId: pin.id,
        row: pin.row,
        col: pin.col,
        entryAngle,
        exitAngle: exit.angle,
        direction: exit.direction,
      },
    ]
  }

  const entryAngle = hitAngle
  const exit = exitAngleForTarget(pin, { pinId: pin.id, row: pin.row, col: pin.col, entryAngle, exitAngle: entryAngle }, cursor)

  return [
    ...next,
    {
      pinId: pin.id,
      row: pin.row,
      col: pin.col,
      entryAngle,
      exitAngle: exit.angle,
      direction: exit.direction,
    },
  ]
}

function contactEnd(contact: ThreadContact, gridSettings: GridSettings) {
  return pointOnCircle(createGridDot(gridSettings, contact.row, contact.col), contact.exitAngle)
}

function ropeHead(startAnchor: StartAnchor, contacts: ThreadContact[], gridSettings: GridSettings) {
  const materialized = materializeContacts(startAnchor, contacts, gridSettings)
  const last = materialized.at(-1)
  const resolvedStart = resolveStartAnchor(startAnchor, gridSettings)
  return last ? contactEnd(last, gridSettings) : { x: resolvedStart.x, y: resolvedStart.y }
}

function renderSegments(startAnchor: StartAnchor, contacts: ThreadContact[], gridSettings: GridSettings) {
  const materialized = materializeContacts(startAnchor, contacts, gridSettings)
  const resolvedStart = resolveStartAnchor(startAnchor, gridSettings)
  const segments: ThreadSegment[] = []
  let head: Point = { x: resolvedStart.x, y: resolvedStart.y }

  materialized.forEach((contact) => {
    const pin = createGridDot(gridSettings, contact.row, contact.col)
    const entry = pointOnCircle(pin, contact.entryAngle)
    const line = makeLine(head, entry)
    if (line) segments.push(line)
    const arc = makeArc(pin, contact.entryAngle, contact.exitAngle, contact.direction)
    if (arc) segments.push(arc)
    head = pointOnCircle(pin, contact.exitAngle)
  })

  return segments
}

function renderPreviewSegment(startAnchor: StartAnchor, contacts: ThreadContact[], gridSettings: GridSettings, cursor?: Point) {
  if (!cursor) return undefined
  const head = ropeHead(startAnchor, contacts, gridSettings)
  return makeLine(head, cursor)
}

function shouldReleaseLastContact(startAnchor: StartAnchor, contacts: ThreadContact[], pointer: Point, gridSettings: GridSettings) {
  if (contacts.length === 0) return false
  const last = contacts[contacts.length - 1]
  const previousHead = ropeHead(startAnchor, contacts.slice(0, -1), gridSettings)
  const lastPin = createGridDot(gridSettings, last.row, last.col)
  return lineCircleIntersections(previousHead, pointer, lastPin).length === 0
}

export function renderThread(
  startAnchor: StartAnchor,
  contacts: ThreadContact[],
  gridSettings: GridSettings,
  cursor?: Point,
): RenderedThread {
  const resolvedStart = resolveStartAnchor(startAnchor, gridSettings)
  const displayContacts = materializeContacts(startAnchor, contacts, gridSettings)
  const openSegments = renderSegments(startAnchor, displayContacts, gridSettings)
  const previewSegment = renderPreviewSegment(startAnchor, displayContacts, gridSettings, cursor)
  const startCommand = `M ${format(resolvedStart.x)} ${format(resolvedStart.y)}`
  const threadPathData = [startCommand, ...openSegments.map(segmentCommand)].join(' ')
  const previewPathData = previewSegment
    ? [`M ${format(previewSegment.from.x)} ${format(previewSegment.from.y)}`, segmentCommand(previewSegment)].join(' ')
    : undefined
  const closingHead = ropeHead(startAnchor, displayContacts, gridSettings)
  const closingLine = makeLine(closingHead, resolvedStart)
  const fillSegments = closingLine ? [...openSegments, closingLine] : openSegments
  const fillPathData = [startCommand, ...fillSegments.map(segmentCommand), 'Z'].join(' ')
  const allSegments = previewSegment ? [...openSegments, previewSegment] : openSegments

  return {
    segments: openSegments,
    threadPathData,
    previewPathData,
    fillPathData,
    invalid: hasSelfIntersection(allSegments),
  }
}

export function advanceThread(
  active: ActiveThread,
  pointer: Point,
  gridSettings: GridSettings,
  gridDots: GridDot[],
  closeRadius: number,
): ActiveThread {
  const threadTip = clampPointOutsidePins(pointer, gridDots)
  let contacts = materializeContacts(active.startAnchor, active.contacts, gridSettings)

  while (shouldReleaseLastContact(active.startAnchor, contacts, threadTip, gridSettings)) {
    contacts = materializeContacts(active.startAnchor, contacts.slice(0, -1), gridSettings)
  }

  for (let guard = 0; guard < 6; guard += 1) {
    const head = ropeHead(active.startAnchor, contacts, gridSettings)
    const hit = findFirstUncapturedPinHit(head, threadTip, gridDots, contacts) ?? findFirstPinHit(head, threadTip, gridDots, contacts.at(-1)?.pinId)
    if (!hit) break
    const existingIndex = contacts.findIndex((contact) => contact.pinId === hit.pin.id)
    if (existingIndex >= 0) {
      contacts = materializeContacts(active.startAnchor, contacts.slice(0, existingIndex + 1), gridSettings)
      break
    }
    contacts = appendContact(active.startAnchor, contacts, hit.pin, hit.point, gridSettings, threadTip)
  }

  const canClose = contacts.length > 0 && distance(threadTip, active.startAnchor) <= closeRadius
  const rendered = renderThread(active.startAnchor, contacts, gridSettings, threadTip)
  return {
    ...rendered,
    startAnchor: active.startAnchor,
    contacts,
    cursor: threadTip,
    canClose,
    activePinId: contacts.at(-1)?.pinId,
  }
}

export function grabThreadPin(active: ActiveThread, pin: GridDot, gridSettings: GridSettings, grabPoint: Point): ActiveThread {
  if (active.contacts.some((contact) => contact.pinId === pin.id)) return active
  const contacts = [
    ...active.contacts,
    {
      pinId: pin.id,
      row: pin.row,
      col: pin.col,
      entryAngle: angleOf(grabPoint, pin),
      exitAngle: angleOf(grabPoint, pin),
    },
  ]
  const cursor = pointOnCircle(pin, angleOf(grabPoint, pin))
  const rendered = renderThread(active.startAnchor, contacts, gridSettings, cursor)
  return {
    ...rendered,
    startAnchor: active.startAnchor,
    contacts,
    cursor,
    canClose: false,
    activePinId: pin.id,
  }
}

export function createActiveThread(startAnchor: StartAnchor): ActiveThread {
  return {
    startAnchor,
    contacts: [],
    cursor: startAnchor,
    canClose: false,
    activePinId: undefined,
    segments: [],
    threadPathData: `M ${format(startAnchor.x)} ${format(startAnchor.y)}`,
    fillPathData: '',
    invalid: false,
  }
}

function ccw(a: Point, b: Point, c: Point) {
  return (c.y - a.y) * (b.x - a.x) > (b.y - a.y) * (c.x - a.x)
}

function linesIntersect(a: Point, b: Point, c: Point, d: Point) {
  if (distance(a, c) < 4 || distance(a, d) < 4 || distance(b, c) < 4 || distance(b, d) < 4) return false
  return ccw(a, c, d) !== ccw(b, c, d) && ccw(a, b, c) !== ccw(a, b, d)
}

function hasSelfIntersection(segments: ThreadSegment[]) {
  const lines = segments.filter((segment): segment is Extract<ThreadSegment, { type: 'line' }> => segment.type === 'line')
  return lines.some((line, index) =>
    lines.slice(0, Math.max(0, index - 1)).some((previous) => linesIntersect(line.from, line.to, previous.from, previous.to)),
  )
}
