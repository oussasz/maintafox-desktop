// ADR-003: all invoke() calls for settings go through this file only.
// Components and stores MUST NOT import from @tauri-apps/api/core directly
// for settings operations.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ActivatePolicyPayload,
  AppSetting,
  PolicySnapshot,
  PolicyTestResult,
  SavePolicyDraftPayload,
  SessionPolicy,
  SettingsChangeEvent,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const AppSettingSchema = z.object({
  id: z.number(),
  setting_key: z.string(),
  setting_scope: z.string(),
  setting_value_json: z.string(),
  category: z.string(),
  setting_risk: z.enum(["low", "high"]),
  validation_status: z.enum(["valid", "draft", "error", "untested"]),
  secret_ref_id: z.number().nullable(),
  last_modified_by_id: z.number().nullable(),
  last_modified_at: z.string(),
});

const SessionPolicySchema = z.object({
  idle_timeout_minutes: z.number(),
  absolute_session_minutes: z.number(),
  offline_grace_hours: z.number(),
  step_up_window_minutes: z.number(),
  max_failed_attempts: z.number(),
  lockout_minutes: z.number(),
});

const SettingsChangeEventSchema = z.object({
  id: z.number(),
  setting_key_or_domain: z.string(),
  change_summary: z.string(),
  old_value_hash: z.string().nullable(),
  new_value_hash: z.string().nullable(),
  changed_by_id: z.number().nullable(),
  changed_at: z.string(),
  required_step_up: z.boolean(),
  apply_result: z.string(),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * List all settings across all categories.
 * Requires `adm.settings` permission.
 */
export async function listAllSettings(): Promise<AppSetting[]> {
  const raw = await invoke<AppSetting[]>("list_all_settings");
  return z.array(AppSettingSchema).parse(raw);
}

/**
 * List settings for a specific category.
 * Requires `adm.settings` permission.
 */
export async function listSettingsByCategory(category: string): Promise<AppSetting[]> {
  const raw = await invoke<AppSetting[]>("list_settings_by_category", { category });
  return z.array(AppSettingSchema).parse(raw);
}

/**
 * List distinct setting categories.
 * Requires `adm.settings` permission.
 */
export async function listSettingsCategories(): Promise<string[]> {
  const raw = await invoke<string[]>("list_settings_categories");
  return z.array(z.string()).parse(raw);
}

/**
 * Read a single setting by key and optional scope.
 * Any authenticated session may read settings.
 */
export async function getSetting(key: string, scope?: string): Promise<AppSetting | null> {
  const raw = await invoke<AppSetting | null>("get_setting", { key, scope });
  if (raw === null) return null;
  return AppSettingSchema.parse(raw);
}

export interface SetSettingPayload {
  key: string;
  scope?: string;
  value_json: string;
  change_summary?: string;
}

/**
 * Write a setting value. Requires `adm.settings` permission.
 * High-risk settings additionally require an active step-up session.
 */
export async function setSetting(payload: SetSettingPayload): Promise<void> {
  await invoke<void>("set_setting", { payload });
}

/**
 * Return the resolved session policy (active snapshot or safe defaults).
 * Intentionally unauthenticated â€” called before login to load idle-timeout config.
 */
export async function getSessionPolicy(): Promise<SessionPolicy> {
  const raw = await invoke<SessionPolicy>("get_session_policy");
  return SessionPolicySchema.parse(raw);
}

/**
 * Return the active policy snapshot for a domain (e.g. "session", "backup").
 * Returns null if no snapshot has been activated yet.
 */
export async function getPolicySnapshot(domain: string): Promise<PolicySnapshot | null> {
  return invoke<PolicySnapshot | null>("get_policy_snapshot", { domain });
}

/**
 * Return the most recent settings audit events.
 * Requires `adm.settings` permission.
 */
export async function listSettingChangeEvents(limit?: number): Promise<SettingsChangeEvent[]> {
  const raw = await invoke<SettingsChangeEvent[]>("list_setting_change_events", {
    limit,
  });
  return z.array(SettingsChangeEventSchema).parse(raw);
}

// â”€â”€ Policy Draft / Test / Activate â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const PolicySnapshotSchema = z.object({
  id: z.number(),
  policy_domain: z.string(),
  version_no: z.number(),
  snapshot_json: z.string(),
  is_active: z.boolean(),
  activated_at: z.string().nullable(),
  activated_by_id: z.number().nullable(),
});

const PolicyTestResultSchema = z.object({
  rule_name: z.string(),
  severity: z.enum(["pass", "warn", "fail"]),
  message: z.string(),
});

/**
 * Save a policy draft snapshot. Creates or updates the draft for the given domain.
 * Requires `adm.settings` permission.
 */
export async function savePolicyDraft(payload: SavePolicyDraftPayload): Promise<PolicySnapshot> {
  const raw = await invoke<PolicySnapshot>("save_policy_draft", { payload });
  return PolicySnapshotSchema.parse(raw);
}

/**
 * Run backend validation rules against the current draft.
 * Returns an array of test results with pass/warn/fail severity.
 * Requires `adm.settings` permission.
 */
export async function testPolicyDraft(domain: string): Promise<PolicyTestResult[]> {
  const raw = await invoke<PolicyTestResult[]>("test_policy_draft", { domain });
  return z.array(PolicyTestResultSchema).parse(raw);
}

/**
 * Promote the draft to active. Old active becomes superseded.
 * Security policy domains require an active step-up session.
 * Requires `adm.settings` permission.
 */
export async function activatePolicy(payload: ActivatePolicyPayload): Promise<PolicySnapshot> {
  const raw = await invoke<PolicySnapshot>("activate_policy", { payload });
  return PolicySnapshotSchema.parse(raw);
}

/**
 * Discard (delete) the current draft snapshot for a domain.
 * Requires `adm.settings` permission.
 */
export async function discardPolicyDraft(domain: string): Promise<void> {
  await invoke<void>("discard_policy_draft", { domain });
}

/**
 * List all policy snapshots for a domain (active + historical).
 * Used for change history display. Ordered by version_no desc.
 */
export async function listPolicySnapshots(domain: string): Promise<PolicySnapshot[]> {
  const raw = await invoke<PolicySnapshot[]>("list_policy_snapshots", { domain });
  return z.array(PolicySnapshotSchema).parse(raw);
}
