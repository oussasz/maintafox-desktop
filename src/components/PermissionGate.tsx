// Conditionally renders children if the user has the required permission.
// On loading, renders nothing (no flash of unauthorized content).
//
// Usage:
//   <PermissionGate permission={P.EQ_MANAGE}>
//     <EditEquipmentButton />
//   </PermissionGate>
//
//   <PermissionGate permission={P.ADM_USERS} fallback={<NotAuthorized />}>
//     <UserManagementPanel />
//   </PermissionGate>

import type { ReactNode, ReactElement } from "react";

import { usePermissions } from "@/hooks/use-permissions";
import { type PermissionName } from "@shared/rbac/permissions.generated";

interface PermissionGateProps {
  /** Single permission name (use this or `anyOf`, not both). */
  permission?: PermissionName;
  /** User must have at least one of these permissions. */
  anyOf?: PermissionName[];
  children: ReactNode;
  fallback?: ReactNode;
}

export function PermissionGate({
  permission,
  anyOf,
  children,
  fallback = null,
}: PermissionGateProps): ReactElement | null {
  const { can, canAny, isLoading } = usePermissions();

  if (isLoading) return null;

  let allowed = false;
  if (anyOf && anyOf.length > 0) {
    allowed = canAny(...anyOf);
  } else if (permission) {
    allowed = can(permission);
  }

  return allowed ? <>{children}</> : <>{fallback}</>;
}
