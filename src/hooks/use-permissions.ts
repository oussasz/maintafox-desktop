import { useContext } from "react";

import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";

/**
 * Hook that returns the centralized permission state from `<PermissionProvider>`.
 *
 * Phase 2 rewrite: delegates to PermissionContext instead of calling IPC independently.
 * All consumers share one permission set loaded once after authentication.
 *
 * Must be used within a `<PermissionProvider>` (placed inside `<AuthGuard>`).
 */
export function usePermissions(): PermissionContextValue {
  const context = useContext(PermissionContext);
  if (!context) {
    throw new Error("usePermissions must be used within <PermissionProvider>");
  }
  return context;
}
