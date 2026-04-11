/**
 * Shared date-formatting helper.
 * Extracted to deduplicate formatDate found in WoDetailDialog, WoArchivePanel, etc.
 * Sprint S5 (GA-030).
 */

/**
 * Format a date string for display using the user's current i18n locale.
 * Falls back to "—" for null/undefined/empty values.
 */
export function formatDate(
  dateStr: string | null | undefined,
  locale: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  if (!dateStr) return "—";
  try {
    return new Date(dateStr).toLocaleDateString(locale, options);
  } catch {
    return "—";
  }
}

/**
 * Format a date string with time.
 */
export function formatDateTime(dateStr: string | null | undefined, locale: string): string {
  return formatDate(dateStr, locale, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Short date format (e.g. for Kanban cards).
 */
export function formatShortDate(dateStr: string | null | undefined, locale: string): string {
  return formatDate(dateStr, locale, {
    day: "numeric",
    month: "short",
  });
}
