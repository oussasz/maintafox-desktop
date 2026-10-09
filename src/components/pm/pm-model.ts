import type { PmOccurrence, PmPlan } from "@shared/ipc-types";

import { PM_ACTIVE_OCCURRENCE_STATUSES } from "./pm-labels";

export type PmDueWindow = "overdue" | "today" | "next7" | "all";

export function startOfUtcDay(d: Date): Date {
  return new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate()));
}

export function addUtcDays(d: Date, days: number): Date {
  const next = new Date(d.getTime());
  next.setUTCDate(next.getUTCDate() + days);
  return next;
}

export function matchesDueWindow(
  occ: PmOccurrence,
  window: PmDueWindow,
  now = new Date(),
): boolean {
  if (window === "all") return true;
  if (!occ.due_at) return false;
  const due = new Date(occ.due_at);
  if (Number.isNaN(due.getTime())) return false;
  const today = startOfUtcDay(now);
  const dueDay = startOfUtcDay(due);
  if (window === "overdue") return dueDay.getTime() < today.getTime();
  if (window === "today") return dueDay.getTime() === today.getTime();
  // next7
  const end = addUtcDays(today, 7);
  return dueDay.getTime() >= today.getTime() && dueDay.getTime() <= end.getTime();
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
      (p.description ?? "").toLowerCase().includes(q)
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
      o.due_basis.toLowerCase().includes(q) ||
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
      o.due_basis.toLowerCase().includes(q) ||
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

export const PM_CALENDAR_TONE_BORDER: Record<PmCalendarTone, string> = {
  overdue: "border-l-red-500",
  today: "border-l-orange-500",
  pipeline: "border-l-blue-500",
  completed: "border-l-green-500",
  muted: "border-l-gray-400",
};

export function buildTriggerJson(args: {
  strategy: string;
  intervalUnit: string;
  intervalValue: number;
  meterSource: string;
  meterInterval: number;
  eventCode: string;
  conditionCode: string;
}): string {
  switch (args.strategy) {
    case "fixed":
    case "floating":
      return JSON.stringify({
        interval_unit: args.intervalUnit,
        interval_value: args.intervalValue,
      });
    case "meter":
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

export function parseTriggerJson(
  strategy: string,
  json: string,
): {
  intervalUnit: string;
  intervalValue: number;
  meterSource: string;
  meterInterval: number;
  eventCode: string;
  conditionCode: string;
} {
  const defaults = {
    intervalUnit: "day",
    intervalValue: 30,
    meterSource: "operating_hours",
    meterInterval: 250,
    eventCode: "",
    conditionCode: "",
  };
  try {
    const obj = JSON.parse(json) as Record<string, unknown>;
    if (strategy === "fixed" || strategy === "floating") {
      return {
        ...defaults,
        intervalUnit: String(obj["interval_unit"] ?? "day"),
        intervalValue: Number(obj["interval_value"] ?? 30),
      };
    }
    if (strategy === "meter") {
      return {
        ...defaults,
        meterSource: String(obj["meter_source"] ?? "operating_hours"),
        meterInterval: Number(obj["interval_value"] ?? obj["threshold_value"] ?? 250),
      };
    }
    if (strategy === "event") {
      return { ...defaults, eventCode: String(obj["event_code"] ?? "") };
    }
    if (strategy === "condition") {
      return { ...defaults, conditionCode: String(obj["condition_code"] ?? "") };
    }
  } catch {
    /* keep defaults */
  }
  return defaults;
}
