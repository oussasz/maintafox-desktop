/**
 * Shared WO status helpers — Option B lifecycle.
 * Statuses: draft → planning → ready → in_progress ↔ on_hold → completed → closed | cancelled
 *
 * Legacy snake_case codes from old data are remapped here for display purposes only.
 */

import { toneChipMap, WO_STATUS_TONE } from "@/design-system/tone";

/** Canonical i18n key for each lifecycle status */
export type WoStatusKey =
  | "draft"
  | "planning"
  | "ready"
  | "inProgress"
  | "onHold"
  | "completed"
  | "closed"
  | "cancelled"
  /** A status code this version does not know: shown as such, never as "Brouillon". */
  | "unknown";

/**
 * Maps any Rust snake_case status_code (current or legacy) to the closest
 * WoStatusKey for i18n label resolution.
 */
const STATUS_MAP: Record<string, WoStatusKey> = {
  // Current Option B codes
  draft: "draft",
  planning: "planning",
  ready: "ready",
  in_progress: "inProgress",
  on_hold: "onHold",
  completed: "completed",
  closed: "closed",
  cancelled: "cancelled",
  // Legacy aliases — map old data to nearest Option B key for display
  awaiting_approval: "planning",
  planned: "planning",
  released: "ready",
  ready_to_schedule: "ready",
  assigned: "ready",
  waiting_for_prerequisite: "onHold",
  paused: "onHold",
  mechanically_complete: "completed",
  technically_verified: "completed",
};

/** Map any snake_case status_code to a camelCase i18n key safe for `t("status.X")`. */
export function statusToI18nKey(s: string): WoStatusKey {
  return STATUS_MAP[s] ?? "unknown";
}

/** Chip classes per status — derived from the shared tone vocabulary (design-system/tone.ts). */
export const STATUS_STYLE: Record<string, string> = toneChipMap(WO_STATUS_TONE);
