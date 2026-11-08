/**
 * Embedded WKWebViews may expose movementX/Y as zero outside pointer lock, and
 * outerWidth describes the host window rather than the embedded viewport.
 * Client coordinates are authoritative until this element owns pointer lock.
 * Under lock movement is already expressed in CSS pixels; devicePixelRatio is
 * a backing-store scale, not an input-coordinate conversion.
 */
export function pointerPosition(
  event: {clientX: number; clientY: number; movementX?: number; movementY?: number},
  current: readonly [number, number],
  ownsPointerLock: boolean,
): [number, number] {
  if (!ownsPointerLock) return [event.clientX, event.clientY]
  const dx = Number.isFinite(event.movementX) ? event.movementX! : 0
  const dy = Number.isFinite(event.movementY) ? event.movementY! : 0
  return [current[0] + dx, current[1] + dy]
}
