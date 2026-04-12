/**
 * Shared WO status helpers.
 * Extracted to eliminate duplication across WoDetailDialog, WorkOrdersPage, WoCalendarView.
 * Phase 2 – Sub-phase 05 – Sprint S5 (GA-028/GA-029).
 */

export type WoStatusKey =
  | "draft"
  | "awaitingApproval"
  | "planned"
  | "released"
  | "readyToSchedule"
  | "assigned"
  | "waitingForPrerequisite"
  | "inProgress"
  | "onHold"
  | "paused"
  | "mechanicallyComplete"
  | "technicallyVerified"
  | "closed"
  | "cancelled";

const STATUS_MAP: Record<string, WoStatusKey> = {
  draft: "draft",
  awaiting_approval: "awaitingApproval",
  planned: "planned",
  released: "released",
  ready_to_schedule: "readyToSchedule",
  assigned: "assigned",
  waiting_for_prerequisite: "waitingForPrerequisite",
  in_progress: "inProgress",
  on_hold: "onHold",
  paused: "paused",
  mechanically_complete: "mechanicallyComplete",
  technically_verified: "technicallyVerified",
  closed: "closed",
  cancelled: "cancelled",
};

/** Map Rust snake_case status to camelCase i18n key */
export function statusToI18nKey(s: string): WoStatusKey {
  return STATUS_MAP[s] ?? "draft";
}

export const STATUS_STYLE: Record<string, string> = {
  draft: "bg-gray-100 text-gray-600",
  awaiting_approval: "bg-purple-100 text-purple-800",
  planned: "bg-blue-100 text-blue-800",
  released: "bg-sky-100 text-sky-800",
  ready_to_schedule: "bg-indigo-100 text-indigo-800",
  assigned: "bg-violet-100 text-violet-800",
  in_progress: "bg-amber-100 text-amber-800",
  on_hold: "bg-orange-100 text-orange-800",
  paused: "bg-orange-100 text-orange-800",
  waiting_for_prerequisite: "bg-yellow-100 text-yellow-800",
  mechanically_complete: "bg-teal-100 text-teal-800",
  technically_verified: "bg-emerald-100 text-emerald-800",
  closed: "bg-neutral-100 text-neutral-500",
  cancelled: "bg-red-100 text-red-700",
};

export const URGENCY_STYLE: Record<string, string> = {
  "1": "bg-green-100 text-green-800",
  "2": "bg-blue-100 text-blue-800",
  "3": "bg-yellow-100 text-yellow-800",
  "4": "bg-orange-100 text-orange-800",
  "5": "bg-red-100 text-red-700",
};
