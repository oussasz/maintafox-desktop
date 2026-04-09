import type { ReactNode } from "react";
import { Outlet } from "react-router-dom";

import { usePermissions } from "@/hooks/use-permissions";

import { UnauthorizedPage } from "../../pages/UnauthorizedPage";

interface PermissionRouteProps {
  /** Single permission check */
  permission?: string;
  /** Pass if the user has ANY of these permissions */
  anyOf?: string[];
  /** Pass only if the user has ALL of these permissions */
  allOf?: string[];
  /** Custom fallback (defaults to UnauthorizedPage) */
  fallback?: ReactNode;
}

/**
 * Route-level permission guard. Wraps a `<Route>` segment and renders
 * `<Outlet />` only if the permission check passes.
 *
 * Usage in router.tsx:
 *   <Route element={<PermissionRoute permission="eq.view" />}>
 *     <Route path="equipment" element={<EquipmentPage />} />
 *   </Route>
 */
export function PermissionRoute({ permission, anyOf, allOf, fallback }: PermissionRouteProps) {
  const { can, canAny, canAll, isLoading } = usePermissions();

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  let allowed = false;

  if (permission) {
    allowed = can(permission);
  } else if (anyOf && anyOf.length > 0) {
    allowed = canAny(...anyOf);
  } else if (allOf && allOf.length > 0) {
    allowed = canAll(...allOf);
  } else {
    // No permission constraint specified — allow through
    allowed = true;
  }

  if (!allowed) {
    return <>{fallback ?? <UnauthorizedPage />}</>;
  }

  return <Outlet />;
}
