import type { MobileLanguage, MobileT } from "../i18n";

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

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * How long ago a stored instant (ISO 8601) was, as a phrase: "à l'instant" under a minute,
 * "il y a 5 min" under an hour, "il y a 3 h" under a day, then "le <date and time>". An instant
 * slightly in the future (clock skew) reads as "à l'instant". Null when absent or unreadable.
 */
export function formatRelativeTime(
  value: string | null | undefined,
  now: number,
  language: MobileLanguage,
  t: MobileT,
): string | null {
  if (!value) return null;
  const at = new Date(value).getTime();
  if (Number.isNaN(at)) return null;
  const elapsed = now - at;
  if (elapsed < MINUTE_MS) return t("time.justNow");
  if (elapsed < HOUR_MS) return t("time.minutesAgo", { n: Math.floor(elapsed / MINUTE_MS) });
  if (elapsed < DAY_MS) return t("time.hoursAgo", { n: Math.floor(elapsed / HOUR_MS) });
  const date = formatDateTime(value, language);
  return date ? t("time.on", { date }) : null;
}
