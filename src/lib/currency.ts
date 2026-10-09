/**
 * Tenant display currency — ISO 4217 codes.
 * SSOT for runtime display: `app_settings.locale.currency_code` via locale-store.
 * Do not hardcode EUR/USD/DZD in UI modules; use `useFormatters().formatCurrency`.
 */

/** Used only when settings have not loaded yet (offline bootstrap / tests without IPC). */
export const FALLBACK_CURRENCY_CODE = "DZD";

/** Setting key in `app_settings` (tenant scope). */
export const LOCALE_CURRENCY_SETTING_KEY = "locale.currency_code";

/** Common ISO 4217 codes offered in Settings → Localization. */
export const CURRENCY_CODE_OPTIONS = [
  "DZD",
  "MAD",
  "TND",
  "EUR",
  "USD",
  "GBP",
  "CHF",
  "CAD",
  "SAR",
  "AED",
] as const;

export type SupportedCurrencyCode = (typeof CURRENCY_CODE_OPTIONS)[number];

export function isIso4217CurrencyCode(code: string): boolean {
  return /^[A-Z]{3}$/.test(code.trim());
}

export function normalizeCurrencyCode(raw: unknown): string {
  if (typeof raw !== "string") return FALLBACK_CURRENCY_CODE;
  const code = raw.trim().toUpperCase();
  return isIso4217CurrencyCode(code) ? code : FALLBACK_CURRENCY_CODE;
}
