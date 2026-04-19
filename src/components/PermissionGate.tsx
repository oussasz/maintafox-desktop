// Conditionally renders children if the user has the required permission.
// On loading, renders nothing (no flash of unauthorized content).
//
// Usage:
//   <PermissionGate permission="eq.manage">
//     <EditEquipmentButton />
//   </PermissionGate>
//
//   <PermissionGate permission="adm.users" fallback={<NotAuthorized />}>
//     <UserManagementPanel />
//   </PermissionGate>

import type { ReactNode, ReactElement } from "react";

import { usePermissions } from "@/hooks/use-permissions";

interface PermissionGateProps {
  /** Single permission name (use this or `anyOf`, not both). */
  permission?: string;
  /** User must have at least one of these permissions. */
  anyOf?: string[];
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
