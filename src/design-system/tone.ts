/**
 * Status tones — the single colour vocabulary for state across Maintafox.
 *
 * Every business status (OT, DI, PM occurrence, permit, PO, inspection round …) maps to ONE
 * of these tones, so the same meaning always has the same colour in every module:
 *
 * | tone      | meaning (examples)                                          |
 * |-----------|-------------------------------------------------------------|
 * | neutral   | not started / archived (draft, closed, deferred)            |
 * | info      | queued, waiting for someone else (submitted, planning)      |
 * | progress  | work is happening (in progress, in review, released)        |
 * | attention | needs a decision from the reader (awaiting approval, hold)  |
 * | warning   | at risk (due soon, low stock, clarification requested)      |
 * | success   | done and accepted (approved, completed, verified)           |
 * | danger    | failed, overdue, blocked, cancelled, safety                 |
 *
 * Colours come from `--tone-*` CSS variables (light + dark in globals.css); each foreground is
 * ≥ 4.5:1 on its background. Never use raw Tailwind palette colours (`bg-amber-100`) for state.
 */

export type Tone = "neutral" | "info" | "progress" | "attention" | "warning" | "success" | "danger";

export const TONES: readonly Tone[] = [
  "neutral",
  "info",
  "progress",
  "attention",
  "warning",
  "success",
  "danger",
] as const;

interface ToneClasses {
  /** Chip / badge surface: background + text + border */
  chip: string;
  /** Small status dot */
  dot: string;
  /** Text-only accent (values, icons) */
  text: string;
  /** Tinted panel (callouts, KPI tile accent background) */
  panel: string;
  /** Solid bar segment (stacked bars, progress) */
  bar: string;
}

export const mfTone: Record<Tone, ToneClasses> = {
  neutral: {
    chip: "border-tone-neutral-border bg-tone-neutral-bg text-tone-neutral-fg",
    dot: "bg-tone-neutral-fg",
    text: "text-tone-neutral-fg",
    panel: "border-tone-neutral-border bg-tone-neutral-bg",
    bar: "bg-tone-neutral-fg",
  },
  info: {
    chip: "border-tone-info-border bg-tone-info-bg text-tone-info-fg",
    dot: "bg-tone-info-fg",
    text: "text-tone-info-fg",
    panel: "border-tone-info-border bg-tone-info-bg",
    bar: "bg-tone-info-fg",
  },
  progress: {
    chip: "border-tone-progress-border bg-tone-progress-bg text-tone-progress-fg",
    dot: "bg-tone-progress-fg",
    text: "text-tone-progress-fg",
    panel: "border-tone-progress-border bg-tone-progress-bg",
    bar: "bg-tone-progress-fg",
  },
  attention: {
    chip: "border-tone-attention-border bg-tone-attention-bg text-tone-attention-fg",
    dot: "bg-tone-attention-fg",
    text: "text-tone-attention-fg",
    panel: "border-tone-attention-border bg-tone-attention-bg",
    bar: "bg-tone-attention-fg",
  },
  warning: {
    chip: "border-tone-warning-border bg-tone-warning-bg text-tone-warning-fg",
    dot: "bg-tone-warning-fg",
    text: "text-tone-warning-fg",
    panel: "border-tone-warning-border bg-tone-warning-bg",
    bar: "bg-tone-warning-fg",
  },
  success: {
    chip: "border-tone-success-border bg-tone-success-bg text-tone-success-fg",
    dot: "bg-tone-success-fg",
    text: "text-tone-success-fg",
    panel: "border-tone-success-border bg-tone-success-bg",
    bar: "bg-tone-success-fg",
  },
  danger: {
    chip: "border-tone-danger-border bg-tone-danger-bg text-tone-danger-fg",
    dot: "bg-tone-danger-fg",
    text: "text-tone-danger-fg",
    panel: "border-tone-danger-border bg-tone-danger-bg",
    bar: "bg-tone-danger-fg",
  },
};

/** Work-order lifecycle status → tone (Option B codes and legacy aliases). */
export const WO_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  planning: "info",
  ready: "info",
  in_progress: "progress",
  on_hold: "attention",
  completed: "success",
  closed: "neutral",
  cancelled: "danger",
  awaiting_approval: "attention",
  planned: "info",
  released: "info",
  ready_to_schedule: "info",
  assigned: "info",
  waiting_for_prerequisite: "attention",
  paused: "attention",
  mechanically_complete: "success",
  technically_verified: "success",
};

/** Intervention-request (DI) status → tone. */
export const DI_STATUS_TONE: Record<string, Tone> = {
  submitted: "info",
  in_review: "progress",
  returned_for_clarification: "warning",
  awaiting_approval: "attention",
  approved: "success",
  deferred: "neutral",
  closed: "neutral",
};

/**
 * Level of the urgency scale shared by requests and work orders (`WORK.URGENCY`) → tone.
 * Codes are fixed by Maintafox; labels and target times come from the backend.
 */
export const URGENCY_TONE: Record<string, Tone> = {
  emergency: "danger",
  urgent: "attention",
  high: "warning",
  normal: "info",
  low: "neutral",
};

export function toneOf(map: Record<string, Tone>, code: string | null | undefined): Tone {
  if (!code) return "neutral";
  return map[code] ?? "neutral";
}

/** Chip classes for a status code through a tone map (drop-in for legacy `*_STYLE` maps). */
export function toneChipMap(map: Record<string, Tone>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(map).map(([code, tone]) => [code, `border ${mfTone[tone].chip}`]),
  );
}

/** Work permit (PTW / LOTO) status → tone. */
export const PERMIT_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  pending_review: "attention",
  approved: "info",
  issued: "info",
  active: "success",
  suspended: "danger",
  revalidation_required: "warning",
  closed: "neutral",
  handed_back: "neutral",
  cancelled: "danger",
  expired: "danger",
};

// ── Inventory & procurement ──────────────────────────────────────────────────

/** Purchase requisition / purchase order / goods receipt status → tone (procurement FSM codes). */
export const PROCUREMENT_STATUS_TONE: Record<string, Tone> = {
  DRAFT: "neutral",
  SUBMITTED: "attention",
  APPROVED: "info",
  PARTIALLY_RECEIVED: "progress",
  RECEIVED: "success",
  RECEIVED_CLOSED: "success",
  CLOSED: "neutral",
  REJECTED: "danger",
  CANCELLED: "danger",
};

/** Purchase priority (`inventory.purchase_priority` lookup) → tone. */
export const PURCHASE_PRIORITY_TONE: Record<string, Tone> = {
  LOW: "neutral",
  NORMAL: "info",
  HIGH: "attention",
  EMERGENCY: "danger",
};

/** Repairable (send-for-repair) order status → tone. */
export const REPAIRABLE_STATUS_TONE: Record<string, Tone> = {
  REQUESTED: "info",
  RELEASED: "info",
  SENT_FOR_REPAIR: "progress",
  RETURNED_FROM_REPAIR: "attention",
  CLOSED: "success",
  SCRAPPED: "danger",
  CANCELLED: "neutral",
};

/** Stock reservation status → tone. */
export const RESERVATION_STATUS_TONE: Record<string, Tone> = {
  active: "info",
  partial: "progress",
  consumed: "success",
  released: "neutral",
};

/** Cycle-count session status → tone. */
export const COUNT_SESSION_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  counting: "progress",
  submitted: "attention",
  approved: "info",
  posted: "success",
  reversed: "warning",
  cancelled: "danger",
};

/** ERP posting state of procurement documents → tone. */
export const POSTING_STATE_TONE: Record<string, Tone> = {
  PENDING_POSTING: "info",
  POSTED: "success",
  POSTING_FAILED: "danger",
  RECONCILED: "success",
};

/** Supplier qualification status → tone. */
export const SUPPLIER_STATUS_TONE: Record<string, Tone> = {
  PREFERRED: "success",
  APPROVED: "info",
  UNDER_EVALUATION: "attention",
  BLOCKED: "danger",
};

// ── Preventive maintenance ───────────────────────────────────────────────────

/** PM occurrence status → tone (overdue is computed separately from the due date). */
export const PM_OCCURRENCE_STATUS_TONE: Record<string, Tone> = {
  forecasted: "neutral",
  generated: "info",
  ready_for_scheduling: "info",
  scheduled: "info",
  in_progress: "progress",
  completed: "success",
  deferred: "warning",
  missed: "danger",
  cancelled: "neutral",
};

/** PM plan lifecycle status → tone. */
export const PM_PLAN_LIFECYCLE_TONE: Record<string, Tone> = {
  draft: "neutral",
  proposed: "attention",
  approved: "info",
  active: "success",
  suspended: "warning",
  retired: "neutral",
};

/** PM plan version status → tone. */
export const PM_VERSION_STATUS_TONE: Record<string, Tone> = {
  draft: "neutral",
  published: "success",
  superseded: "neutral",
};

/** PM execution result → tone. */
export const PM_EXECUTION_RESULT_TONE: Record<string, Tone> = {
  completed_no_findings: "success",
  completed_with_findings: "attention",
  deferred: "warning",
  missed: "danger",
  cancelled: "neutral",
};

/** PM finding severity → tone. */
export const PM_FINDING_SEVERITY_TONE: Record<string, Tone> = {
  low: "neutral",
  medium: "warning",
  high: "attention",
  critical: "danger",
};

/** Report run (Analytics → History) status → tone. */
export const REPORT_RUN_STATUS_TONE: Record<string, Tone> = {
  pending: "info",
  running: "progress",
  success: "success",
  failed: "danger",
};

/** Every intervention-request workflow status (incl. terminal ones) → tone, for distributions. */
export const DI_WORKFLOW_STATUS_TONE: Record<string, Tone> = {
  ...DI_STATUS_TONE,
  pending_review: "info",
  screened: "attention",
  approved_for_planning: "success",
  rejected: "danger",
  converted_to_work_order: "success",
  closed_as_non_executable: "neutral",
  archived: "neutral",
};

/** In-app notification delivery state → tone. */
export const NOTIFICATION_DELIVERY_TONE: Record<string, Tone> = {
  pending: "info",
  delivered: "info",
  read: "neutral",
  acknowledged: "success",
  escalated: "attention",
  snoozed: "neutral",
  expired: "neutral",
  closed: "neutral",
};

/** Notification severity → tone. */
export const NOTIFICATION_SEVERITY_TONE: Record<string, Tone> = {
  info: "info",
  warning: "warning",
  error: "danger",
  critical: "danger",
};

/** Archive action result (archive history) → tone. */
export const ARCHIVE_ACTION_RESULT_TONE: Record<string, Tone> = {
  success: "success",
  failed: "danger",
  blocked: "danger",
};

// ── Reliability (RAMS) ───────────────────────────────────────────────────────

/** Reliability study decision status → tone. */
export const RELIABILITY_DECISION_STATUS_TONE: Record<string, Tone> = {
  proposed: "attention",
  approved: "info",
  applied: "success",
  rejected: "neutral",
};

/** Data quality of an equipment's failure records (fleet view) → tone. */
export const RELIABILITY_DATA_QUALITY_TONE: Record<string, Tone> = {
  complete: "success",
  to_check: "warning",
  no_operating_hours: "danger",
};

/** Data check severity (study step 1) → tone. */
export const RELIABILITY_CHECK_TONE: Record<string, Tone> = {
  success: "success",
  info: "info",
  warning: "warning",
  danger: "danger",
};

/** Failure pattern read from the Weibull shape β → tone. */
export const RELIABILITY_PATTERN_TONE: Record<string, Tone> = {
  early_life: "attention",
  random: "info",
  wear_out: "success",
};

/** Timing of completed PM work against its tolerance window → tone. */
export const PM_TIMING_TONE: Record<string, Tone> = {
  on_time: "success",
  early: "warning",
  late: "danger",
};

/** Criticality matrix zone (FMECA, plant rating grids) → tone. */
export const RELIABILITY_ZONE_TONE: Record<string, Tone> = {
  unacceptable: "danger",
  reduce: "attention",
  monitor: "warning",
  acceptable: "success",
};

/** RCM tactic retained for a failure mode → tone. */
export const RELIABILITY_TACTIC_TONE: Record<string, Tone> = {
  condition_based: "success",
  time_based: "success",
  failure_finding: "info",
  run_to_failure: "neutral",
  redesign: "attention",
};

// ── Assets / Org ─────────────────────────────────────────────────────────────

/** Equipment status (EQUIPMENT.STATUS codes and legacy aliases) → tone. */
export const ASSET_STATUS_TONE: Record<string, Tone> = {
  ACTIVE_IN_SERVICE: "success",
  ACTIVE: "success",
  OPERATIONAL: "success",
  UNDER_MAINTENANCE: "progress",
  MAINTENANCE: "progress",
  STANDBY: "neutral",
  IN_STOCK: "info",
  SPARE: "info",
  OUT_OF_SERVICE: "danger",
  DECOMMISSIONED: "neutral",
  SCRAPPED: "neutral",
};

/** Equipment criticality class → tone (A is the highest consequence). */
export const CRITICALITY_TONE: Record<"A" | "B" | "C" | "D", Tone> = {
  A: "danger",
  B: "warning",
  C: "info",
  D: "neutral",
};

/** Organisation unit status → tone. */
export const ORG_NODE_STATUS_TONE: Record<string, Tone> = {
  active: "success",
  inactive: "neutral",
  draft: "info",
};
