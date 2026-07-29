/**
 * Shared WO status helpers — Option B lifecycle.
 * Statuses: draft → planning → ready → in_progress ↔ on_hold → completed → closed | cancelled
 *
 * Legacy snake_case codes from old data are remapped here for display purposes only.
 */

/** Canonical i18n key for each lifecycle status */
export type WoStatusKey =
  | "draft"
  | "planning"
  | "ready"
  | "inProgress"
  | "onHold"
  | "completed"
  | "closed"
  | "cancelled";

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
  return STATUS_MAP[s] ?? "draft";
}

export const STATUS_STYLE: Record<string, string> = {
  // Option B statuses
  draft: "bg-gray-100 text-gray-600",
  planning: "bg-blue-100 text-blue-800",
  ready: "bg-indigo-100 text-indigo-800",
  in_progress: "bg-amber-100 text-amber-800",
  on_hold: "bg-yellow-100 text-yellow-800",
  completed: "bg-teal-100 text-teal-800",
  closed: "bg-neutral-100 text-neutral-500",
  cancelled: "bg-red-100 text-red-700",
  // Legacy display fallbacks
  awaiting_approval: "bg-blue-100 text-blue-800",
  planned: "bg-blue-100 text-blue-800",
  released: "bg-indigo-100 text-indigo-800",
  ready_to_schedule: "bg-indigo-100 text-indigo-800",
  assigned: "bg-violet-100 text-violet-800",
  waiting_for_prerequisite: "bg-yellow-100 text-yellow-800",
  paused: "bg-orange-100 text-orange-800",
  mechanically_complete: "bg-teal-100 text-teal-800",
  technically_verified: "bg-emerald-100 text-emerald-800",
};

export const URGENCY_STYLE: Record<string, string> = {
  "1": "bg-green-100 text-green-800",
  "2": "bg-blue-100 text-blue-800",
  "3": "bg-yellow-100 text-yellow-800",
  "4": "bg-orange-100 text-orange-800",
  "5": "bg-red-100 text-red-700",
};
