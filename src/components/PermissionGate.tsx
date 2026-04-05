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
  permission: string;
  children: ReactNode;
  fallback?: ReactNode;
}

export function PermissionGate({
  permission,
  children,
  fallback = null,
}: PermissionGateProps): ReactElement | null {
  const { can, isLoading } = usePermissions();

  if (isLoading) return null;

  return can(permission) ? <>{children}</> : <>{fallback}</>;
}
