// ADR-003: all invoke() calls for updater operations go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly for
// update operations. Use the updater store / useUpdater() hook instead.
//
// Master plan Phase 5.5 (in-app updates). The Rust side is
// src-tauri/src/commands/updater.rs + src-tauri/src/updater/mod.rs.

import { Channel } from "@tauri-apps/api/core";
import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  InstallOutcome,
  UpdateCheckResult,
  UpdateJournalView,
  UpdateProgressEvent,
} from "@shared/ipc-types";

// ── Runtime validation schemas ────────────────────────────────────────────────
// Mirror the serde shapes in src-tauri/src/updater/mod.rs.
// Zod parse is the enforced boundary — any unexpected shape from the Rust layer
// is caught here rather than silently corrupted in store state.

const UpdateKindSchema = z.enum(["upgrade", "rollback"]);

export const UpdateCheckResultSchema = z.object({
  status: z.enum(["available", "up_to_date", "not_configured", "not_activated", "unreachable"]),
  available: z.boolean(),
  current_version: z.string(),
  version: z.string().nullable(),
  notes: z.string().nullable(),
  pub_date: z.string().nullable(),
  kind: UpdateKindSchema.nullable(),
  mandatory: z.boolean(),
  mandatory_reason: z.string().nullable(),
  notes_en: z.string().nullable(),
  notes_fr: z.string().nullable(),
  message: z.string().nullable(),
});

const ByteCountSchema = z.number().int().nonnegative();

export const UpdateProgressEventSchema = z.discriminatedUnion("event", [
  z.object({ event: z.literal("backing_up") }),
  z.object({ event: z.literal("restoring_backup") }),
  z.object({
    event: z.literal("downloading"),
    downloaded_bytes: ByteCountSchema,
    total_bytes: ByteCountSchema.nullable(),
  }),
  z.object({ event: z.literal("installing") }),
]);

const InstallRefusalSchema = z.discriminatedUnion("code", [
  z.object({ code: z.literal("rollback_backup_missing") }),
  z.object({
    code: z.literal("rollback_unsynced_changes"),
    count: z.number().int().nonnegative(),
  }),
  z.object({ code: z.literal("update_withdrawn") }),
]);

export const InstallOutcomeSchema = z.discriminatedUnion("outcome", [
  z.object({ outcome: z.literal("restarting") }),
  z.object({ outcome: z.literal("refused"), refusal: InstallRefusalSchema }),
]);

export const UpdateJournalViewSchema = z.object({
  state: z.enum([
    "installing",
    "completed",
    "rollback_required",
    "rolled_back",
    "install_not_completed",
  ]),
  kind: UpdateKindSchema,
  from_version: z.string(),
  to_version: z.string(),
  failure: z.string().nullable(),
  notice_acknowledged: z.boolean(),
});

/** Callback receiving validated progress events while an update installs. */
export type UpdateProgressHandler = (event: UpdateProgressEvent) => void;

/**
 * Channel for the Rust `on_progress` argument. Each message is validated; a malformed one is
 * logged and skipped (progress is informative — the command's own result decides the outcome).
 */
function progressChannel(onProgress: UpdateProgressHandler): Channel<unknown> {
  return new Channel<unknown>((raw) => {
    const parsed = UpdateProgressEventSchema.safeParse(raw);
    if (parsed.success) {
      onProgress(parsed.data);
    } else {
      console.warn("updater-service: ignored malformed progress event", raw, parsed.error);
    }
  });
}

// ── Service functions ─────────────────────────────────────────────────────────
// Failures are shown inline by the updates UI (plain language), so no global toast here.

/**
 * Asks the update server whether this device should install another version.
 * No session required — safe at startup and on the login screen.
 * Never fails for network reasons: `status: "unreachable"` + `message` instead.
 */
export async function checkForUpdate(locale?: string): Promise<UpdateCheckResult> {
  const raw = await invoke<unknown>("check_for_update", { locale }, { notifyOnError: false });
  return UpdateCheckResultSchema.parse(raw);
}

/**
 * Backs up the database, downloads, verifies and installs the offered update.
 * Requires a signed-in session (enforced by the Rust command). The caller shows a
 * confirmation first. On `restarting` the installer runs and this process exits.
 */
export async function installPendingUpdate(
  locale: string | undefined,
  onProgress: UpdateProgressHandler,
): Promise<InstallOutcome> {
  const raw = await invoke<unknown>(
    "install_pending_update",
    { locale, onProgress: progressChannel(onProgress) },
    { notifyOnError: false },
  );
  return InstallOutcomeSchema.parse(raw);
}

/**
 * Recovery screen only: reinstalls the version this device ran before a failed database
 * update (the pre-update backup is already staged). No session required.
 */
export async function reinstallPreviousVersion(
  locale: string | undefined,
  onProgress: UpdateProgressHandler,
): Promise<InstallOutcome> {
  const raw = await invoke<unknown>(
    "reinstall_previous_version",
    { locale, onProgress: progressChannel(onProgress) },
    { notifyOnError: false },
  );
  return InstallOutcomeSchema.parse(raw);
}

/** The last in-app update's journal; null when this device never updated in the app. */
export async function getUpdateJournal(): Promise<UpdateJournalView | null> {
  const raw = await invoke<unknown>("get_update_journal", undefined, { notifyOnError: false });
  return UpdateJournalViewSchema.nullable().parse(raw);
}

/** Records that the user has read the outcome notice of the last update. */
export async function acknowledgeUpdateNotice(): Promise<void> {
  await invoke<void>("acknowledge_update_notice", undefined, { notifyOnError: false });
}
