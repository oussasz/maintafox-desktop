/** Availability tab domain helpers (ResourceTimeline + summary). */

import {
  RESOURCE_TIMELINE_DEFAULT_HORIZON_DAYS,
  RESOURCE_TIMELINE_HORIZON_PRESETS,
  RESOURCE_TIMELINE_STATE_CLASS,
  type ResourceTimelineState,
} from "@/design-system/resource-timeline-tokens";
import type { AvailabilityCalendarEntry } from "@shared/ipc-types";

export type AvailabilityCellState = Extract<
  ResourceTimelineState,
  "free" | "partial" | "blocked" | "critical" | "rest"
>;

/** Chronology = ResourceTimeline; list = compact secondary view. Matrix removed. */
export type AvailabilityViewMode = "chronology" | "list";

export { RESOURCE_TIMELINE_HORIZON_PRESETS as AVAILABILITY_HORIZON_PRESETS };
export { RESOURCE_TIMELINE_DEFAULT_HORIZON_DAYS as AVAILABILITY_DEFAULT_HORIZON_DAYS };

/** Default local wall-clock bounds when seeding a block from a calendar day (planning day, not HR). */
export const AVAILABILITY_BLOCK_SEED_START_LOCAL = "08:00";
export const AVAILABILITY_BLOCK_SEED_END_LOCAL = "17:00";

export function toLocalIsoDate(value: Date): string {
  const y = value.getFullYear();
  const m = String(value.getMonth() + 1).padStart(2, "0");
  const d = String(value.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function addDaysLocal(isoDate: string, days: number): string {
  // split() always yields at least one element; NaN propagates to an Invalid Date as before.
  const [y = Number.NaN, m = 1, d = 1] = isoDate.split("-").map(Number);
  const dt = new Date(y, m - 1, d);
  dt.setDate(dt.getDate() + days);
  return toLocalIsoDate(dt);
}

export function eachDateInclusive(from: string, to: string): string[] {
  if (from > to) return [];
  const out: string[] = [];
  let cur = from;
  while (cur <= to) {
    out.push(cur);
    cur = addDaysLocal(cur, 1);
  }
  return out;
}

/** Parse ISO-ish datetime to epoch ms; returns null if invalid. */
export function parseBlockTimeMs(value: string): number | null {
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

export function deriveCellState(row: AvailabilityCalendarEntry): AvailabilityCellState {
  if (row.scheduled_minutes <= 0) return "rest";
  if (row.has_critical_block) return "critical";
  if (row.available_minutes <= 0 && row.blocked_minutes > 0) return "blocked";
  if (row.blocked_minutes > 0 && row.available_minutes > 0) return "partial";
  return "free";
}

export interface AvailabilityPersonRow {
  personnel_id: number;
  employee_code: string;
  full_name: string;
  team_id: number | null;
  team_name: string | null;
  cells: Map<string, AvailabilityCalendarEntry>;
}

export function buildPersonRows(entries: AvailabilityCalendarEntry[]): AvailabilityPersonRow[] {
  const map = new Map<number, AvailabilityPersonRow>();
  for (const entry of entries) {
    let row = map.get(entry.personnel_id);
    if (!row) {
      row = {
        personnel_id: entry.personnel_id,
        employee_code: entry.employee_code,
        full_name: entry.full_name,
        team_id: entry.team_id,
        team_name: entry.team_name,
        cells: new Map(),
      };
      map.set(entry.personnel_id, row);
    }
    row.cells.set(entry.work_date, entry);
    if (entry.team_name && !row.team_name) {
      row.team_name = entry.team_name;
      row.team_id = entry.team_id;
    }
  }
  return [...map.values()].sort((a, b) => {
    const teamCmp = (a.team_name ?? "").localeCompare(b.team_name ?? "");
    if (teamCmp !== 0) return teamCmp;
    return a.full_name.localeCompare(b.full_name);
  });
}

/** Page-level horizon KPIs — formula parked for catalog placement; not rendered in Availability UI. */
export interface AvailabilitySummary {
  availableTechnicians: number;
  blockedTechnicians: number;
  criticalConstraintDays: number;
  coverageRiskDays: number;
}

/** See `.cursor/rules/kpi-catalog.mdc` staging — Availability page summary KPIs. */
export function computeAvailabilitySummary(
  people: AvailabilityPersonRow[],
  dates: string[],
): AvailabilitySummary {
  let availableTechnicians = 0;
  let blockedTechnicians = 0;
  let criticalConstraintDays = 0;

  for (const person of people) {
    let freeDays = 0;
    let blockedOrCriticalDays = 0;
    for (const date of dates) {
      const cell = person.cells.get(date);
      if (!cell) continue;
      const state = deriveCellState(cell);
      if (state === "free") freeDays += 1;
      if (state === "blocked" || state === "critical") blockedOrCriticalDays += 1;
      if (state === "critical") criticalConstraintDays += 1;
    }
    const scoredDays = dates.filter((d) => person.cells.has(d)).length;
    if (scoredDays > 0 && freeDays * 2 >= scoredDays) {
      availableTechnicians += 1;
    }
    if (blockedOrCriticalDays > 0) blockedTechnicians += 1;
  }

  let coverageRiskDays = 0;
  for (const date of dates) {
    let freeHeadcount = 0;
    for (const person of people) {
      const cell = person.cells.get(date);
      if (!cell) continue;
      if (deriveCellState(cell) === "free") freeHeadcount += 1;
    }
    if (people.length > 0 && freeHeadcount === 0) coverageRiskDays += 1;
  }

  return {
    availableTechnicians,
    blockedTechnicians,
    criticalConstraintDays,
    coverageRiskDays,
  };
}

/** Legend / list chips — token-backed, not ad-hoc hex. */
export const CELL_STATE_CLASS: Record<AvailabilityCellState, string> = {
  free: RESOURCE_TIMELINE_STATE_CLASS.free,
  partial: RESOURCE_TIMELINE_STATE_CLASS.partial,
  blocked: RESOURCE_TIMELINE_STATE_CLASS.blocked,
  critical: RESOURCE_TIMELINE_STATE_CLASS.critical,
  rest: RESOURCE_TIMELINE_STATE_CLASS.rest,
};
