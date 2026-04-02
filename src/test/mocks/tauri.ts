/**
 * Typed reference to the globally mocked invoke function.
 * The mock itself is created in src/test/setup.ts — this module provides
 * typed access and pre-built fixtures for test convenience.
 */
import { invoke } from "@tauri-apps/api/core";
import { vi } from "vitest";

export const mockInvoke = vi.mocked(invoke);

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
} as const;
