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
  // node's rights (or rights from before a role change) while the next answer loads. Coming back
  // to a node at the same version reuses its answer while it refreshes: every role change bumps
  // the version first, so the same node and version means the same rights.
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

export interface AccessAtNodes {
  loading: boolean;
  /**
   * Whether, at EVERY one of the nodes, the user holds at least one of `permissions` (an empty
   * node list answers true: nothing to refuse).
   */
  canAllAny: (permissions: readonly string[]) => boolean;
}

/** A loaded answer for a set of nodes (keyed so a render never reads another set's answer). */
interface LoadedNodes {
  key: string;
  version: number;
  /** One entry per node; null: every permission (administrator). */
  granted: (Set<string> | null)[];
}

/**
 * The user's rights at several org nodes at once (bulk actions over rows of different sites).
 * Same source and refresh rules as `useAccessAt`; the backend stays the authority.
 */
export function useAccessAtNodes(orgNodeIds: readonly number[]): AccessAtNodes {
  const [loaded, setLoaded] = useState<LoadedNodes | undefined>(undefined);
  const [version, setVersion] = useState(0);
  const key = [...new Set(orgNodeIds)].sort((a, b) => a - b).join(",");

  useEffect(() => {
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
    const nodes = key === "" ? [] : key.split(",").map(Number);
    let cancelled = false;
    void Promise.all(
      nodes.map((node) =>
        getMyAccessAt(node)
          .then((perms) => (perms === null ? null : new Set(perms)))
          .catch(() => new Set<string>()),
      ),
    ).then((granted) => {
      if (!cancelled) setLoaded({ key, version, granted });
    });
    return () => {
      cancelled = true;
    };
  }, [key, version]);

  const current = loaded?.key === key && loaded.version === version ? loaded : undefined;
  const canAllAny = useCallback(
    (permissions: readonly string[]) =>
      current !== undefined &&
      current.granted.every((g) => g === null || permissions.some((p) => g.has(p))),
    [current],
  );
  return { loading: current === undefined, canAllAny };
}
