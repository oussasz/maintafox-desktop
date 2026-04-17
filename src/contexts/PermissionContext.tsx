import { listen } from "@tauri-apps/api/event";
import { type ReactNode, createContext, useCallback, useEffect, useMemo, useState } from "react";
import { Outlet } from "react-router-dom";

import { getMyPermissions } from "@/services/rbac-service";
import type { PermissionRecord } from "@shared/ipc-types";

// ── Context value ─────────────────────────────────────────────────────────

export interface PermissionContextValue {
  permissions: PermissionRecord[];
  isLoading: boolean;
  can: (permissionName: string) => boolean;
  canAny: (...permissionNames: string[]) => boolean;
  canAll: (...permissionNames: string[]) => boolean;
  refresh: () => Promise<void>;
}

export const PermissionContext = createContext<PermissionContextValue | null>(null);

// ── Provider ──────────────────────────────────────────────────────────────

interface PermissionProviderProps {
  children?: ReactNode;
}

/**
 * Central permission provider. Loads permissions once after authentication,
 * listens for `rbac-changed` and `session-unlocked` events to auto-refresh.
 *
 * Place inside `<AuthGuard>` so it only mounts when the user is authenticated.
 * All `usePermissions()` consumers share this single permission set.
 */
export function PermissionProvider({ children }: PermissionProviderProps) {
  const [permissions, setPermissions] = useState<PermissionRecord[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const load = useCallback(async () => {
    setIsLoading(true);
    try {
      const perms = await getMyPermissions();
      setPermissions(perms);
    } catch {
      setPermissions([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  // Initial load
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

  const can = useCallback((permissionName: string) => nameSet.has(permissionName), [nameSet]);

  const canAny = useCallback(
    (...permissionNames: string[]) => permissionNames.some((n) => nameSet.has(n)),
    [nameSet],
  );

  const canAll = useCallback(
    (...permissionNames: string[]) => permissionNames.every((n) => nameSet.has(n)),
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
