import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  SYNC_WORKER_STATUS_EVENT,
  SyncWorkerStatusSchema,
  getSyncWorkerStatus,
} from "@/services/sync-service";
import type { SyncWorkerStatus } from "@shared/ipc-types";

import { type UserError, toUserError } from "../lib/user-error";

/** How often "il y a X min" is recomputed so it stays true while a screen stays open. */
export const FRESHNESS_TICK_MS = 60_000;

/**
 * The sync worker's status: read once when the screen opens, then kept current by the engine's
 * `sync-worker-status` event. One subscription per screen, released when it unmounts.
 */
export function useSyncStatus(): {
  /** Null until the first answer. */
  status: SyncWorkerStatus | null;
  error: UserError | null;
  refresh: () => Promise<void>;
} {
  const [status, setStatus] = useState<SyncWorkerStatus | null>(null);
  const [error, setError] = useState<UserError | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await getSyncWorkerStatus());
      setError(null);
    } catch (err) {
      setError(toUserError(err, "sync.status"));
    }
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void refresh();
    void listen<unknown>(SYNC_WORKER_STATUS_EVENT, (event) => {
      const parsed = SyncWorkerStatusSchema.safeParse(event.payload);
      if (parsed.success) {
        setStatus(parsed.data as SyncWorkerStatus);
        setError(null);
      } else {
        setError(toUserError(parsed.error, "sync.status_event"));
      }
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((err: unknown) => setError(toUserError(err, "sync.listen")));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refresh]);

  return { status, error, refresh };
}

/** The current time (ms), refreshed every `intervalMs`. */
export function useNow(intervalMs: number = FRESHNESS_TICK_MS): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

/**
 * Calls `onSynced` when a background sync finishes (`last_success_at` changes). The first status
 * the screen receives is the baseline, not a change: the screen has just loaded its own data.
 */
export function useAfterBackgroundSync(
  status: SyncWorkerStatus | null,
  onSynced: () => void,
): void {
  const baseline = useRef<{ at: string | null } | null>(null);
  const callback = useRef(onSynced);
  useEffect(() => {
    callback.current = onSynced;
  });
  const lastSuccess = status ? status.last_success_at : undefined;
  useEffect(() => {
    if (lastSuccess === undefined) return;
    if (baseline.current === null) {
      baseline.current = { at: lastSuccess };
      return;
    }
    if (baseline.current.at !== lastSuccess) {
      baseline.current = { at: lastSuccess };
      callback.current();
    }
  }, [lastSuccess]);
}
