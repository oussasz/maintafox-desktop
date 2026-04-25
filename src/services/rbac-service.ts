// ADR-003 compliant: all invoke() calls for RBAC go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for permission operations.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AdminChangeEventDetail,
  AdminEventFilter,
  AdminStatsPayload,
  AssignableRoleSummary,
  AssignRoleScopeInput,
  CreateCustomPermissionInput,
  CreateDelegationInput,
  CreateRoleInput,
  CreateUserInput,
  DelegationPolicyView,
  EmergencyGrantView,
  GrantEmergencyElevationInput,
  IdPayload,
  ImportResult,
  MissingTenantScopeUser,
  PasswordPolicySettings,
  PermissionDependencyRow,
  PermissionListFilter,
  PermissionRecord,
  PermissionWithSystem,
  RbacSettingEntry,
  RevokeEmergencyElevationInput,
  RoleDetail,
  RoleExportPayload,
  RoleImportPayload,
  RoleTemplate,
  RoleValidationResult,
  RoleWithPermissions,
  SessionSummary,
  SimulateAccessInput,
  SimulateAccessResult,
  StepUpRequest,
  StepUpResponse,
  TenantScopeBackfillResult,
  UpdateDelegationInput,
  UpdateRoleInput,
  UpdateUserInput,
  UserDetail,
  UserListFilter,
  UserPresence,
  UserWithRoles,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Admin stats â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ User presence â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const UserPresenceSchema = z.object({
  user_id: z.number(),
  status: z.enum(["active", "idle", "offline"]),
  last_activity_at: z.string().nullable(),
});

export async function getUserPresence(userIds: number[]): Promise<UserPresence[]> {
  const raw = await invoke<unknown[]>("get_user_presence", { userIds });
  return z.array(UserPresenceSchema).parse(raw);
}

// â”€â”€ User management â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
  personnel_id: z.number().nullable().default(null),
  email: z.string().nullable().default(null),
  phone: z.string().nullable().default(null),
  identity_mode: z.string(),
  is_active: z.boolean(),
  force_password_change: z.boolean(),
  last_seen_at: z.string().nullable(),
  locked_until: z.string().nullable(),
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

const MissingTenantScopeUserSchema = z.object({
  user_id: z.number(),
  username: z.string(),
  identity_mode: z.string(),
  has_any_role_assignment: z.boolean(),
});

const TenantScopeBackfillResultSchema = z.object({
  tenant_id: z.string().nullable(),
  updated_count: z.number(),
  updated_user_ids: z.array(z.number()),
});

export async function listUsers(filter: UserListFilter): Promise<UserWithRoles[]> {
  const raw = await invoke<unknown[]>("list_users", { filter });
  return z.array(UserWithRolesSchema).parse(raw);
}

const AssignableRoleSummarySchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().nullable(),
  role_type: z.string(),
  status: z.string(),
  is_system: z.boolean(),
});

/** Roles that may be selected when creating a user (requires adm.users). */
export async function listAssignableRoles(): Promise<AssignableRoleSummary[]> {
  const raw = await invoke<unknown[]>("list_assignable_roles");
  return z.array(AssignableRoleSummarySchema).parse(raw);
}

export async function getUser(userId: number): Promise<UserDetail> {
  const raw = await invoke<unknown>("get_user", { userId });
  return UserDetailSchema.parse(raw);
}

export async function createUser(input: CreateUserInput): Promise<IdPayload> {
  const raw = await invoke<unknown>("create_user", { input });
  return IdPayloadSchema.parse(raw);
}

export async function listUsersMissingTenantScope(): Promise<MissingTenantScopeUser[]> {
  const raw = await invoke<unknown[]>("list_users_missing_tenant_scope");
  return z.array(MissingTenantScopeUserSchema).parse(raw);
}

export async function backfillUsersMissingTenantScope(): Promise<TenantScopeBackfillResult> {
  const raw = await invoke<unknown>("backfill_users_missing_tenant_scope");
  return TenantScopeBackfillResultSchema.parse(raw);
}

export async function updateUser(input: UpdateUserInput): Promise<void> {
  await invoke<void>("update_user", { input });
}

export async function deactivateUser(userId: number): Promise<void> {
  await invoke<void>("deactivate_user", { userId });
}

export async function assignRoleScope(input: AssignRoleScopeInput): Promise<IdPayload> {
  const raw = await invoke<unknown>("assign_role_scope", { input });
  return IdPayloadSchema.parse(raw);
}

export async function revokeRoleScope(assignmentId: number): Promise<void> {
  await invoke<void>("revoke_role_scope", { assignmentId });
}

// â”€â”€ Role management â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
  const raw = await invoke<unknown>("get_role", { roleId });
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
  await invoke<void>("delete_role", { roleId });
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
  await invoke<void>("unlock_user_account", { userId });
}

// â”€â”€ RBAC settings (password policy, lockout) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Permission catalog (SP06-F02) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Emergency elevation (SP06-F02) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ Admin Governance â€” Session Visibility (SP06-F03) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const SessionSummarySchema = z.object({
  session_id: z.string(),
  user_id: z.string(),
  username: z.string(),
  device_id: z.string().nullable(),
  device_name: z.string().nullable(),
  device_trust_status: z.string(),
  session_started_at: z.string(),
  last_activity_at: z.string().nullable(),
  is_current_session: z.boolean(),
  current_role_names: z.array(z.string()),
});

export async function listActiveSessions(): Promise<SessionSummary[]> {
  const raw = await invoke<unknown[]>("list_active_sessions");
  return z.array(SessionSummarySchema).parse(raw);
}

export async function revokeSession(sessionId: string): Promise<void> {
  await invoke<void>("revoke_session", { sessionId });
}

// â”€â”€ Admin Governance â€” Delegation (SP06-F03) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DelegationPolicyViewSchema = z.object({
  id: z.number(),
  admin_role_id: z.number(),
  admin_role_name: z.string(),
  managed_scope_type: z.string(),
  managed_scope_reference: z.string().nullable(),
  allowed_domains: z.array(z.string()),
  requires_step_up_for_publish: z.boolean(),
});

export async function listDelegationPolicies(): Promise<DelegationPolicyView[]> {
  const raw = await invoke<unknown[]>("list_delegation_policies");
  return z.array(DelegationPolicyViewSchema).parse(raw);
}

export async function createDelegationPolicy(
  input: CreateDelegationInput,
): Promise<DelegationPolicyView> {
  const raw = await invoke<unknown>("create_delegation_policy", { input });
  return DelegationPolicyViewSchema.parse(raw);
}

export async function updateDelegationPolicy(input: UpdateDelegationInput): Promise<void> {
  await invoke<void>("update_delegation_policy", { input });
}

export async function deleteDelegationPolicy(policyId: number): Promise<void> {
  await invoke<void>("delete_delegation_policy", { policyId });
}

// â”€â”€ Admin Governance â€” Emergency Grants (SP06-F03) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const EmergencyGrantViewSchema = z.object({
  assignment_id: z.number(),
  user_id: z.number(),
  username: z.string(),
  role_id: z.number(),
  role_name: z.string(),
  scope_type: z.string(),
  scope_reference: z.string().nullable(),
  emergency_reason: z.string().nullable(),
  emergency_expires_at: z.string().nullable(),
  assigned_by_username: z.string().nullable(),
  created_at: z.string(),
  is_expired: z.boolean(),
});

export async function listEmergencyGrants(): Promise<EmergencyGrantView[]> {
  const raw = await invoke<unknown[]>("list_emergency_grants");
  return z.array(EmergencyGrantViewSchema).parse(raw);
}

// â”€â”€ Admin Governance â€” Role Import/Export (SP06-F03) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const RoleExportEntrySchema = z.object({
  id: z.number(),
  name: z.string(),
  description: z.string().nullable(),
  permissions: z.array(z.string()),
  is_system: z.boolean(),
});

const RoleExportPayloadSchema = z.object({
  roles: z.array(RoleExportEntrySchema),
  exported_at: z.string(),
  exported_by: z.string(),
});

const SkippedRoleSchema = z.object({
  name: z.string(),
  errors: z.array(z.string()),
});

const ImportResultSchema = z.object({
  imported_count: z.number(),
  skipped: z.array(SkippedRoleSchema),
});

export async function exportRoleModel(roleIds: number[]): Promise<RoleExportPayload> {
  const raw = await invoke<unknown>("export_role_model", { roleIds });
  return RoleExportPayloadSchema.parse(raw);
}

export async function importRoleModel(payload: RoleImportPayload): Promise<ImportResult> {
  const raw = await invoke<unknown>("import_role_model", { payload });
  return ImportResultSchema.parse(raw);
}

// â”€â”€ Admin Audit Events (SP06-F04) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const AdminChangeEventDetailSchema = z.object({
  id: z.number(),
  action: z.string(),
  actor_id: z.number().nullable(),
  actor_username: z.string().nullable(),
  target_user_id: z.number().nullable(),
  target_username: z.string().nullable(),
  target_role_id: z.number().nullable(),
  target_role_name: z.string().nullable(),
  acted_at: z.string(),
  scope_type: z.string().nullable(),
  scope_reference: z.string().nullable(),
  summary: z.string().nullable(),
  diff_json: z.string().nullable(),
  step_up_used: z.boolean(),
  apply_result: z.string(),
});

export async function listAdminEvents(filter: AdminEventFilter): Promise<AdminChangeEventDetail[]> {
  const raw = await invoke<unknown[]>("list_admin_events", { filter });
  return z.array(AdminChangeEventDetailSchema).parse(raw);
}

export async function getAdminEvent(eventId: number): Promise<AdminChangeEventDetail> {
  const raw = await invoke<unknown>("get_admin_event", { eventId });
  return AdminChangeEventDetailSchema.parse(raw);
}
