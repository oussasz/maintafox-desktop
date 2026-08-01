import type { PermissionRecord } from "@shared/ipc-types";

/**
 * Process-lifetime cache of the last successfully loaded permission set.
 * Survives PermissionProvider remounts (HMR, auth-lock branch flips) so the
 * Sidebar never collapses to ungated items while a reload is in flight or fails transiently.
 */
let cachedUserId: number | null = null;
let cachedPermissions: PermissionRecord[] = [];

export function readPermissionCache(userId: number | null | undefined): PermissionRecord[] {
  if (userId == null) {
    return cachedPermissions.length > 0 ? [...cachedPermissions] : [];
  }
  if (cachedUserId !== null && cachedUserId !== userId) {
    return [];
  }
  return cachedPermissions.length > 0 ? [...cachedPermissions] : [];
}

export function writePermissionCache(userId: number | null, permissions: PermissionRecord[]): void {
  if (permissions.length === 0) {
    return;
  }
  cachedUserId = userId;
  cachedPermissions = [...permissions];
}

export function clearPermissionCache(): void {
  cachedUserId = null;
  cachedPermissions = [];
}

/** Test helper. */
export function resetPermissionCacheForTests(): void {
  clearPermissionCache();
}
