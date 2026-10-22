import { useCallback, useEffect, useRef, useState } from "react";

import { StepUpDialog } from "@/components/auth/StepUpDialog";
import { registerStepUpRequester } from "@/lib/step-up-bridge";

/**
 * Mounted once under the shell. Shows the password dialog for any command that was refused
 * with `STEP_UP_REQUIRED` and whose caller has no `withStepUp` of its own (see `ipc-invoke`).
 */
export function GlobalStepUpHost() {
  const [open, setOpen] = useState(false);
  const waitingRef = useRef<((verified: boolean) => void)[]>([]);

  const settle = useCallback((verified: boolean) => {
    setOpen(false);
    const waiting = waitingRef.current;
    waitingRef.current = [];
    for (const resolve of waiting) resolve(verified);
  }, []);

  useEffect(() => {
    registerStepUpRequester(
      () =>
        new Promise<boolean>((resolve) => {
          // Several refused commands at once share one dialog.
          waitingRef.current.push(resolve);
          setOpen(true);
        }),
    );
    return () => registerStepUpRequester(null);
  }, []);

  return (
    <StepUpDialog open={open} onVerified={() => settle(true)} onCancel={() => settle(false)} />
  );
}
