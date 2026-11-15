/**
 * Shared date-formatting helper.
 * Extracted to deduplicate formatDate found in WoDetailDialog, WoArchivePanel, etc.
 * Sprint S5 (GA-030).
 */

import {
  formatDate as formatLocalDate,
  formatDateTime as formatLocalDateTime,
  formatDayMonth,
  toDate,
} from "@/utils/formatters";

/** Map i18next language code (e.g. `fr`, `en`) to a BCP 47 tag for `Intl` APIs. */
export function intlLocaleForLanguage(lang: string | undefined): string {
  if (!lang) return "fr-FR";
  if (lang.startsWith("fr")) return "fr-FR";
  if (lang.startsWith("en")) return "en-US";
  return lang;
}

/**
 * Format a date string for display using the user's current i18n locale: "31/10/2026" (fr).
 * A calendar day stays that day and an instant is shown in the user's own time zone (see
 * `toDate`). Falls back to "—" for null/undefined/empty values. With `options` the caller picks a
 * custom Intl layout.
 */
export function formatDate(
  dateStr: string | null | undefined,
  locale: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  if (!dateStr) return "—";
  if (!options) return formatLocalDate(dateStr, locale);
  try {
    const date = toDate(dateStr);
    if (Number.isNaN(date.getTime())) return "—";
    return date.toLocaleDateString(intlLocaleForLanguage(locale), options);
  } catch {
    return "—";
  }
}

/** Format a date string with time: "31/10/2026 22:00" (fr), in the user's own time zone. */
export function formatDateTime(dateStr: string | null | undefined, locale: string): string {
  if (!dateStr) return "—";
  return formatLocalDateTime(dateStr, locale);
}

/** Short date format (e.g. for Kanban cards): "31/10" (fr). */
export function formatShortDate(dateStr: string | null | undefined, locale: string): string {
  if (!dateStr) return "—";
  return formatDayMonth(dateStr, locale);
}
