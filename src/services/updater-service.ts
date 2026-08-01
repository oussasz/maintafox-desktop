// ADR-003: all invoke() calls for updater operations go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly for
// update operations. Use the useUpdater() hook instead.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { UpdateCheckResult } from "@shared/ipc-types";

// â”€â”€ Runtime validation schema â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Mirrors UpdateCheckResult in src-tauri/src/commands/updater.rs.
// Zod parse is the enforced boundary â€” any unexpected shape from the Rust layer
// is caught here rather than silently corrupted in store state.

const UpdateCheckResultSchema = z.object({
  available: z.boolean(),
  version: z.string().nullable(),
  notes: z.string().nullable(),
  pub_date: z.string().nullable(),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Check the remote manifest for a newer version.
 * No session required â€” safe at startup and on the login screen.
 * Always resolves (never throws); manifest failures return `available: false`.
 */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  const raw = await invoke<UpdateCheckResult>("check_for_update");
  return UpdateCheckResultSchema.parse(raw);
}

/**
 * Download and install a pending update.
 * Requires an active authenticated session â€” the Rust command enforces this.
 * The caller (useUpdater hook) is responsible for showing a confirmation dialog
 * before invoking this function.
 */
export async function installPendingUpdate(): Promise<void> {
  await invoke<void>("install_pending_update");
}
