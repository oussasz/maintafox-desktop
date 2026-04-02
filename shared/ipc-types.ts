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
// Frontend invokes via: invoke("shutdown_app")
