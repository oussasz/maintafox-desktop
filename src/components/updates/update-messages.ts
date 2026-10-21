import type { TFunction } from "i18next";

import type { InstallRefusal } from "@shared/ipc-types";

/**
 * Plain-language explanation of why an install was not started.
 * `targetVersion` is the version the install would have put in place.
 */
export function refusalMessage(
  t: TFunction<"shell">,
  refusal: InstallRefusal,
  targetVersion: string,
): string {
  switch (refusal.code) {
    case "update_withdrawn":
      return t("updates.refusal.update_withdrawn");
    case "rollback_unsynced_changes":
      return t("updates.refusal.rollback_unsynced_changes", { count: refusal.count });
    case "rollback_backup_missing":
      return t("updates.refusal.rollback_backup_missing", { version: targetVersion });
    case "rollback_backup_changed":
      return t("updates.refusal.rollback_backup_changed", { version: targetVersion });
    case "restore_pending":
      return t("updates.refusal.restore_pending");
    case "rollback_refused":
      return t("updates.refusal.rollback_refused", { version: targetVersion });
  }
}

/** The reason codes the update server gives for a mandatory update (vendor-console PR #3). */
export const MANDATORY_REASON_CODES = [
  "forced_update_floor",
  "emergency",
  "rollback_requested",
  "recalled_version",
] as const;

/**
 * Why an update is mandatory, translated from the server's code. An unknown code gets the generic
 * text; the raw code is never shown.
 */
export function mandatoryReasonMessage(t: TFunction<"shell">, code: string | null): string {
  const known = MANDATORY_REASON_CODES.find((c) => c === code);
  return known ? t(`updates.mandatoryReason.${known}`) : t("updates.banner.mandatoryDefault");
}

/** Release notes in the UI language (French notes for "fr", English otherwise). */
export function notesForLocale(
  language: string,
  notes: { notes_en: string | null; notes_fr: string | null; notes: string | null },
): string | null {
  const localized = language.startsWith("fr") ? notes.notes_fr : notes.notes_en;
  return localized?.trim() ? localized : notes.notes?.trim() ? notes.notes : null;
}
