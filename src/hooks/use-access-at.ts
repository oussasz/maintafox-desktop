/**
 * The current user's rights at one org node (step 6): plant-wide rights plus those held on the
 * node or above it. `can(permission)` answers for that node; the backend stays the authority.
 * Reloaded when roles change (`rbac-changed`).
 */
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

import { getMyAccessAt } from "@/services/rbac-service";

export interface AccessAt {
  loading: boolean;
  /** Whether the user holds `permission` at the node. */
  can: (permission: string) => boolean;
}

export function useAccessAt(orgNodeId: number | null | undefined): AccessAt {
  // null: every permission (administrator); undefined: not loaded (or no node).
  const [granted, setGranted] = useState<Set<string> | null | undefined>(undefined);
  const [loading, setLoading] = useState(false);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    // The listener may attach after unmount: detach it as soon as it arrives then.
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("rbac-changed", () => setVersion((v) => v + 1)).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    // Never answer for the previous node while the new one loads.
    setGranted(undefined);
    if (orgNodeId == null) return;
    let cancelled = false;
    setLoading(true);
    void getMyAccessAt(orgNodeId)
      .then((perms) => {
        if (!cancelled) setGranted(perms === null ? null : new Set(perms));
      })
      .catch(() => {
        if (!cancelled) setGranted(new Set());
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [orgNodeId, version]);

  const can = useCallback(
    (permission: string) => (granted === null ? true : (granted?.has(permission) ?? false)),
    [granted],
  );
  return { loading, can };
}
