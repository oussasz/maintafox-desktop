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
 * Formats a monetary amount using the given ISO 4217 currency code.
 * Callers must pass the tenant currency from settings — there is no hardcoded default.
 * UI components should use `useFormatters().formatCurrency` which binds Settings → Localization.
 */
export function formatCurrency(
  value: number,
  locale: string,
  currency: string,
  symbol?: string | null,
  minorUnits = 2,
): string {
  if (!Number.isFinite(value)) return "\u2014";
  const unit = symbol?.trim();
  const parts = new Intl.NumberFormat(toIntlLocale(locale), {
    style: "currency",
    currency,
    // With the reference list's own symbol ("DA") the ISO code is only a placeholder for it.
    ...(unit ? { currencyDisplay: "code" as const } : {}),
    minimumFractionDigits: minorUnits,
    maximumFractionDigits: minorUnits,
  }).formatToParts(value);
  // The symbol sits where the language places a currency (fr: after the amount, en: before).
  return parts.map((p) => (p.type === "currency" && unit ? unit : p.value)).join("");
}

/**
 * A duration in hours: "3,5 h" (fr), "3.5 h" (en). Whole hours show no decimals; at most one
 * decimal is kept.
 */
export function formatHours(value: number, locale: string): string {
  if (!Number.isFinite(value)) return "\u2014";
  return `${formatNumber(value, locale, { maximumFractionDigits: 1 })}\u00a0h`;
}

const BYTE_UNITS: Record<string, readonly string[]> = {
  fr: ["o", "Ko", "Mo", "Go", "To"],
  en: ["B", "KB", "MB", "GB", "TB"],
};

/** A file size: "1,5 Mo" (fr), "1.5 MB" (en); binary steps (1 024). */
export function formatBytes(bytes: number, locale: string): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "\u2014";
  const units = BYTE_UNITS[locale] ?? BYTE_UNITS["fr"] ?? [];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i += 1;
  }
  return `${formatNumber(n, locale, { maximumFractionDigits: i === 0 ? 0 : 1 })}\u00a0${units[i]}`;
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
 * A date-only text ("2026-10-22", a calendar day with no time) is that day in the user's own
 * time zone; `new Date("2026-10-22")` would be midnight UTC, i.e. the day before west of Greenwich.
 */
function toDate(value: Date | string | number): Date {
  if (value instanceof Date) return value;
  if (typeof value === "string") {
    const day = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
    if (day) return new Date(Number(day[1]), Number(day[2]) - 1, Number(day[3]));
  }
  return new Date(value);
}

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
  const date = toDate(value);
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
  const date = toDate(value);
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
  const date = toDate(value);
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
  const date = toDate(value);
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
  const date = toDate(value);
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
