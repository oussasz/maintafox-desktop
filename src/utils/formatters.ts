/**
 * formatters.ts
 *
 * Pure locale-aware formatting utilities. All functions are stateless and take
 * an explicit `locale` parameter. Components MUST use the `useFormatters()` hook
 * which binds the current locale automatically.
 *
 * Supported locales: "fr" (→ fr-FR), "en" (→ en-US)
 * Phase 3: "ar" (→ ar-DZ) added here without changing the API.
 */

/** Maps our two-letter locale codes to BCP 47 locale strings for Intl APIs. */
const INTL_LOCALE_MAP: Record<string, string> = {
  fr: "fr-FR",
  en: "en-US",
  ar: "ar-DZ",
};

function toIntlLocale(locale: string): string {
  return INTL_LOCALE_MAP[locale] ?? "fr-FR";
}

// ─── Number Formatting ─────────────────────────────────────────────────────

/**
 * Formats a number according to the active locale.
 *
 * French:  1 234,56   (narrow no-break space as thousands, comma as decimal)
 * English: 1,234.56   (comma as thousands, period as decimal)
 */
export function formatNumber(
  value: number,
  locale: string,
  options: Intl.NumberFormatOptions = {},
): string {
  if (!Number.isFinite(value)) return "\u2014";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2,
    ...options,
  }).format(value);
}

/**
 * Formats a number as a decimal with a fixed number of decimal places.
 * Useful for quantity fields where precision is always required.
 */
export function formatDecimal(value: number, locale: string, decimalPlaces = 2): string {
  if (!Number.isFinite(value)) return "\u2014";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    minimumFractionDigits: decimalPlaces,
    maximumFractionDigits: decimalPlaces,
  }).format(value);
}

/**
 * Formats a monetary amount.
 *
 * French:  1 234,56 €    (amount + space + symbol, fr-FR convention)
 * English: $1,234.56     (symbol + amount, en-US convention)
 *
 * Uses Intl.NumberFormat with style: "currency" which handles symbol placement
 * automatically per locale.
 */
export function formatCurrency(value: number, locale: string, currency = "EUR"): string {
  if (!Number.isFinite(value)) return "\u2014";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    style: "currency",
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

/**
 * Formats a number as a percentage.
 * French:  45 %
 * English: 45%
 */
export function formatPercent(value: number, locale: string): string {
  if (!Number.isFinite(value)) return "\u2014";
  return new Intl.NumberFormat(toIntlLocale(locale), {
    style: "percent",
    minimumFractionDigits: 0,
    maximumFractionDigits: 1,
  }).format(value / 100);
}

// ─── Date Formatting ───────────────────────────────────────────────────────

/**
 * Formats a Date (or ISO string / timestamp) as a short date.
 *
 * French:  31/03/2026
 * English: 03/31/2026
 *
 * Uses explicit day/month/year options to avoid OS-level locale format
 * interpretation differences.
 */
export function formatDate(
  value: Date | string | number | null | undefined,
  locale: string,
): string {
  if (value == null) return "\u2014";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "\u2014";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(date);
}

/**
 * Formats a Date as a long human-readable date.
 *
 * French:  31 mars 2026
 * English: March 31, 2026
 */
export function formatDateLong(
  value: Date | string | number | null | undefined,
  locale: string,
): string {
  if (value == null) return "\u2014";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "\u2014";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(date);
}

/**
 * Formats a Date including time.
 *
 * French:  31/03/2026 14:30
 * English: 03/31/2026, 2:30 PM
 */
export function formatDateTime(
  value: Date | string | number | null | undefined,
  locale: string,
): string {
  if (value == null) return "\u2014";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "\u2014";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: locale === "en",
  }).format(date);
}

/**
 * Formats a time-only value.
 *
 * French:  14:30
 * English: 2:30 PM
 */
export function formatTime(
  value: Date | string | number | null | undefined,
  locale: string,
): string {
  if (value == null) return "\u2014";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "\u2014";
  return new Intl.DateTimeFormat(toIntlLocale(locale), {
    hour: "2-digit",
    minute: "2-digit",
    hour12: locale === "en",
  }).format(date);
}

/**
 * Formats a relative time (e.g., "2 minutes ago", "il y a 2 minutes").
 * Uses Intl.RelativeTimeFormat. Falls back to absolute date for values
 * older than 7 days.
 */
export function formatRelativeTime(
  value: Date | string | number | null | undefined,
  locale: string,
): string {
  if (value == null) return "\u2014";
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "\u2014";

  const now = Date.now();
  const diffMs = date.getTime() - now;
  const diffSeconds = Math.round(diffMs / 1000);
  const absSeconds = Math.abs(diffSeconds);

  const rtf = new Intl.RelativeTimeFormat(toIntlLocale(locale), {
    numeric: "auto",
  });

  if (absSeconds < 60) return rtf.format(diffSeconds, "second");
  if (absSeconds < 3600) return rtf.format(Math.round(diffSeconds / 60), "minute");
  if (absSeconds < 86400) return rtf.format(Math.round(diffSeconds / 3600), "hour");
  if (absSeconds < 604800) return rtf.format(Math.round(diffSeconds / 86400), "day");

  // Older than 7 days → absolute date
  return formatDate(date, locale);
}

// ─── Text Direction (RTL readiness) ────────────────────────────────────────

const RTL_LOCALES = new Set(["ar", "he", "fa", "ur"]);

/** Returns the text direction for a given locale. */
export function getLocaleDirection(locale: string): "ltr" | "rtl" {
  return RTL_LOCALES.has(locale) ? "rtl" : "ltr";
}
