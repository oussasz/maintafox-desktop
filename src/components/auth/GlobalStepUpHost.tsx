import { useCallback, useEffect, useRef, useState } from "react";

import { StepUpDialog } from "@/components/auth/StepUpDialog";
import { registerStepUpRequester } from "@/lib/step-up-bridge";
import { useSessionStore } from "@/store/session-store";

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
    return () => {
      registerStepUpRequester(null);
      // Never leave a caller waiting (or a store stuck on "saving") when the host goes away.
      settle(false);
    };
  }, [settle]);

  // A lock, sign-out or another user ends the request: queued commands are dropped, not
  // replayed under whoever types a password next.
  const userId = useSessionStore((s) => s.info?.user_id ?? null);
  const active = useSessionStore((s) => s.info?.is_authenticated === true && !s.info.is_locked);
  // biome-ignore lint/correctness/useExhaustiveDependencies: both values are the trigger.
  useEffect(() => {
    settle(false);
  }, [userId, active, settle]);

  return (
    <StepUpDialog open={open} onVerified={() => settle(true)} onCancel={() => settle(false)} />
  );
}
