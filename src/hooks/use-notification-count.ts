import { useState, useEffect, useRef, useCallback } from "react";

import { invokeSilent } from "@/lib/ipc-invoke";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import { useSessionStore } from "@/store/session-store";

const POLL_INTERVAL_MS = 30_000;

/**
 * Polls the `get_unread_count` IPC command every 30 seconds.
 *
 * Uses invokeSilent so AUTH/SESSION failures never open AuthLockLayer.
 * Polling pauses while the session is locked or absent.
 */
export function useNotificationCount(): number {
  const [count, setCount] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const authenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const locked = useSessionStore((s) => s.info?.is_locked === true);

  const poll = useCallback(async () => {
    if (!isSessionActiveForBackgroundWork()) {
      setCount(0);
      return;
    }
    try {
      const result = await invokeSilent<number>("get_unread_count");
      setCount(result);
    } catch {
      setCount(0);
    }
  }, []);

  useEffect(() => {
    if (!authenticated || locked) {
      setCount(0);
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
      return;
    }

    void poll();
    timerRef.current = setInterval(() => void poll(), POLL_INTERVAL_MS);

    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
      }
    };
  }, [authenticated, locked, poll]);

  return count;
}
