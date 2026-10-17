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
  if (window === "overdue") {
    // Past the tolerance window when the occurrence has one (late), else past the due date.
    const end = occ.window_end_at ? new Date(occ.window_end_at) : due;
    return (Number.isNaN(end.getTime()) ? due : end).getTime() < now.getTime();
  }
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
  // Overdue = past the end of the tolerance window (the due date when there is none): the same
  // rule as the PM page KPI and the backend (`pm::queries::PM_OVERDUE_SQL`).
  if (matchesDueWindow(occ, "overdue", now)) return "overdue";
  // Due today, or past its due day but still inside its tolerance window: due, not yet late.
  if (startOfUtcDay(due).getTime() <= startOfUtcDay(now).getTime()) return "today";
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

/** Early/late tolerance of a version (days, or % of the interval). */
export interface PmTolerance {
  unit: "days" | "percent";
  early: number;
  late: number;
}

/** Usage counter of a version: a chosen asset meter, or a meter type found on each asset. */
export interface PmMeterSpec {
  assetMeterId: number | null;
  meterType: string | null;
  interval: number;
  /** Lifetime reading at the last service; empty = the reading when the version takes effect. */
  startReading: number | null;
}

export interface PmTriggerFields {
  intervalUnit: string;
  intervalValue: number;
  /** Legacy meter mode kept for versions written before meter types were offered. */
  meterSource: string;
  /** Meter strategy: the counter and its interval. */
  meter: PmMeterSpec;
  /** Calendar strategies: "or every N units, whichever comes first" (null = calendar only). */
  orMeter: PmMeterSpec | null;
  tolerance: PmTolerance | null;
  eventCode: string;
  conditionCode: string;
}

function meterSpecJson(spec: PmMeterSpec, legacySource?: string): Record<string, unknown> {
  const out: Record<string, unknown> = { interval_value: spec.interval };
  if (spec.assetMeterId != null) out["asset_meter_id"] = spec.assetMeterId;
  else if (spec.meterType) out["meter_type"] = spec.meterType;
  else if (legacySource) out["meter_source"] = legacySource;
  if (spec.startReading != null) out["start_reading"] = spec.startReading;
  return out;
}

export function buildTriggerJson(args: PmTriggerFields & { strategy: string }): string {
  let body: Record<string, unknown>;
  switch (args.strategy) {
    case "fixed":
    case "floating":
      body = { interval_unit: args.intervalUnit, interval_value: args.intervalValue };
      if (args.orMeter) body["meter"] = meterSpecJson(args.orMeter);
      break;
    case "meter":
      body = meterSpecJson(args.meter, args.meterSource);
      break;
    case "event":
      body = { event_code: args.eventCode.trim().toUpperCase() };
      break;
    case "condition":
      body = { condition_code: args.conditionCode.trim().toUpperCase() };
      break;
    default:
      return "{}";
  }
  if (args.tolerance && (args.tolerance.early > 0 || args.tolerance.late > 0)) {
    body["tolerance"] = args.tolerance;
  }
  return JSON.stringify(body);
}

/** Starting values of the version form for a new version (shown and editable in the form). */
export const PM_TRIGGER_FORM_START: PmTriggerFields = {
  intervalUnit: "day",
  intervalValue: 30,
  meterSource: "operating_hours",
  meter: { assetMeterId: null, meterType: null, interval: 250, startReading: null },
  orMeter: null,
  tolerance: null,
  eventCode: "",
  conditionCode: "",
};

function numberOr(value: unknown, fallback: number): number {
  const n = typeof value === "number" ? value : Number(value);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

function finiteOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

const LEGACY_SOURCE_TYPE: Record<string, string> = {
  operating_hours: "HOURS",
  odometer: "DISTANCE",
};

function parseMeterSpec(obj: Record<string, unknown>, fallbackInterval: number): PmMeterSpec {
  const legacy =
    typeof obj["meter_source"] === "string" ? LEGACY_SOURCE_TYPE[obj["meter_source"]] : undefined;
  return {
    assetMeterId: typeof obj["asset_meter_id"] === "number" ? obj["asset_meter_id"] : null,
    meterType: typeof obj["meter_type"] === "string" ? obj["meter_type"] : (legacy ?? null),
    interval: numberOr(obj["interval_value"] ?? obj["threshold_value"], fallbackInterval),
    startReading: finiteOrNull(obj["start_reading"]),
  };
}

function parseTolerance(raw: unknown): PmTolerance | null {
  if (!raw || typeof raw !== "object") return null;
  const obj = raw as Record<string, unknown>;
  const unit = obj["unit"] === "percent" ? "percent" : obj["unit"] === "days" ? "days" : null;
  if (!unit) return null;
  return {
    unit,
    early: finiteOrNull(obj["early"]) ?? 0,
    late: finiteOrNull(obj["late"]) ?? 0,
  };
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
  const tolerance = parseTolerance(obj["tolerance"]);
  if (strategy === "fixed" || strategy === "floating") {
    const meterBlock = obj["meter"];
    return {
      ...start,
      tolerance,
      intervalUnit: typeof obj["interval_unit"] === "string" ? obj["interval_unit"] : "day",
      intervalValue: numberOr(obj["interval_value"], start.intervalValue),
      orMeter:
        meterBlock && typeof meterBlock === "object"
          ? parseMeterSpec(meterBlock as Record<string, unknown>, start.meter.interval)
          : null,
    };
  }
  if (strategy === "meter") {
    return {
      ...start,
      tolerance,
      meterSource:
        typeof obj["meter_source"] === "string" ? obj["meter_source"] : start.meterSource,
      meter: parseMeterSpec(obj, start.meter.interval),
    };
  }
  if (strategy === "event") {
    return {
      ...start,
      tolerance,
      eventCode: typeof obj["event_code"] === "string" ? obj["event_code"] : "",
    };
  }
  if (strategy === "condition") {
    return {
      ...start,
      tolerance,
      conditionCode: typeof obj["condition_code"] === "string" ? obj["condition_code"] : "",
    };
  }
  return start;
}

/** One structured required part of a version (`required_parts_json` entry). */
export interface PmRequiredPart {
  articleCode: string;
  quantity: number | null;
}

/** Plain codes (older versions) and `{article_code, quantity}` entries. */
export function parseRequiredParts(raw: string | null | undefined): PmRequiredPart[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((item: unknown): PmRequiredPart[] => {
      if (typeof item === "string" && item.trim())
        return [{ articleCode: item.trim(), quantity: null }];
      if (item && typeof item === "object") {
        const o = item as Record<string, unknown>;
        const code = o["article_code"] ?? o["code"];
        if (typeof code === "string" && code.trim()) {
          return [{ articleCode: code.trim(), quantity: finiteOrNull(o["quantity"] ?? o["qty"]) }];
        }
      }
      return [];
    });
  } catch {
    return [];
  }
}

export function requiredPartsJson(parts: PmRequiredPart[]): string | null {
  const rows = parts
    .filter((p) => p.articleCode.trim())
    .map((p) =>
      p.quantity != null && p.quantity > 0
        ? { article_code: p.articleCode.trim(), quantity: p.quantity }
        : p.articleCode.trim(),
    );
  return rows.length > 0 ? JSON.stringify(rows) : null;
}

/** Skill codes of `required_skills_json`. */
export function parseCodeList(raw: string | null | undefined): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed)
      ? parsed.filter((c): c is string => typeof c === "string" && c.trim() !== "")
      : [];
  } catch {
    return [];
  }
}
