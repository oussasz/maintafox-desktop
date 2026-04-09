// ADR-003 compliant: all invoke() calls for RBAC go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for permission operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  AdminStatsPayload,
  CreateUserInput,
  PasswordPolicySettings,
  PermissionRecord,
  RbacSettingEntry,
  StepUpRequest,
  StepUpResponse,
  UserPresence,
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

export async function createUser(input: CreateUserInput): Promise<{ user_id: number }> {
  const raw = await invoke<unknown>("create_user", { payload: input });
  return z.object({ user_id: z.number() }).parse(raw);
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
