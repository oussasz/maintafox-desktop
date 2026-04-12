import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

import { getUserPresence } from "@/services/rbac-service";
import type { UserPresence } from "@shared/ipc-types";

// ── Presence cache (module-level, shared across all instances) ────────────

const CACHE_TTL_MS = 30_000;
const cachedPresence: Map<number, UserPresence> = new Map();
let cacheTimestamp = 0;
let pendingFetch: Promise<void> | null = null;

async function fetchPresence(userIds: number[]): Promise<void> {
  const now = Date.now();
  // Don't refetch if cache is fresh
  if (now - cacheTimestamp < CACHE_TTL_MS && userIds.every((id) => cachedPresence.has(id))) {
    return;
  }
  // Deduplicate parallel requests
  if (pendingFetch) return pendingFetch;

  pendingFetch = (async () => {
    try {
      const results = await getUserPresence(userIds);
      for (const p of results) {
        cachedPresence.set(p.user_id, p);
      }
      cacheTimestamp = Date.now();
    } catch {
      // IPC failure — leave cache stale, will retry on next interval
    } finally {
      pendingFetch = null;
    }
  })();

  return pendingFetch;
}

// ── Component ─────────────────────────────────────────────────────────────

interface OnlinePresenceIndicatorProps {
  userId: number;
  size?: "sm" | "md";
}

const SIZE_MAP = {
  sm: "h-2.5 w-2.5",
  md: "h-3 w-3",
};

const STATUS_COLORS: Record<string, string> = {
  active: "bg-emerald-500",
  idle: "bg-amber-400",
  offline: "bg-gray-300",
};

/**
 * Shows a green/amber/gray presence dot for a user.
 * Batch-fetches presence data with a 30-second shared cache.
 */
export function OnlinePresenceIndicator({ userId, size = "sm" }: OnlinePresenceIndicatorProps) {
  const [status, setStatus] = useState<string>("offline");
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    const update = async () => {
      // Touch current user's session so presence stays fresh
      invoke("touch_session").catch(() => {});
      await fetchPresence([userId]);
      const cached = cachedPresence.get(userId);
      setStatus(cached?.status ?? "offline");
    };

    void update();
    intervalRef.current = setInterval(() => void update(), CACHE_TTL_MS);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [userId]);

  return (
    <span
      className={`inline-block shrink-0 rounded-full ${SIZE_MAP[size]} ${STATUS_COLORS[status] ?? STATUS_COLORS["offline"]}`}
      role="status"
      aria-label={status}
    />
  );
}
