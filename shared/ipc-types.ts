// IPC contract types shared between src/ (frontend) and the Tauri command layer.
// Types defined here must be kept in sync with Rust structs in src-tauri/src/.

export interface HealthCheckResponse {
  status: "ok" | "degraded";
  version: string;
  db_connected: boolean;
  locale: string;
}

// ─── Startup Events ────────────────────────────────────────────────────────

export type StartupStage =
  | "db_ready"
  | "migrations_complete"
  | "entitlement_cache_loaded"
  | "ready"
  | "failed";

export interface StartupEvent {
  stage: StartupStage;
  /** Present only for stage = "migrations_complete" */
  applied?: number;
  /** Present only for stage = "failed" */
  reason?: string;
}

// ─── App Info ──────────────────────────────────────────────────────────────

export interface AppInfoResponse {
  version: string;
  build_mode: "debug" | "release";
  os: string;
  arch: string;
  app_name: string;
  default_locale: string;
}

// ─── Task Status ───────────────────────────────────────────────────────────

export type TaskStatusKind = "running" | "cancelled" | "finished";

export interface TaskStatusEntry {
  id: string;
  status: TaskStatusKind;
}

// ─── Shutdown ──────────────────────────────────────────────────────────────

// shutdown_app — no response type; the Rust command calls app.exit(0).

// ── Pagination ────────────────────────────────────────────────────────────

export interface PageRequest {
  page: number;
  per_page: number;
}

export interface Page<T> {
  items: T[];
  total: number;
  page: number;
  per_page: number;
  total_pages: number;
}

// ── Lookup domain types ───────────────────────────────────────────────────

export interface LookupDomainSummary {
  id: number;
  sync_id: string;
  domain_key: string;
  display_name: string;
  domain_type: "system" | "tenant" | "module";
  is_extensible: number;
  is_locked: number;
  schema_version: number;
  value_count: number | null;
}

export interface LookupValueOption {
  id: number;
  code: string;
  label: string;
  fr_label: string | null;
  en_label: string | null;
  color: string | null;
  is_active: number;
}

export interface LookupValueRecord {
  id: number;
  sync_id: string;
  domain_id: number;
  code: string;
  label: string;
  fr_label: string | null;
  en_label: string | null;
  description: string | null;
  sort_order: number;
  is_active: number;
  is_system: number;
  color: string | null;
  parent_value_id: number | null;
}

export interface LookupDomainFilter {
  domain_type?: string;
  query?: string;
  include_deleted?: boolean;
  include_inactive?: boolean;
}

// ─── Diagnostics / Integrity ───────────────────────────────────────────────

export interface IntegrityIssue {
  code: string;
  description: string;
  is_auto_repairable: boolean;
  subject: string;
}

export interface IntegrityReport {
  is_healthy: boolean;
  is_recoverable: boolean;
  issues: IntegrityIssue[];
  seed_schema_version: number | null;
  domain_count: number;
  value_count: number;
}

// ─── Locale / i18n ─────────────────────────────────────────────────────────

export interface LocalePreference {
  active_locale: string;
  user_locale: string | null;
  tenant_locale: string | null;
  os_locale: string | null;
  supported_locales: string[];
}

// ─── Authentication & Session ──────────────────────────────────────────────

export interface SessionInfo {
  is_authenticated: boolean;
  is_locked: boolean;
  user_id: number | null;
  username: string | null;
  display_name: string | null;
  is_admin: boolean | null;
  force_password_change: boolean | null;
  expires_at: string | null; // ISO 8601
  last_activity_at: string | null; // ISO 8601
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface LoginResponse {
  session_info: SessionInfo;
}

// ─── Device Trust ──────────────────────────────────────────────────────────

export interface TrustedDevice {
  id: string; // UUID
  user_id: number;
  device_fingerprint: string; // 64-char hex SHA-256
  device_label: string | null;
  is_revoked: boolean;
  last_seen_at: string | null; // ISO 8601
  registered_at: string; // ISO 8601
}

export type DeviceTrustStatus =
  | { status: "trusted"; device: TrustedDevice }
  | { status: "unknown" }
  | { status: "revoked" };

// ─── RBAC / Permissions ────────────────────────────────────────────────────

export interface PermissionRecord {
  name: string;
  description: string;
  category: string;
  is_dangerous: boolean;
  requires_step_up: boolean;
}

export interface StepUpRequest {
  password: string;
}

export interface StepUpResponse {
  success: boolean;
  expires_at: string; // ISO 8601
}

// ─── Settings / Configuration Center (SP06-F01) ────────────────────────────

export interface AppSetting {
  id: number;
  setting_key: string;
  setting_scope: string;
  setting_value_json: string;
  category: string;
  setting_risk: "low" | "high";
  validation_status: "valid" | "draft" | "error" | "untested";
  secret_ref_id: number | null;
  last_modified_by_id: number | null;
  last_modified_at: string; // ISO 8601
}

export interface PolicySnapshot {
  id: number;
  policy_domain: string;
  version_no: number;
  snapshot_json: string; // JSON-encoded policy document
  is_active: boolean;
  activated_at: string | null;
  activated_by_id: number | null;
}

export interface SettingsChangeEvent {
  id: number;
  setting_key_or_domain: string;
  change_summary: string;
  old_value_hash: string | null;
  new_value_hash: string | null;
  changed_by_id: number | null;
  changed_at: string; // ISO 8601
  required_step_up: boolean;
  apply_result: string;
}

export interface SessionPolicy {
  idle_timeout_minutes: number;
  absolute_session_minutes: number;
  offline_grace_hours: number;
  step_up_window_minutes: number;
  max_failed_attempts: number;
  lockout_minutes: number;
}

// ─── Auth UI Commands ──────────────────────────────────────────────────────

export interface UnlockSessionRequest {
  password: string;
}

export interface ForceChangePasswordRequest {
  new_password: string;
}

export interface ForceChangePasswordResponse {
  session_info: SessionInfo;
}

// ─── Updater ──────────────────────────────────────────────────────────────
// Mirrors UpdateCheckResult in src-tauri/src/commands/updater.rs

export interface UpdateCheckResult {
  available: boolean;
  version: string | null;
  notes: string | null;
  pub_date: string | null;
}

// ─── Diagnostics / Support Bundle (SP06-F03) ──────────────────────────────

/** Rich application metadata from `get_diagnostics_info` (session-gated). */
export interface DiagnosticsAppInfo {
  app_version: string;
  os_name: string;
  os_version: string;
  arch: string;
  db_schema_version: number;
  active_locale: string;
  sync_status: string;
  uptime_seconds: number;
}

/** Sanitized support bundle returned by `generate_support_bundle`. */
export interface SupportBundle {
  generated_at: string;
  app_info: DiagnosticsAppInfo;
  log_lines: string[];
  collection_warnings: string[];
}

// Frontend invokes via: invoke("shutdown_app")

// ─── Backup & Restore Preflight (SP06-F04) ────────────────────────────────

export interface BackupRunRecord {
  id: number;
  trigger: string;
  status: string;
  output_path: string;
  file_size_bytes: number | null;
  sha256_checksum: string | null;
  encryption_mode: string;
  db_schema_version: number | null;
  started_at: string;
  completed_at: string | null;
  error_message: string | null;
  initiated_by_id: number | null;
}

export interface BackupRunResult {
  run_id: number;
  output_path: string;
  file_size_bytes: number;
  sha256_checksum: string;
  status: string;
}

export interface RestoreTestResult {
  backup_path: string;
  integrity_ok: boolean;
  stored_checksum: string | null;
  computed_checksum: string;
  checksum_match: boolean;
  integrity_check_output: string;
  warnings: string[];
}

// ─── Organization & Site Operating Model (SP01-F01) ───────────────────────────

export interface OrgStructureModel {
  id: number;
  sync_id: string;
  version_number: number;
  /** "draft" | "active" | "superseded" | "archived" */
  status: string;
  description: string | null;
  activated_at: string | null;
  activated_by_id: number | null;
  superseded_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface OrgNodeType {
  id: number;
  sync_id: string;
  structure_model_id: number;
  code: string;
  label: string;
  icon_key: string | null;
  depth_hint: number | null;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
  is_active: boolean;
  created_at: string;
  updated_at: string;
}

export interface OrgRelationshipRule {
  id: number;
  structure_model_id: number;
  parent_type_id: number;
  parent_type_label: string | null;
  child_type_id: number;
  child_type_label: string | null;
  min_children: number | null;
  max_children: number | null;
  created_at: string;
}

export interface CreateStructureModelPayload {
  description: string | null;
}

export interface CreateOrgNodeTypePayload {
  structure_model_id: number;
  code: string;
  label: string;
  icon_key?: string;
  depth_hint?: number;
  can_host_assets: boolean;
  can_own_work: boolean;
  can_carry_cost_center: boolean;
  can_aggregate_kpis: boolean;
  can_receive_permits: boolean;
  is_root_type: boolean;
}

export interface CreateRelationshipRulePayload {
  structure_model_id: number;
  parent_type_id: number;
  child_type_id: number;
  min_children?: number;
  max_children?: number;
}
