import { listen } from "@tauri-apps/api/event";
import {
  type ReactNode,
  createContext,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Outlet } from "react-router-dom";

import { useSession } from "@/hooks/use-session";
import { readPermissionCache, writePermissionCache } from "@/lib/permission-cache";
import { getMyPermissions } from "@/services/rbac-service";
import type { PermissionRecord } from "@shared/ipc-types";
import { type PermissionName } from "@shared/rbac/permissions.generated";

// ── Context value ─────────────────────────────────────────────────────────

export interface PermissionContextValue {
  permissions: PermissionRecord[];
  isLoading: boolean;
  can: (permissionName: PermissionName) => boolean;
  canAny: (...permissionNames: PermissionName[]) => boolean;
  canAll: (...permissionNames: PermissionName[]) => boolean;
  refresh: () => Promise<void>;
}

export const PermissionContext = createContext<PermissionContextValue | null>(null);

// ── Provider ──────────────────────────────────────────────────────────────

interface PermissionProviderProps {
  children?: ReactNode;
}

/**
 * Central permission provider. Uses the shared session store (same SSOT as
 * AuthGuard) and a process-lifetime cache so remounts / transient AUTH_ERROR
 * never wipe the Sidebar down to ungated items only.
 *
 * Cache clear is owned by logout paths only (session store / AuthGuard), never
 * by transient unauthenticated flips during AuthLock.
 */
export function PermissionProvider({ children }: PermissionProviderProps) {
  const session = useSession();
  const authenticated = session.info?.is_authenticated === true;
  const userId = session.info?.user_id ?? null;

  const [permissions, setPermissions] = useState<PermissionRecord[]>(() =>
    readPermissionCache(userId),
  );
  const [isLoading, setIsLoading] = useState(true);
  const loadGeneration = useRef(0);

  const load = useCallback(async () => {
    const generation = ++loadGeneration.current;

    if (!authenticated) {
      // Wait for authentication — seed from cache, never wipe here.
      const cached = readPermissionCache(userId);
      if (generation === loadGeneration.current) {
        if (cached.length > 0) {
          setPermissions(cached);
        }
        setIsLoading(false);
      }
      return;
    }

    setIsLoading(true);
    try {
      const perms = await getMyPermissions();
      if (generation !== loadGeneration.current) {
        return;
      }
      writePermissionCache(userId, perms);
      setPermissions(perms);
    } catch {
      if (generation !== loadGeneration.current) {
        return;
      }
      // Any failure: keep last known / cache. Never collapse nav to ungated-only.
      const cached = readPermissionCache(userId);
      setPermissions((prev) => (prev.length > 0 ? prev : cached));
    } finally {
      if (generation === loadGeneration.current) {
        setIsLoading(false);
      }
    }
  }, [authenticated, userId]);

  // Load whenever auth presence / user changes.
  useEffect(() => {
    void load();
  }, [load]);

  // Listen for rbac-changed event (emitted on role/assignment mutations)
  useEffect(() => {
    const unlisten = listen("rbac-changed", () => {
      void load();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [load]);

  // Listen for session-unlocked event (permissions may have changed while locked)
  useEffect(() => {
    const unlisten = listen("session-unlocked", () => {
      void load();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [load]);

  // Memoized permission check helpers
  const nameSet = useMemo(() => new Set(permissions.map((p) => p.name)), [permissions]);

  const can = useCallback(
    (permissionName: PermissionName) => nameSet.has(permissionName),
    [nameSet],
  );

  const canAny = useCallback(
    (...permissionNames: PermissionName[]) => permissionNames.some((n) => nameSet.has(n)),
    [nameSet],
  );

  const canAll = useCallback(
    (...permissionNames: PermissionName[]) => permissionNames.every((n) => nameSet.has(n)),
    [nameSet],
  );

  const value = useMemo<PermissionContextValue>(
    () => ({ permissions, isLoading, can, canAny, canAll, refresh: load }),
    [permissions, isLoading, can, canAny, canAll, load],
  );

  return (
    <PermissionContext.Provider value={value}>{children ?? <Outlet />}</PermissionContext.Provider>
  );
}
