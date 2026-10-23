/**
 * True for input the user really made (keyboard, mouse, touch), false for events a script
 * dispatched (`isTrusted === false`). Only real input keeps the session from idle-locking.
 */
export function isTrustedInput(event: Event): boolean {
  return event.isTrusted;
}
