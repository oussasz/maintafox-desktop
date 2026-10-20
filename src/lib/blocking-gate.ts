/**
 * Whether a blocking screen (the forced lock removal acknowledgement at sign-in) is shown: the
 * rest of the app is inert meanwhile, and document-level shortcuts (Ctrl+K command palette) must
 * open nothing under it.
 */

let active = false;

/** Set by the blocking screen when it shows or goes away. */
export function setBlockingGateActive(value: boolean): void {
  active = value;
}

/** True while a blocking screen is shown: global shortcuts do nothing. */
export function isBlockingGateActive(): boolean {
  return active;
}
