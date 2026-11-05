/** Frames to wait for the error to render before giving up (about a fifth of a second). */
const MAX_ATTEMPTS = 12;
const ATTEMPT_DELAY_MS = 16;

/**
 * Brings the first error of a long form into view once it has rendered: scrolls it to the
 * middle of the dialog and puts the keyboard focus on its control. Field errors are the
 * `role="alert"` lines that `FormField` renders under each control (id `<name>-error`, control
 * id `<name>`); a form-level alert with no control is only scrolled to.
 */
export function revealFirstFormError(formId: string): void {
  // Errors appear on a later render than the submit handler that sets them, and React does not
  // promise which frame: look again for a few frames until the alert is there.
  const attempt = (left: number) => {
    const form = document.getElementById(formId);
    const alert = form?.querySelector<HTMLElement>('[role="alert"]');
    if (!alert) {
      if (left > 1) window.setTimeout(() => attempt(left - 1), ATTEMPT_DELAY_MS);
      return;
    }
    alert.scrollIntoView?.({ block: "center" });
    const name = alert.id.endsWith("-error") ? alert.id.slice(0, -"-error".length) : null;
    const control = name ? document.getElementById(name) : null;
    control?.focus({ preventScroll: true });
  };
  window.setTimeout(() => attempt(MAX_ATTEMPTS), 0);
}
