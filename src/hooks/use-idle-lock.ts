import { useEffect } from "react";

import { confirmSessionAfterSilentFailure } from "@/lib/ipc-invoke";
import { getSessionInfo, touchSession } from "@/services/auth-service";
import { useSessionStore } from "@/store/session-store";

/** Real user input in the app window: only these keep the session from idle-locking. */
export const USER_INPUT_EVENTS = [
  "pointerdown",
  "pointermove",
  "keydown",
  "wheel",
  "touchstart",
] as const;

/** At most one activity report a minute (more often when the idle timeout is very short). */
export const TOUCH_THROTTLE_MAX_MS = 60_000;

/** The lock check runs this long after the idle deadline (the server decides at the deadline). */
export const IDLE_CHECK_GRACE_MS = 1_000;

/** Time between two activity reports: a quarter of the idle timeout, at most a minute. */
export function touchThrottleMs(idleMs: number): number {
  return Math.min(TOUCH_THROTTLE_MAX_MS, Math.max(1_000, Math.floor(idleMs / 4)));
}

/**
 * Idle lock of the open app (AU-34). Idle means no keyboard, mouse or touch input in the app
 * window, whatever background polls run:
 * - real input sends `touch_session` (the only call that counts as activity): at once on the
 *   first input after a quiet spell, then at most once per throttle interval (a later input in
 *   the interval is reported at its end);
 * - when the idle deadline (the session's `last_activity_at` plus its `idle_timeout_minutes`,
 *   from the session info) passes, the session is read again (`get_session_info`, which does not
 *   count as activity): the server reports it locked and the lock screen shows, without waiting
 *   for some other call to fail. It is checked again when the window comes back into view (a PC
 *   waking from sleep).
 * The server stays the authority: a refused report (`SESSION_LOCKED`) re-reads the session too.
 */
export function useIdleLock(): void {
  const active = useSessionStore((s) => s.info?.is_authenticated === true && !s.info.is_locked);
  const idleMinutes = useSessionStore((s) => s.info?.idle_timeout_minutes ?? null);
  const lastActivityAt = useSessionStore((s) => s.info?.last_activity_at ?? null);

  // Real input → activity report.
  useEffect(() => {
    if (!active || idleMinutes === null || idleMinutes <= 0) return;
    const throttleMs = touchThrottleMs(idleMinutes * 60_000);
    let lastSentAt = Number.NEGATIVE_INFINITY;
    let trailing: number | null = null;
    let stopped = false;

    const send = () => {
      lastSentAt = Date.now();
      touchSession()
        .then((info) => {
          if (!stopped) useSessionStore.getState().applySessionInfo(info);
        })
        .catch((err: unknown) => {
          // Locked past its deadline, or ended: the re-read session shows the right screen.
          if (!stopped) void confirmSessionAfterSilentFailure(err);
        });
    };

    const onInput = () => {
      const since = Date.now() - lastSentAt;
      if (since >= throttleMs) {
        if (trailing !== null) {
          window.clearTimeout(trailing);
          trailing = null;
        }
        send();
        return;
      }
      if (trailing === null) {
        trailing = window.setTimeout(() => {
          trailing = null;
          send();
        }, throttleMs - since);
      }
    };

    const options: AddEventListenerOptions = { capture: true, passive: true };
    for (const type of USER_INPUT_EVENTS) window.addEventListener(type, onInput, options);
    return () => {
      stopped = true;
      for (const type of USER_INPUT_EVENTS) window.removeEventListener(type, onInput, options);
      if (trailing !== null) window.clearTimeout(trailing);
    };
  }, [active, idleMinutes]);

  // Idle deadline → the session is read again; the server says it is locked.
  useEffect(() => {
    if (!active || idleMinutes === null || idleMinutes <= 0 || !lastActivityAt) return;
    const last = Date.parse(lastActivityAt);
    if (Number.isNaN(last)) return;
    const deadline = last + idleMinutes * 60_000;
    let stopped = false;
    let timer: number | null = null;

    const schedule = (delayMs: number) => {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(check, delayMs);
    };

    function check() {
      if (timer !== null) window.clearTimeout(timer);
      timer = null;
      getSessionInfo()
        .then((info) => {
          if (stopped) return;
          useSessionStore.getState().applySessionInfo(info);
          // Still open with the same activity time (should not happen): look again shortly.
          if (
            info.is_authenticated &&
            !info.is_locked &&
            info.last_activity_at === lastActivityAt
          ) {
            schedule(IDLE_CHECK_GRACE_MS * 5);
          }
        })
        .catch(() => {
          if (!stopped) schedule(IDLE_CHECK_GRACE_MS * 5);
        });
    }

    const onBackInView = () => {
      if (document.visibilityState === "visible" && Date.now() >= deadline + IDLE_CHECK_GRACE_MS) {
        check();
      }
    };

    schedule(Math.max(0, deadline - Date.now()) + IDLE_CHECK_GRACE_MS);
    window.addEventListener("focus", onBackInView);
    document.addEventListener("visibilitychange", onBackInView);
    return () => {
      stopped = true;
      if (timer !== null) window.clearTimeout(timer);
      window.removeEventListener("focus", onBackInView);
      document.removeEventListener("visibilitychange", onBackInView);
    };
  }, [active, idleMinutes, lastActivityAt]);
}
