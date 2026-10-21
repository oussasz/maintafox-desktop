import { type ReactElement, useCallback, useRef, useState } from "react";

import { StepUpDialog } from "@/components/auth/StepUpDialog";
import { isStepUpRequiredError } from "@/utils/errors";

/** Message of the error `withStepUp` rejects with when the user closes the step-up dialog. */
export const STEP_UP_CANCELLED_MESSAGE = "Step-up cancelled by user";

/** The user closed the step-up dialog (nothing was sent): no refusal to show. */
export function isStepUpCancelled(err: unknown): boolean {
  return err instanceof Error && err.message === STEP_UP_CANCELLED_MESSAGE;
}

interface UseStepUpReturn {
  /** Wraps an async action that may need step-up. Shows dialog if backend returns StepUpRequired. */
  withStepUp: <T>(action: () => Promise<T>) => Promise<T>;
  /** The dialog element to render (place once in page layout) */
  StepUpDialogElement: ReactElement;
}

/**
 * Hook that transparently handles step-up re-authentication.
 *
 * Usage:
 *   const { withStepUp, StepUpDialogElement } = useStepUp();
 *   const handleDelete = () => withStepUp(() => deleteRole(id));
 *   // StepUpDialogElement must be rendered in the component tree.
 */
export function useStepUp(): UseStepUpReturn {
  const [open, setOpen] = useState(false);
  const pendingRef = useRef<{
    action: () => Promise<unknown>;
    resolve: (value: unknown) => void;
    reject: (reason: unknown) => void;
  } | null>(null);

  const withStepUp = useCallback(<T,>(action: () => Promise<T>): Promise<T> => {
    return new Promise<T>((resolve, reject) => {
      // Try the action directly first
      action()
        .then(resolve)
        .catch((err: unknown) => {
          if (isStepUpRequiredError(err)) {
            // Store pending action and open dialog
            pendingRef.current = {
              action: action as () => Promise<unknown>,
              resolve: resolve as (v: unknown) => void,
              reject,
            };
            setOpen(true);
          } else {
            reject(err);
          }
        });
    });
  }, []);

  const handleVerified = useCallback(() => {
    setOpen(false);
    const pending = pendingRef.current;
    if (!pending) return;
    pendingRef.current = null;

    // Retry the action after successful step-up
    pending.action().then(pending.resolve).catch(pending.reject);
  }, []);

  const handleCancel = useCallback(() => {
    setOpen(false);
    const pending = pendingRef.current;
    if (!pending) return;
    pendingRef.current = null;
    pending.reject(new Error(STEP_UP_CANCELLED_MESSAGE));
  }, []);

  const StepUpDialogElement = (
    <StepUpDialog open={open} onVerified={handleVerified} onCancel={handleCancel} />
  );

  return { withStepUp, StepUpDialogElement };
}
