/**
 * Shift rules of the work-order planning tab (backend: `wo::shifts`).
 *
 * A save sends the shift only when the planner changed it: otherwise the stored shift is kept as
 * it was planned (and checked again by the backend only when the date or the team moves).
 */
import type { WorkOrder } from "@shared/ipc-types";

export interface ShiftSaveFields {
  shift_class_version_id?: number;
  clear_shift?: boolean;
}

/** Shift fields of a planning save or an assignment: nothing unless the planner changed it. */
export function shiftSaveFields(
  touched: boolean,
  value: string,
  wo: Pick<WorkOrder, "shift" | "shift_class_version_id">,
): ShiftSaveFields {
  if (!touched) return {};
  if (value) return { shift_class_version_id: Number(value) };
  // "No shift" chosen: only clears something that is there (a window or an old code).
  return wo.shift_class_version_id != null || wo.shift != null ? { clear_shift: true } : {};
}

function minutes(hhmm: string): number | null {
  const m = /^(\d{2}):(\d{2})$/.exec(hhmm);
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
}

/**
 * Whether the planned start / end fall outside the chosen window (a warning, never a refusal).
 * Times are read in local time, as the planner typed them or as stored. A window ending at or
 * before its start runs past midnight; `startsPreviousDay` is the previous night's shift still
 * running at the planned start.
 */
export function plannedOutsideWindow(
  plannedStart: string | null,
  plannedEnd: string | null,
  window: { shift_start: string; shift_end: string; starts_previous_day?: boolean },
): boolean {
  if (!plannedStart) return false;
  const start = new Date(plannedStart);
  if (Number.isNaN(start.getTime())) return false;
  const s = minutes(window.shift_start);
  const e = minutes(window.shift_end);
  if (s == null || e == null) return false;
  const overnight = e <= s;
  const winStart = window.starts_previous_day ? s - 24 * 60 : s;
  const winEnd = window.starts_previous_day ? e : overnight ? e + 24 * 60 : e;
  const midnight = new Date(start);
  midnight.setHours(0, 0, 0, 0);
  const at = (d: Date) => (d.getTime() - midnight.getTime()) / 60000;
  if (at(start) < winStart || at(start) > winEnd) return true;
  if (!plannedEnd) return false;
  const end = new Date(plannedEnd);
  return !Number.isNaN(end.getTime()) && at(end) > winEnd;
}
