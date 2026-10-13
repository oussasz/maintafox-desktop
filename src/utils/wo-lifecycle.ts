/**
 * PRD §6.5 work-order lifecycle (12 states), derived from the WO's own facts.
 *
 * Mirror of the Rust `wo::lifecycle::derive` (the source of truth). The backend already returns
 * `lifecycle_state` on every work order; this mirror exists for places that hold the facts but
 * not the derived value, and is checked against every combination exported by the Rust test
 * (`shared/wo/lifecycle-cases.json`).
 */

export const WO_LIFECYCLE_STATES = [
  "draft",
  "awaiting_approval",
  "planned",
  "ready_to_schedule",
  "assigned",
  "waiting_for_prerequisite",
  "in_progress",
  "paused",
  "mechanically_complete",
  "technically_verified",
  "closed",
  "cancelled",
] as const;

export type WoLifecycleState = (typeof WO_LIFECYCLE_STATES)[number];

export type WoPhase =
  | "draft"
  | "planning"
  | "ready"
  | "in_progress"
  | "on_hold"
  | "completed"
  | "closed"
  | "cancelled";

export interface WoLifecycleFacts {
  phase: WoPhase;
  planning_approval_required: boolean;
  planning_approved: boolean;
  /** Planned start and planned end both set. */
  has_schedule: boolean;
  /** Primary responsible or assigned team set. */
  has_assignee: boolean;
  technically_verified: boolean;
  /** Kind of the open delay segment, if any. */
  open_hold: "pause" | "prerequisite" | null;
}

export function deriveWoLifecycle(f: WoLifecycleFacts): WoLifecycleState {
  switch (f.phase) {
    case "draft":
      return "draft";
    case "planning":
      return f.planning_approval_required && !f.planning_approved ? "awaiting_approval" : "planned";
    case "ready":
      return f.has_schedule && f.has_assignee ? "assigned" : "ready_to_schedule";
    case "in_progress":
      return "in_progress";
    case "on_hold":
      return f.open_hold === "prerequisite" ? "waiting_for_prerequisite" : "paused";
    case "completed":
      return f.technically_verified ? "technically_verified" : "mechanically_complete";
    case "closed":
      return "closed";
    case "cancelled":
      return "cancelled";
  }
}

export function isWoLifecycleState(v: unknown): v is WoLifecycleState {
  return typeof v === "string" && (WO_LIFECYCLE_STATES as readonly string[]).includes(v);
}
