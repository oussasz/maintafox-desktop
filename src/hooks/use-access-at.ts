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

/** A loaded answer, tagged with the node and reload it was loaded for. */
interface Loaded {
  node: number;
  version: number;
  /** null: every permission (administrator). */
  granted: Set<string> | null;
}

export function useAccessAt(orgNodeId: number | null | undefined): AccessAt {
  const [loaded, setLoaded] = useState<Loaded | undefined>(undefined);
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
    if (orgNodeId == null) return;
    let cancelled = false;
    const done = (granted: Set<string> | null) => {
      if (!cancelled) setLoaded({ node: orgNodeId, version, granted });
    };
    void getMyAccessAt(orgNodeId)
      .then((perms) => done(perms === null ? null : new Set(perms)))
      .catch(() => done(new Set()));
    return () => {
      cancelled = true;
    };
  }, [orgNodeId, version]);

  // Only an answer for this very node and reload counts, so a render never shows the previous
  // node's rights (or rights from before a role change) while the next answer loads.
  const current =
    orgNodeId != null && loaded?.node === orgNodeId && loaded.version === version
      ? loaded
      : undefined;
  const can = useCallback(
    (permission: string) =>
      current === undefined ? false : current.granted === null || current.granted.has(permission),
    [current],
  );
  return { loading: orgNodeId != null && current === undefined, can };
}
