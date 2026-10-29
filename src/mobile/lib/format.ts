import type { MobileLanguage } from "../i18n";

/**
 * The locale used for dates and numbers. Arabic uses the Maghreb variant (Latin digits, as on
 * plant documents and equipment plates).
 */
export function intlLocale(language: MobileLanguage): string {
  return language === "ar" ? "ar-DZ" : language;
}

/** A stored instant (ISO 8601) as a short local date and time, or null when absent or unreadable. */
export function formatDateTime(
  value: string | null | undefined,
  language: MobileLanguage,
): string | null {
  if (!value) return null;
  const at = new Date(value);
  if (Number.isNaN(at.getTime())) return null;
  return new Intl.DateTimeFormat(intlLocale(language), {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(at);
}

/** Hours as "1,5 h" in the app language. */
export function formatHours(hours: number, language: MobileLanguage): string {
  return new Intl.NumberFormat(intlLocale(language), { maximumFractionDigits: 1 }).format(hours);
}
