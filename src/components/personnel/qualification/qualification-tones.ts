/**
 * Qualification states → design-system tones (same meaning, same colour everywhere):
 * valid = success, runs out soon = attention (orange is for what needs attention), expired /
 * rejected / missing-required = danger, waiting for verification = info, self-declared = neutral.
 */
import type { Tone } from "@/design-system/tone";
import type { GapStatus, MatrixCellStatus } from "@/services/qualification-gate-service";

export const MATRIX_CELL_TONE: Record<MatrixCellStatus, Tone> = {
  valid: "success",
  expiring_soon: "attention",
  expired: "danger",
  pending: "info",
  rejected: "danger",
  unvalidated: "neutral",
};

export const GAP_STATUS_TONE: Record<GapStatus, Tone> = {
  missing: "danger",
  expired: "danger",
  expires_before_work: "attention",
  pending_verification: "info",
  rejected: "danger",
  level_too_low: "warning",
  unvalidated: "neutral",
};

/** `readiness_status` of a recorded certificate (backend `readiness_status`). */
export const CERT_READINESS_TONE: Record<string, Tone> = {
  valid: "success",
  expiring_soon: "attention",
  expired: "danger",
  pending: "info",
  rejected: "danger",
};

/** Capacity day reasons (`personnel::capacity`). */
export const DAY_REASON_TONE: Record<string, Tone> = {
  working: "success",
  absent: "warning",
  rest_day: "neutral",
  site_rest_day: "neutral",
  holiday: "info",
  no_schedule: "warning",
  timetable_not_set_up: "warning",
  not_employed: "neutral",
};

export const CAPACITY_BASIS_TONE: Record<string, Tone> = {
  roster: "success",
  rest_day: "neutral",
  holiday: "info",
  no_schedule: "warning",
  no_members: "warning",
};

/** Days left before `expiresAt` (ISO date or timestamp) counted from `today` (YYYY-MM-DD). */
export function daysUntil(expiresAt: string, today: string): number | null {
  const end = Date.parse(`${expiresAt.slice(0, 10)}T00:00:00Z`);
  const start = Date.parse(`${today.slice(0, 10)}T00:00:00Z`);
  if (Number.isNaN(end) || Number.isNaN(start)) return null;
  return Math.round((end - start) / 86_400_000);
}

/** Expiry date = issue date + validity months (calendar months, clamped to month end). */
export function addMonths(isoDate: string, months: number): string | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(isoDate);
  if (!m) return null;
  const year = Number(m[1]);
  const month = Number(m[2]) - 1 + months;
  const day = Number(m[3]);
  const target = new Date(Date.UTC(year, month, 1));
  const lastDay = new Date(
    Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0),
  ).getUTCDate();
  target.setUTCDate(Math.min(day, lastDay));
  return target.toISOString().slice(0, 10);
}

/** Today as YYYY-MM-DD in local time. */
export function todayIso(now: Date = new Date()): string {
  const y = now.getFullYear();
  const mo = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${mo}-${d}`;
}
