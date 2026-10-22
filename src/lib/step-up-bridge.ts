/**
 * Bridge between `invoke` (plain module) and the step-up password dialog (React).
 *
 * Callers that wrap their command in `useStepUp().withStepUp` handle a `STEP_UP_REQUIRED`
 * refusal themselves. For every other caller, `invoke` asks this bridge to show the
 * password dialog once and then retries the command, so a screen that forgot the wrapper
 * can never end up with a silent "nothing happens" (audit AU-07, AU-53).
 */

/** Message of the error thrown when the user closes the step-up dialog without confirming. */
export const STEP_UP_CANCELLED_MESSAGE = "Step-up cancelled by user";

type StepUpRequester = () => Promise<boolean>;

let requester: StepUpRequester | null = null;
let localHandlers = 0;

/** The global dialog host registers itself here while it is mounted. */
export function registerStepUpRequester(fn: StepUpRequester | null): void {
  requester = fn;
}

/** True when a global dialog host is mounted and able to ask for the password. */
export function hasStepUpRequester(): boolean {
  return requester !== null;
}

/** Marks the duration of a `withStepUp` call: its own dialog handles the refusal then. */
export async function trackLocalStepUp<T>(run: () => Promise<T>): Promise<T> {
  localHandlers += 1;
  try {
    return await run();
  } finally {
    localHandlers -= 1;
  }
}

export function isLocalStepUpActive(): boolean {
  return localHandlers > 0;
}

/** Asks for the password; resolves true when verified, false when the user cancelled. */
export function requestGlobalStepUp(): Promise<boolean> {
  return requester ? requester() : Promise.resolve(false);
}
