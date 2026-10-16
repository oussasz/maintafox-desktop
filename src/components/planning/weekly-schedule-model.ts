/** Pure helpers of the weekly schedule view (planning step D). */

import { describeAppError } from "@/lib/app-error";
import type { ScheduleCommitment } from "@shared/ipc-types";

/** The coded reasons of a refusal when there are some, else its translated title. */
export function errorText(err: unknown): string {
  const { title, description } = describeAppError(err);
  return description ?? title;
}

/** `YYYY-MM-DD` of a local date. */
export function isoDay(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** A `YYYY-MM-DD` day as a local date (not UTC midnight, which would shift west of UTC). */
export function parseDay(day: string): Date {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

export function addDays(day: string, n: number): string {
  const date = parseDay(day);
  date.setDate(date.getDate() + n);
  return isoDay(date);
}

/** Every day from `start` to `end` (inclusive). A site week has 7 days, or 6 to 13 when it
 * straddles a change of the site's weekend. */
export function daysBetween(start: string, end: string): string[] {
  const days: string[] = [];
  for (let day = start; day <= end; day = addDays(day, 1)) days.push(day);
  return days;
}

/** Commitments of one crew whose slot touches one local day; an end at midnight stays the day before. */
export function commitmentsOnDay(
  commitments: ScheduleCommitment[],
  teamId: number,
  day: string,
): ScheduleCommitment[] {
  return commitments.filter((c) => {
    if (c.assigned_team_id !== teamId) return false;
    const start = isoDay(new Date(c.committed_start));
    const end = isoDay(new Date(new Date(c.committed_end).getTime() - 1));
    return start <= day && day <= end;
  });
}
