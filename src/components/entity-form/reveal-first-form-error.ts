/**
 * Brings the first error of a long form into view once it has rendered: scrolls it to the
 * middle of the dialog and puts the keyboard focus on its control. Field errors are the
 * `role="alert"` lines that `FormField` renders under each control (id `<name>-error`, control
 * id `<name>`); a form-level alert with no control is only scrolled to.
 */
export function revealFirstFormError(formId: string): void {
  // Errors appear on the next render, after the submit handler returns.
  window.setTimeout(() => {
    const form = document.getElementById(formId);
    const alert = form?.querySelector<HTMLElement>('[role="alert"]');
    if (!alert) return;
    alert.scrollIntoView?.({ block: "center" });
    const name = alert.id.endsWith("-error") ? alert.id.slice(0, -"-error".length) : null;
    const control = name ? document.getElementById(name) : null;
    control?.focus({ preventScroll: true });
  }, 0);
}
