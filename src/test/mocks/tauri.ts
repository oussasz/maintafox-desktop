/**
 * Typed reference to the globally mocked invoke function.
 * The mock itself is created in src/test/setup.ts — this module provides
 * typed access and pre-built fixtures for test convenience.
 *
 * Prefer the Tauri core `invoke` mock: app services wrap it via `@/lib/ipc-invoke`,
 * so resetting/asserting on the core mock covers all IPC call sites.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { vi } from "vitest";

export const mockInvoke = vi.mocked(tauriInvoke);

/** Pre-built valid response fixtures matching shared/ipc-types.ts */
export const fixtures = {
  healthCheck: {
    status: "ok" as const,
    version: "0.1.0",
    db_connected: true,
    locale: "fr",
  },
  appInfo: {
    version: "0.1.0",
    build_mode: "debug" as const,
    os: "windows",
    arch: "x86_64",
    app_name: "Maintafox",
    default_locale: "fr",
  },
  taskStatus: [] as Array<{ id: string; status: "running" | "cancelled" | "finished" }>,
  taskStatusWithEntries: [
    { id: "bg-sync-001", status: "running" as const },
    { id: "bg-cleanup-002", status: "finished" as const },
  ],
  integrityReportHealthy: {
    is_healthy: true,
    is_recoverable: true,
    issues: [],
    seed_schema_version: 1,
    domain_count: 18,
    value_count: 95,
  },
  integrityReportUnhealthy: {
    is_healthy: false,
    is_recoverable: true,
    issues: [
      {
        code: "MISSING_DOMAIN",
        description: "Domaine requis manquant",
        is_auto_repairable: true,
        subject: "equipment.criticality",
      },
    ],
    seed_schema_version: 1,
    domain_count: 17,
    value_count: 90,
  },
  authenticatedSession: {
    is_authenticated: true,
    is_locked: false,
    user_id: 1,
    username: "admin",
    display_name: "Administrateur",
    is_admin: true,
    force_password_change: true,
    expires_at: new Date(Date.now() + 8 * 3600 * 1000).toISOString(),
    last_activity_at: new Date().toISOString(),
    tenant_id: "ten_demo",
    token_tenant_id: "ten_demo",
  },
  lockedSession: {
    is_authenticated: false,
    is_locked: true,
    user_id: 1,
    username: "admin",
    display_name: "Administrateur",
    is_admin: true,
    force_password_change: false,
    expires_at: new Date(Date.now() + 8 * 3600 * 1000).toISOString(),
    last_activity_at: new Date(Date.now() - 40 * 60 * 1000).toISOString(),
    tenant_id: "ten_demo",
    token_tenant_id: "ten_demo",
  },
  noSession: {
    is_authenticated: false,
    is_locked: false,
    user_id: null,
    username: null,
    display_name: null,
    is_admin: null,
    force_password_change: null,
    expires_at: null,
    last_activity_at: null,
    tenant_id: null,
    token_tenant_id: null,
  },
  // ── Updater ──────────────────────────────────────────────────────────────
  updateCheckNoUpdate: {
    status: "up_to_date" as const,
    available: false,
    current_version: "1.1.0",
    version: null,
    release_id: null,
    notes: null,
    pub_date: null,
    kind: null,
    mandatory: false,
    mandatory_reason: null,
    mandatory_message: null,
    notes_en: null,
    notes_fr: null,
    message: null,
  },
  updateCheckAvailable: {
    status: "available" as const,
    available: true,
    current_version: "1.1.0",
    version: "1.2.0",
    release_id: "rel_120",
    notes: "Bug fixes and stability improvements.",
    pub_date: "2026-04-15T00:00:00Z",
    kind: "upgrade" as const,
    mandatory: false,
    mandatory_reason: null,
    mandatory_message: null,
    notes_en: "Bug fixes and stability improvements.",
    notes_fr: "Corrections de bugs et améliorations de stabilité.",
    message: null,
  },
} as const;
