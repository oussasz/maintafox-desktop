import type { Tone } from "@/design-system/tone";
import type { PmOccurrenceRow as PmOccurrence, PmPlanRow as PmPlan } from "@/services/pm-service";

import {
  PM_ACTIVE_OCCURRENCE_STATUSES,
  allowedOccurrenceTargets as allowedOccurrenceTargetsFor,
} from "./pm-labels";

export type PmDueWindow = "overdue" | "today" | "next7" | "all";

export function startOfUtcDay(d: Date): Date {
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
}

export function addUtcDays(d: Date, days: number): Date {
  const next = new Date(d.getTime());
  next.setUTCDate(next.getUTCDate() + days);
  return next;
}

/**
 * Due windows, defined exactly as the backend due KPIs (`get_pm_due_metrics`) so a KPI tile and
 * the list it opens show the same occurrences: overdue = due before now; today = due on the
 * current UTC day; next 7 days = due between now and now + 7 days.
 */
export function matchesDueWindow(
  occ: PmOccurrence,
  window: PmDueWindow,
  now = new Date(),
): boolean {
  if (window === "all") return true;
  if (!occ.due_at) return false;
  const due = new Date(occ.due_at);
  if (Number.isNaN(due.getTime())) return false;
  if (window === "overdue") return due.getTime() < now.getTime();
  if (window === "today") return startOfUtcDay(due).getTime() === startOfUtcDay(now).getTime();
  // next7
  const end = addUtcDays(now, 7);
  return due.getTime() >= now.getTime() && due.getTime() <= end.getTime();
}

export function filterPlans(
  plans: PmPlan[],
  opts: { search: string; lifecycle: string | null; strategy: string | null },
): PmPlan[] {
  const q = opts.search.trim().toLowerCase();
  return plans.filter((p) => {
    if (opts.lifecycle && p.lifecycle_status !== opts.lifecycle) return false;
    if (opts.strategy && p.strategy_type !== opts.strategy) return false;
    if (!q) return true;
    return (
      p.code.toLowerCase().includes(q) ||
      p.title.toLowerCase().includes(q) ||
      (p.description ?? "").toLowerCase().includes(q) ||
      (p.asset_code ?? "").toLowerCase().includes(q) ||
      (p.asset_name ?? "").toLowerCase().includes(q)
    );
  });
}

export function filterDueOccurrences(
  occurrences: PmOccurrence[],
  opts: {
    search: string;
    status: string | null;
    strategy: string | null;
    dueWindow: PmDueWindow;
  },
): PmOccurrence[] {
  const q = opts.search.trim().toLowerCase();
  const active = new Set<string>(PM_ACTIVE_OCCURRENCE_STATUSES);
  return occurrences.filter((o) => {
    if (!active.has(o.status)) return false;
    if (opts.status && o.status !== opts.status) return false;
    if (opts.strategy && (o.strategy_type ?? "") !== opts.strategy) return false;
    if (!matchesDueWindow(o, opts.dueWindow)) return false;
    if (!q) return true;
    return (
      (o.plan_code ?? "").toLowerCase().includes(q) ||
      (o.plan_title ?? "").toLowerCase().includes(q) ||
      (o.asset_code ?? "").toLowerCase().includes(q) ||
      (o.asset_name ?? "").toLowerCase().includes(q) ||
      (o.linked_work_order_code ?? "").toLowerCase().includes(q)
    );
  });
}

/** Calendar: require due_at; do not force active-only (completed/missed/cancelled may show). */
export function filterCalendarOccurrences(
  occurrences: PmOccurrence[],
  opts: {
    search: string;
    status: string | null;
    strategy: string | null;
  },
): PmOccurrence[] {
  const q = opts.search.trim().toLowerCase();
  return occurrences.filter((o) => {
    if (!o.due_at) return false;
    if (opts.status && o.status !== opts.status) return false;
    if (opts.strategy && (o.strategy_type ?? "") !== opts.strategy) return false;
    if (!q) return true;
    return (
      (o.plan_code ?? "").toLowerCase().includes(q) ||
      (o.plan_title ?? "").toLowerCase().includes(q) ||
      (o.asset_code ?? "").toLowerCase().includes(q) ||
      (o.asset_name ?? "").toLowerCase().includes(q) ||
      (o.linked_work_order_code ?? "").toLowerCase().includes(q)
    );
  });
}

export type PmCalendarTone = "overdue" | "today" | "pipeline" | "completed" | "muted";

const ACTIVE_SET = new Set<string>(PM_ACTIVE_OCCURRENCE_STATUSES);

export function calendarOccurrenceTone(occ: PmOccurrence, now = new Date()): PmCalendarTone {
  if (occ.status === "completed") return "completed";
  if (occ.status === "missed" || occ.status === "cancelled") return "muted";
  if (!occ.due_at || !ACTIVE_SET.has(occ.status)) return "pipeline";
  const due = new Date(occ.due_at);
  if (Number.isNaN(due.getTime())) return "pipeline";
  const today = startOfUtcDay(now);
  const dueDay = startOfUtcDay(due);
  if (dueDay.getTime() < today.getTime()) return "overdue";
  if (dueDay.getTime() === today.getTime()) return "today";
  return "pipeline";
}

/** Calendar chip meaning → status tone (one colour vocabulary, see design-system/tone.ts). */
export const PM_CALENDAR_TONE: Record<PmCalendarTone, Tone> = {
  overdue: "danger",
  today: "warning",
  pipeline: "info",
  completed: "success",
  muted: "neutral",
};

/** Left accent of a calendar chip (literal classes so Tailwind keeps them). */
export const PM_CALENDAR_TONE_BORDER: Record<PmCalendarTone, string> = {
  overdue: "border-l-tone-danger-fg",
  today: "border-l-tone-warning-fg",
  pipeline: "border-l-tone-info-fg",
  completed: "border-l-tone-success-fg",
  muted: "border-l-tone-neutral-fg",
};

/** An active occurrence whose due date is before today. */
export function isOccurrenceOverdue(occ: PmOccurrence, now = new Date()): boolean {
  return ACTIVE_SET.has(occ.status) && matchesDueWindow(occ, "overdue", now);
}

/**
 * Execution results the backend accepts from an occurrence status (result → target status).
 * "cancelled" is not offered here: cancelling needs a reason the execution input cannot carry,
 * so it is done with "Change status".
 */
const EXECUTION_RESULT_TARGET: Record<string, string> = {
  completed_no_findings: "completed",
  completed_with_findings: "completed",
  deferred: "deferred",
  missed: "missed",
};

/**
 * "Log execution" records how the work went; the backend completes an occurrence only from
 * `in_progress`, so the action is offered there only (other moves use "Change status").
 */
export function canLogExecution(occ: Pick<PmOccurrence, "status">): boolean {
  return occ.status === "in_progress";
}

export function allowedExecutionResults(status: string, allResults: readonly string[]): string[] {
  const targets = new Set(allowedOccurrenceTargetsFor(status));
  return allResults.filter((r) => {
    const target = EXECUTION_RESULT_TARGET[r];
    return target != null && targets.has(target);
  });
}

export interface PmTriggerFields {
  intervalUnit: string;
  intervalValue: number;
  /** Legacy meter mode: first active meter of this type on the asset. */
  meterSource: string;
  meterInterval: number;
  /** Chosen asset meter (recurring every `meterInterval` units from `startReading`). */
  assetMeterId: number | null;
  /** Meter reading at the last service; thresholds count from it. */
  startReading: number | null;
  eventCode: string;
  conditionCode: string;
}

export function buildTriggerJson(args: PmTriggerFields & { strategy: string }): string {
  switch (args.strategy) {
    case "fixed":
    case "floating":
      return JSON.stringify({
        interval_unit: args.intervalUnit,
        interval_value: args.intervalValue,
      });
    case "meter":
      if (args.assetMeterId != null) {
        return JSON.stringify({
          asset_meter_id: args.assetMeterId,
          interval_value: args.meterInterval,
          start_reading: args.startReading,
        });
      }
      return JSON.stringify({
        meter_source: args.meterSource,
        interval_value: args.meterInterval,
      });
    case "event":
      return JSON.stringify({ event_code: args.eventCode.trim().toUpperCase() });
    case "condition":
      return JSON.stringify({ condition_code: args.conditionCode.trim().toUpperCase() });
    default:
      return "{}";
  }
}

/** Starting values of the version form for a new version (shown and editable in the form). */
export const PM_TRIGGER_FORM_START: PmTriggerFields = {
  intervalUnit: "day",
  intervalValue: 30,
  meterSource: "operating_hours",
  meterInterval: 250,
  assetMeterId: null,
  startReading: null,
  eventCode: "",
  conditionCode: "",
};

function numberOr(value: unknown, fallback: number): number {
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

export function parseTriggerJson(strategy: string, json: string): PmTriggerFields {
  const start = PM_TRIGGER_FORM_START;
  let obj: Record<string, unknown>;
  try {
    const parsed = JSON.parse(json) as unknown;
    if (!parsed || typeof parsed !== "object") return start;
    obj = parsed as Record<string, unknown>;
  } catch {
    return start;
  }
  if (strategy === "fixed" || strategy === "floating") {
    return {
      ...start,
      intervalUnit: typeof obj["interval_unit"] === "string" ? obj["interval_unit"] : "day",
      intervalValue: numberOr(obj["interval_value"], start.intervalValue),
    };
  }
  if (strategy === "meter") {
    const meterId = typeof obj["asset_meter_id"] === "number" ? obj["asset_meter_id"] : null;
    const startReading =
      typeof obj["start_reading"] === "number" && Number.isFinite(obj["start_reading"])
        ? obj["start_reading"]
        : null;
    return {
      ...start,
      meterSource:
        typeof obj["meter_source"] === "string" ? obj["meter_source"] : start.meterSource,
      meterInterval: numberOr(obj["interval_value"] ?? obj["threshold_value"], start.meterInterval),
      assetMeterId: meterId,
      startReading,
    };
  }
  if (strategy === "event") {
    return { ...start, eventCode: typeof obj["event_code"] === "string" ? obj["event_code"] : "" };
  }
  if (strategy === "condition") {
    return {
      ...start,
      conditionCode: typeof obj["condition_code"] === "string" ? obj["condition_code"] : "",
    };
  }
  return start;
}
