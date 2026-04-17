import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useRef, useCallback } from "react";

const POLL_INTERVAL_MS = 30_000;

/**
 * Polls the `get_unread_notification_count` IPC command every 30 seconds.
 *
 * If the command is not yet implemented (Phase 2 SP07), the hook catches the
 * error silently and returns 0. This allows the TopBar to use the hook now
 * without blocking on the notification backend.
 */
export function useNotificationCount(): number {
  const [count, setCount] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const poll = useCallback(async () => {
    try {
      const result = await invoke<number>("get_unread_notification_count");
      setCount(result);
    } catch {
      // Command not yet available — silent fallback to 0
      setCount(0);
    }
  }, []);

  useEffect(() => {
    // Initial fetch
    void poll();

    // Periodic polling
    timerRef.current = setInterval(() => void poll(), POLL_INTERVAL_MS);

    return () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
      }
    };
  }, [poll]);

  return count;
}
