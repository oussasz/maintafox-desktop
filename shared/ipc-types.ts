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

// Frontend invokes via: invoke("shutdown_app")
