/**
 * Database encryption key operations (Phase 1.2).
 * Backend: src-tauri/src/commands/database_security.rs
 */
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

/** Admin-only (adm.settings + step-up): human-readable recovery key `XXXX-XXXX-…`. */
export async function getDatabaseRecoveryKey(): Promise<string> {
  const raw = await invoke<unknown>("get_database_recovery_key");
  return z.string().min(1).parse(raw);
}

/**
 * Startup recovery when the OS credential store lost the database key. On success the backend
 * stores the key and restarts the application (this promise does not resolve in that case).
 */
export async function recoverDatabaseKey(recoveryKey: string): Promise<void> {
  await invoke<void>("recover_database_key", { recoveryKey });
}
