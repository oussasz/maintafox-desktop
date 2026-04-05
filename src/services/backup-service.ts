// ADR-003 compliant: all IPC calls for backup/restore go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for backup operations.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { BackupRunRecord, BackupRunResult, RestoreTestResult } from "@shared/ipc-types";

// ── Zod schemas for runtime shape validation ──────────────────────────────

const BackupRunRecordSchema = z.object({
  id: z.number().int(),
  trigger: z.string(),
  status: z.string(),
  output_path: z.string(),
  file_size_bytes: z.number().nullable(),
  sha256_checksum: z.string().nullable(),
  encryption_mode: z.string(),
  db_schema_version: z.number().nullable(),
  started_at: z.string(),
  completed_at: z.string().nullable(),
  error_message: z.string().nullable(),
  initiated_by_id: z.number().nullable(),
});

const BackupRunResultSchema = z.object({
  run_id: z.number().int(),
  output_path: z.string(),
  file_size_bytes: z.number(),
  sha256_checksum: z.string(),
  status: z.string(),
});

const RestoreTestResultSchema = z.object({
  backup_path: z.string(),
  integrity_ok: z.boolean(),
  stored_checksum: z.string().nullable(),
  computed_checksum: z.string(),
  checksum_match: z.boolean(),
  integrity_check_output: z.string(),
  warnings: z.array(z.string()),
});

// ── Service functions ─────────────────────────────────────────────────────

/**
 * Run a manual backup to the specified target path.
 * Requires: active session + adm.settings + recent step-up.
 */
export async function runManualBackup(targetPath: string): Promise<BackupRunResult> {
  const raw = await invoke<unknown>("run_manual_backup", {
    payload: { target_path: targetPath },
  });
  return BackupRunResultSchema.parse(raw) as BackupRunResult;
}

/**
 * List recent backup runs for the audit/history UI.
 * Requires: active session + adm.settings.
 */
export async function listBackupRuns(limit?: number): Promise<BackupRunRecord[]> {
  const raw = await invoke<unknown>("list_backup_runs", { limit });
  return z.array(BackupRunRecordSchema).parse(raw) as BackupRunRecord[];
}

/**
 * Validate a backup file's integrity without restoring it.
 * Requires: active session + adm.settings.
 */
export async function validateBackupFile(backupPath: string): Promise<RestoreTestResult> {
  const raw = await invoke<unknown>("validate_backup_file", {
    backupPath,
  });
  return RestoreTestResultSchema.parse(raw) as RestoreTestResult;
}

/**
 * Factory reset stub — validates all security gates but does NOT delete data.
 * Phase 2 will implement actual data deletion after VPS sync drain.
 * Requires: active session + adm.settings + step-up + typed confirmation phrase.
 */
export async function factoryResetStub(confirmationPhrase: string): Promise<void> {
  await invoke<void>("factory_reset_stub", {
    payload: { confirmation_phrase: confirmationPhrase },
  });
}
