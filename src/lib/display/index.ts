/**
 * Presentation-only display formatters.
 * Never accept or render raw database IDs — callers pass enriched DTO fields.
 */

const DASH = "—";

function trimOrEmpty(value: string | null | undefined): string {
  return (value ?? "").trim();
}

/** Empty / whitespace → em dash. */
export function formatOrDash(value: string | null | undefined): string {
  const v = trimOrEmpty(value);
  return v.length > 0 ? v : DASH;
}

/** Asset business label: `CODE — Name`. */
export function formatAssetLabel(
  code: string | null | undefined,
  name: string | null | undefined,
): string {
  const c = trimOrEmpty(code);
  const n = trimOrEmpty(name);
  if (c && n) return `${c} — ${n}`;
  if (c) return c;
  if (n) return n;
  return DASH;
}

/** Person label: display name, else username; optional role suffix. */
export function formatPersonLabel(
  displayName: string | null | undefined,
  username?: string | null,
  role?: string | null,
): string {
  const primary = trimOrEmpty(displayName) || trimOrEmpty(username);
  if (!primary) return DASH;
  const r = trimOrEmpty(role);
  return r ? `${primary} (${r})` : primary;
}

/** Org node label: `CODE — Name` (Phase 1; full path later). */
export function formatOrgNodeLabel(
  code: string | null | undefined,
  name: string | null | undefined,
): string {
  return formatAssetLabel(code, name);
}

/**
 * Site / warehouse / location label: `CODE — Name` when name exists, else code only.
 * Prefer this over rendering bare codes when enrichment is available.
 */
export function formatSiteLabel(
  code: string | null | undefined,
  name: string | null | undefined,
): string {
  return formatAssetLabel(code, name);
}

/** DI / WO business code. */
export function formatEntityCode(code: string | null | undefined): string {
  return formatOrDash(code);
}

export { DASH as DISPLAY_DASH };
export {
  elapsedMinutesBetween,
  formatDurationMinutes,
  formatHoursWorked,
  hoursToMinutes,
  type DurationFormatStyle,
} from "./format-duration";

