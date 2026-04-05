import { useState, useEffect, useCallback } from "react";

import { getMyPermissions } from "@/services/rbac-service";
import type { PermissionRecord } from "@shared/ipc-types";

interface UsePermissionsReturn {
  /** Full list of permissions the user holds */
  permissions: PermissionRecord[];
  /** True while loading */
  isLoading: boolean;
  /** Returns true if the user holds the given permission name */
  can: (permissionName: string) => boolean;
  /** Reload permissions from the backend (call after role change) */
  refresh: () => Promise<void>;
}

/**
 * Preloads the current user's effective permissions after login.
 * Provides a `can(name)` helper for inline checks and feeds
 * `<PermissionGate>` for declarative rendering.
 *
 * Phase 1: each hook instance fires its own IPC call.
 * Phase 2 will introduce a PermissionProvider context to deduplicate.
 */
export function usePermissions(): UsePermissionsReturn {
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

  useEffect(() => {
    void load();
  }, [load]);

  const can = useCallback(
    (permissionName: string) => permissions.some((p) => p.name === permissionName),
    [permissions],
  );

  return { permissions, isLoading, can, refresh: load };
}
