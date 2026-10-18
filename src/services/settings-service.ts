// ADR-003: all invoke() calls for settings go through this file only.
// Components and stores MUST NOT import from @tauri-apps/api/core directly
// for settings operations.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  CurrencyOption,
  DocumentBrandingLogoPreview,
  DocumentBrandingLogoUploadResult,
  ActivatePolicyPayload,
  AppSetting,
  PolicySnapshot,
  PolicyTestResult,
  SavePolicyDraftPayload,
  SessionPolicy,
  SettingsChangeEvent,
  SettingsOverview,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const AppSettingSchema = z.object({
  id: z.number(),
  setting_key: z.string(),
  setting_scope: z.string(),
  setting_value_json: z.string(),
  category: z.string(),
  // Coerce legacy/unknown DB values so one bad row cannot blank the whole Settings UI.
  setting_risk: z.string().transform((v): "low" | "high" => (v === "high" ? "high" : "low")),
  validation_status: z.string().transform((v): "valid" | "draft" | "error" | "untested" => {
    if (v === "draft" || v === "error" || v === "untested" || v === "valid") return v;
    return "valid";
  }),
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
  changed_by_name: z.string().nullable().default(null),
  changed_at: z.string(),
  required_step_up: z.boolean(),
  apply_result: z.string(),
  old_value_json: z.string().nullable().default(null),
  new_value_json: z.string().nullable().default(null),
});

const SettingDefinitionViewSchema = z.object({
  key: z.string(),
  category: z.string(),
  scope: z.string(),
  kind: z.enum(["choice", "integer", "decimal", "currency", "month", "document_header", "module"]),
  options: z.array(z.string()),
  /** Bounds of an `integer`, `decimal` or `month` value (a decimal's bounds may be fractional). */
  min: z.number().nullable(),
  max: z.number().nullable(),
  shipped_value_json: z.string(),
  current_value_json: z.string().nullable(),
  differs_from_shipped: z.boolean(),
  editor: z.enum(["section", "dedicated"]),
  risk: z.string().transform((v): "low" | "high" => (v === "high" ? "high" : "low")),
  required_permission: z.string(),
  locked_reason: z.string().nullable(),
  last_modified_at: z.string().nullable(),
  last_modified_by: z.string().nullable(),
});

const SettingsOverviewSchema = z.object({
  definitions: z.array(SettingDefinitionViewSchema),
  roles_allowed_to_change: z.array(z.string()),
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
 * Ensure baseline settings + default policy snapshots exist (idempotent).
 * Requires `adm.settings`.
 */
export async function ensureSettingsDefaults(): Promise<void> {
  await invoke<void>("ensure_settings_defaults");
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
export async function listSettingChangeEvents(
  limit?: number,
  settingKey?: string,
): Promise<SettingsChangeEvent[]> {
  const raw = await invoke<SettingsChangeEvent[]>("list_setting_change_events", {
    limit,
    settingKey,
  });
  return z.array(SettingsChangeEventSchema).parse(raw);
}

const CurrencyOptionSchema = z.object({
  code: z.string(),
  label: z.string(),
  label_en: z.string().nullable(),
  symbol: z.string().nullable(),
  minor_units: z.number().nullable(),
  is_active: z.boolean(),
});

/** Currencies of Données de référence › Devises (published list). Any session. */
export async function listReferenceCurrencies(): Promise<CurrencyOption[]> {
  const raw = await invoke<unknown>("list_reference_currencies");
  return z.array(CurrencyOptionSchema).parse(raw);
}

/**
 * Settings in use: shipped vs current value, lock state, last change, and the roles
 * allowed to change them. Requires `adm.settings`.
 */
export async function getSettingsOverview(): Promise<SettingsOverview> {
  const raw = await invoke<unknown>("get_settings_overview");
  return SettingsOverviewSchema.parse(raw);
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
  code: z.string(),
  params: z.record(z.string(), z.string()),
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

// ── Document branding (letterhead logo) ─────────────────────────────────────

const DocumentBrandingLogoPreviewSchema = z.object({
  mime_type: z.string(),
  data_base64: z.string(),
  relative_path: z.string(),
});

const DocumentBrandingLogoUploadResultSchema = z.object({
  relative_path: z.string(),
  mime_type: z.string(),
  data_base64: z.string(),
});

/** Setting key for the TipTap letterhead JSON (tenant scope). */
export const DOCUMENT_HEADER_SETTING_KEY = "documents.default_header";

export interface DocumentHeaderSettingValue {
  version: number;
  content_json: unknown;
  html: string;
  logo_relative_path: string | null;
}

export const EMPTY_DOCUMENT_HEADER: DocumentHeaderSettingValue = {
  version: 1,
  content_json: { type: "doc", content: [{ type: "paragraph" }] },
  html: "",
  logo_relative_path: null,
};

export function parseDocumentHeaderValue(
  raw: string | null | undefined,
): DocumentHeaderSettingValue {
  if (!raw || !raw.trim()) return { ...EMPTY_DOCUMENT_HEADER };
  try {
    const parsed = JSON.parse(raw) as Partial<DocumentHeaderSettingValue>;
    if (typeof parsed !== "object" || parsed === null) return { ...EMPTY_DOCUMENT_HEADER };
    return {
      version: typeof parsed.version === "number" ? parsed.version : 1,
      content_json: parsed.content_json ?? EMPTY_DOCUMENT_HEADER.content_json,
      html: typeof parsed.html === "string" ? parsed.html : "",
      logo_relative_path:
        typeof parsed.logo_relative_path === "string" ? parsed.logo_relative_path : null,
    };
  } catch {
    return { ...EMPTY_DOCUMENT_HEADER };
  }
}

/** Upload / replace tenant document branding logo. Requires `adm.settings`. */
export async function uploadDocumentBrandingLogo(
  sourcePath: string,
): Promise<DocumentBrandingLogoUploadResult> {
  const raw = await invoke<unknown>("upload_document_branding_logo", { sourcePath });
  return DocumentBrandingLogoUploadResultSchema.parse(raw);
}

/** Clear tenant document branding logo. Requires `adm.settings`. */
export async function clearDocumentBrandingLogo(): Promise<void> {
  await invoke<void>("clear_document_branding_logo");
}

/** Read tenant document branding logo preview for a `data:` URL. */
export async function readDocumentBrandingLogoPreview(): Promise<DocumentBrandingLogoPreview | null> {
  const raw = await invoke<unknown>("read_document_branding_logo_preview");
  if (raw === null) return null;
  return DocumentBrandingLogoPreviewSchema.parse(raw);
}

// ── Per-site rules (Settings › Général › Règles par site) ─────────────────────────────────

const SiteSettingViewSchema = z.object({
  site_node_id: z.number(),
  site_code: z.string(),
  site_name: z.string(),
  /** `null`: never set at this site, or unreadable (the default applies). */
  value: z.boolean().nullable(),
  /** The stored value cannot be read: the default applies until the rule is set again. */
  value_unreadable: z.boolean(),
  default_value: z.boolean(),
  updated_by_name: z.string().nullable(),
  updated_at: z.string().nullable(),
  row_version: z.number().nullable(),
});
export type SiteSettingView = z.infer<typeof SiteSettingViewSchema>;

const SiteSettingEventSchema = z.object({
  id: z.number(),
  old_value: z.boolean().nullable(),
  new_value: z.boolean(),
  reason: z.string(),
  changed_by_name: z.string().nullable(),
  required_step_up: z.boolean(),
  changed_at: z.string(),
});
export type SiteSettingEvent = z.infer<typeof SiteSettingEventSchema>;

/** A per-site rule at every planning site. */
export async function listSiteSettings(key: string): Promise<SiteSettingView[]> {
  const raw = await invoke<unknown>("list_site_settings", { key });
  return z.array(SiteSettingViewSchema).parse(raw);
}

export interface SetSiteSettingInput {
  site_node_id: number;
  key: string;
  value: boolean;
  /** Why (audited), at least 10 characters. */
  reason: string;
  /** Row version read (`null`: the site had no rule). */
  expected_row_version: number | null;
}

/** Sets a per-site rule (step-up; the reason is audited). */
export async function setSiteSetting(input: SetSiteSettingInput): Promise<void> {
  await invoke<void>("set_site_setting", { input });
}

/** The audited changes of a per-site rule at one site, newest first. */
export async function listSiteSettingEvents(
  siteNodeId: number,
  key: string,
): Promise<SiteSettingEvent[]> {
  const raw = await invoke<unknown>("list_site_setting_events", { siteNodeId, key });
  return z.array(SiteSettingEventSchema).parse(raw);
}
