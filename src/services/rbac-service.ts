// ADR-003 compliant: all invoke() calls for RBAC go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for permission operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  AdminStatsPayload,
  AssignRoleScopeInput,
  CreateCustomPermissionInput,
  CreateRoleInput,
  CreateUserInput,
  GrantEmergencyElevationInput,
  IdPayload,
  PasswordPolicySettings,
  PermissionDependencyRow,
  PermissionListFilter,
  PermissionRecord,
  PermissionWithSystem,
  RbacSettingEntry,
  RevokeEmergencyElevationInput,
  RoleDetail,
  RoleTemplate,
  RoleValidationResult,
  RoleWithPermissions,
  SimulateAccessInput,
  SimulateAccessResult,
  StepUpRequest,
  StepUpResponse,
  UpdateRoleInput,
  UpdateUserInput,
  UserDetail,
  UserListFilter,
  UserPresence,
  UserWithRoles,
} from "@shared/ipc-types";

// ── Zod schemas for runtime validation ────────────────────────────────────────

export const PermissionRecordSchema = z.object({
  name: z.string(),
  description: z.string(),
  category: z.string(),
  is_dangerous: z.boolean(),
  requires_step_up: z.boolean(),
});

export const StepUpResponseSchema = z.object({
  success: z.boolean(),
  expires_at: z.string(),
});

// ── Service functions ─────────────────────────────────────────────────────────

/**
 * Fetch the effective permission set for the currently authenticated user.
 * Called after login to populate the `usePermissions` hook so that
 * `<PermissionGate>` can work without round-tripping for every check.
 */
export async function getMyPermissions(): Promise<PermissionRecord[]> {
  const raw = await invoke<unknown[]>("get_my_permissions");
  return z.array(PermissionRecordSchema).parse(raw);
}

/**
 * Verify the user's password for a dangerous-action step-up.
 * On success, the backend records a 120-second verification window.
 * Throws on wrong password (AUTH_ERROR) or no session (AUTH_ERROR).
 */
export async function verifyStepUp(password: string): Promise<StepUpResponse> {
  const payload: StepUpRequest = { password };
  const raw = await invoke<unknown>("verify_step_up", { payload });
  return StepUpResponseSchema.parse(raw);
}

// ── Admin stats ───────────────────────────────────────────────────────────

const AdminStatsSchema = z.object({
  active_users: z.number(),
  inactive_users: z.number(),
  total_roles: z.number(),
  system_roles: z.number(),
  custom_roles: z.number(),
  active_sessions: z.number(),
  unassigned_users: z.number(),
  emergency_grants_active: z.number(),
});

export async function getAdminStats(): Promise<AdminStatsPayload> {
  const raw = await invoke<unknown>("get_admin_stats");
  return AdminStatsSchema.parse(raw);
}

// ── User presence ─────────────────────────────────────────────────────────

const UserPresenceSchema = z.object({
  user_id: z.number(),
  status: z.enum(["active", "idle", "offline"]),
  last_activity_at: z.string().nullable(),
});

export async function getUserPresence(userIds: number[]): Promise<UserPresence[]> {
  const raw = await invoke<unknown[]>("get_user_presence", { user_ids: userIds });
  return z.array(UserPresenceSchema).parse(raw);
}

// ── User management ───────────────────────────────────────────────────────

const IdPayloadSchema = z.object({ id: z.number() });

const RoleAssignmentSummarySchema = z.object({
  assignment_id: z.number(),
  role_id: z.number(),
  role_name: z.string(),
  scope_type: z.string(),
  scope_reference: z.string().nullable(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  is_emergency: z.boolean(),
});

const UserWithRolesSchema = z.object({
  id: z.number(),
  username: z.string(),
  display_name: z.string().nullable(),
  identity_mode: z.string(),
  is_active: z.boolean(),
  force_password_change: z.boolean(),
  last_seen_at: z.string().nullable(),
  roles: z.array(RoleAssignmentSummarySchema),
});

const UserScopeAssignmentSchema = z.object({
  id: z.number(),
  user_id: z.number(),
  role_id: z.number(),
  scope_type: z.string(),
  scope_reference: z.string().nullable(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  assigned_by_id: z.number().nullable(),
  notes: z.string().nullable(),
  is_emergency: z.boolean(),
  emergency_reason: z.string().nullable(),
  emergency_expires_at: z.string().nullable(),
  created_at: z.string(),
  deleted_at: z.string().nullable(),
});

const UserDetailSchema = z.object({
  user: UserWithRolesSchema,
  scope_assignments: z.array(UserScopeAssignmentSchema),
  effective_permissions: z.array(z.string()),
});

export async function listUsers(filter: UserListFilter): Promise<UserWithRoles[]> {
  const raw = await invoke<unknown[]>("list_users", { filter });
  return z.array(UserWithRolesSchema).parse(raw);
}

export async function getUser(userId: number): Promise<UserDetail> {
  const raw = await invoke<unknown>("get_user", { user_id: userId });
  return UserDetailSchema.parse(raw);
}

export async function createUser(input: CreateUserInput): Promise<IdPayload> {
  const raw = await invoke<unknown>("create_user", { input });
  return IdPayloadSchema.parse(raw);
}

export async function updateUser(input: UpdateUserInput): Promise<void> {
  await invoke<void>("update_user", { input });
}

export async function deactivateUser(userId: number): Promise<void> {
  await invoke<void>("deactivate_user", { user_id: userId });
}

export async function assignRoleScope(input: AssignRoleScopeInput): Promise<IdPayload> {
  const raw = await invoke<unknown>("assign_role_scope", { input });
  return IdPayloadSchema.parse(raw);
}

export async function revokeRoleScope(assignmentId: number): Promise<void> {
  await invoke<void>("revoke_role_scope", { assignment_id: assignmentId });
}

// ── Role management ───────────────────────────────────────────────────────

const RoleWithPermissionsSchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().nullable(),
  role_type: z.string(),
  status: z.string(),
  is_system: z.boolean(),
  permissions: z.array(z.string()),
});

const RoleDetailSchema = z.object({
  role: RoleWithPermissionsSchema,
  dependency_warnings: z.array(z.string()),
});

const RoleTemplateSchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().nullable(),
  module_set_json: z.string(),
  is_system: z.boolean(),
});

const SimulateAccessResultSchema = z.object({
  permissions: z.record(z.string(), z.boolean()),
  assignments: z.array(UserScopeAssignmentSchema),
  dependency_warnings: z.array(z.string()),
  blocked_by: z.array(z.string()),
});

export async function listRoles(): Promise<RoleWithPermissions[]> {
  const raw = await invoke<unknown[]>("list_roles");
  return z.array(RoleWithPermissionsSchema).parse(raw);
}

export async function getRole(roleId: number): Promise<RoleDetail> {
  const raw = await invoke<unknown>("get_role", { role_id: roleId });
  return RoleDetailSchema.parse(raw);
}

export async function createRole(input: CreateRoleInput): Promise<IdPayload> {
  const raw = await invoke<unknown>("create_role", { input });
  return IdPayloadSchema.parse(raw);
}

export async function updateRole(input: UpdateRoleInput): Promise<void> {
  await invoke<void>("update_role", { input });
}

export async function deleteRole(roleId: number): Promise<void> {
  await invoke<void>("delete_role", { role_id: roleId });
}

export async function listRoleTemplates(): Promise<RoleTemplate[]> {
  const raw = await invoke<unknown[]>("list_role_templates");
  return z.array(RoleTemplateSchema).parse(raw);
}

export async function simulateAccess(input: SimulateAccessInput): Promise<SimulateAccessResult> {
  const raw = await invoke<unknown>("simulate_access", { input });
  return SimulateAccessResultSchema.parse(raw);
}

export async function unlockUserAccount(userId: number): Promise<void> {
  await invoke<void>("unlock_user_account", { user_id: userId });
}

// ── RBAC settings (password policy, lockout) ──────────────────────────────

const RbacSettingSchema = z.object({
  key: z.string(),
  value: z.string(),
  description: z.string().nullable(),
});

export async function getRbacSettings(prefix: string): Promise<RbacSettingEntry[]> {
  const raw = await invoke<unknown[]>("get_rbac_settings", { prefix });
  return z.array(RbacSettingSchema).parse(raw);
}

export async function updateRbacSetting(key: string, value: string): Promise<void> {
  await invoke<void>("update_rbac_setting", { key, value });
}

const PasswordPolicySchema = z.object({
  max_age_days: z.number(),
  warn_days_before_expiry: z.number(),
  min_length: z.number(),
  require_uppercase: z.boolean(),
  require_lowercase: z.boolean(),
  require_digit: z.boolean(),
  require_special: z.boolean(),
});

export async function getPasswordPolicy(): Promise<PasswordPolicySettings> {
  const raw = await invoke<unknown>("get_password_policy");
  return PasswordPolicySchema.parse(raw);
}

// ── Permission catalog (SP06-F02) ─────────────────────────────────────────

const PermissionWithSystemSchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().nullable(),
  category: z.string(),
  is_dangerous: z.boolean(),
  requires_step_up: z.boolean(),
  is_system: z.boolean(),
});

const PermissionDependencyRowSchema = z.object({
  id: z.number(),
  permission_name: z.string(),
  required_permission_name: z.string(),
  dependency_type: z.string(),
});

const MissingDependencySchema = z.object({
  permission_name: z.string(),
  required_permission_name: z.string(),
  dependency_type: z.string(),
});

const RoleValidationResultSchema = z.object({
  missing_hard_deps: z.array(MissingDependencySchema),
  warn_deps: z.array(MissingDependencySchema),
  unknown_permissions: z.array(z.string()),
  is_valid: z.boolean(),
});

export async function listPermissions(
  filter: PermissionListFilter,
): Promise<PermissionWithSystem[]> {
  const raw = await invoke<unknown[]>("list_permissions", { filter });
  return z.array(PermissionWithSystemSchema).parse(raw);
}

export async function getPermissionDependencies(
  permissionName: string,
): Promise<PermissionDependencyRow[]> {
  const raw = await invoke<unknown[]>("get_permission_dependencies", {
    permission_name: permissionName,
  });
  return z.array(PermissionDependencyRowSchema).parse(raw);
}

export async function createCustomPermission(
  input: CreateCustomPermissionInput,
): Promise<PermissionWithSystem> {
  const raw = await invoke<unknown>("create_custom_permission", { input });
  return PermissionWithSystemSchema.parse(raw);
}

export async function validateRolePermissions(
  permissionNames: string[],
): Promise<RoleValidationResult> {
  const raw = await invoke<unknown>("validate_role_permissions", {
    input: { permission_names: permissionNames },
  });
  return RoleValidationResultSchema.parse(raw);
}

// ── Emergency elevation (SP06-F02) ────────────────────────────────────────

export async function grantEmergencyElevation(
  input: GrantEmergencyElevationInput,
): Promise<IdPayload> {
  const raw = await invoke<unknown>("grant_emergency_elevation", { input });
  return IdPayloadSchema.parse(raw);
}

export async function revokeEmergencyElevation(
  input: RevokeEmergencyElevationInput,
): Promise<void> {
  await invoke<void>("revoke_emergency_elevation", { input });
}
