import { type ReactElement, useCallback, useRef, useState } from "react";

import type { StepUpDialog } from "@/components/auth/StepUpDialog";

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
          // Check if the error indicates step-up is required
          const isStepUpRequired =
            err instanceof Error &&
            (err.message.includes("STEP_UP_REQUIRED") ||
              err.message.includes("step_up") ||
              err.message.includes("StepUpRequired"));

          if (isStepUpRequired) {
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
    pending.reject(new Error("Step-up cancelled by user"));
  }, []);

  const StepUpDialogElement = (
    <StepUpDialog open={open} onVerified={handleVerified} onCancel={handleCancel} />
  );

  return { withStepUp, StepUpDialogElement };
}
