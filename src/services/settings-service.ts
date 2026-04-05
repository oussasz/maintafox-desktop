// ADR-003: all invoke() calls for settings go through this file only.
// Components and stores MUST NOT import from @tauri-apps/api/core directly
// for settings operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  AppSetting,
  PolicySnapshot,
  SessionPolicy,
  SettingsChangeEvent,
} from "@shared/ipc-types";

// ── Zod schemas for runtime validation ────────────────────────────────────────

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

// ── Service functions ─────────────────────────────────────────────────────────

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
 * Intentionally unauthenticated — called before login to load idle-timeout config.
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
