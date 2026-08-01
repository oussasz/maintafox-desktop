/**
 * Human-readable duration formatting for WO execution (never raw decimals).
 */

export type DurationFormatStyle = "auto" | "hm" | "clock" | "minutes";

function roundMinutes(totalMinutes: number): number {
  if (!Number.isFinite(totalMinutes) || totalMinutes < 0) return 0;
  return Math.round(totalMinutes);
}

/** Convert hours (f64 from backend) to whole minutes for display. */
export function hoursToMinutes(hours: number | null | undefined): number {
  if (hours == null || !Number.isFinite(hours) || hours < 0) return 0;
  return roundMinutes(hours * 60);
}

/**
 * Format a duration given in minutes.
 * - `hm`: `2 h 30` or `2 h` when exact hours
 * - `minutes`: `45 min`
 * - `clock`: `00:15` / `02:30`
 * - `auto`: &lt; 60 → `45 min`; else `2 h 30`
 */
export function formatDurationMinutes(
  minutes: number | null | undefined,
  style: DurationFormatStyle = "auto",
  labels: { hours: string; minutes: string } = { hours: "h", minutes: "min" },
): string {
  const m = roundMinutes(minutes ?? 0);
  const h = Math.floor(m / 60);
  const rem = m % 60;

  if (style === "clock") {
    return `${String(h).padStart(2, "0")}:${String(rem).padStart(2, "0")}`;
  }
  if (style === "minutes" || (style === "auto" && h === 0)) {
    return `${m} ${labels.minutes}`;
  }
  if (rem === 0) {
    return `${h} ${labels.hours}`;
  }
  return `${h} ${labels.hours} ${rem}`;
}

/** Format `hours_worked` from labor DTO. */
export function formatHoursWorked(
  hours: number | null | undefined,
  style: DurationFormatStyle = "auto",
  labels?: { hours: string; minutes: string },
): string {
  return formatDurationMinutes(hoursToMinutes(hours), style, labels);
}

/** Elapsed minutes between two ISO timestamps (null-safe). */
export function elapsedMinutesBetween(
  startedAt: string | null | undefined,
  endedAt: string | null | undefined,
): number | null {
  if (!startedAt || !endedAt) return null;
  const start = Date.parse(startedAt);
  const end = Date.parse(endedAt);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null;
  return roundMinutes((end - start) / 60_000);
}
