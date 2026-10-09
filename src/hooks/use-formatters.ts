/**
 * use-formatters.ts
 *
 * React hook that binds the current active locale and tenant currency
 * (from the locale store / Settings) to all pure formatter functions.
 * Components MUST use this hook — they must NOT import and call the raw
 * formatters with a hardcoded locale or currency string.
 *
 * When the user switches language or currency in Settings, components
 * using this hook re-render with correctly formatted values.
 *
 * Usage:
 *   const { formatDate, formatNumber, formatCurrency } = useFormatters();
 *   return <td>{formatCurrency(line.cost)}</td>;
 */

import { useMemo } from "react";

import { useLocaleStore } from "@/stores/locale-store";
import {
  formatDate,
  formatDateLong,
  formatDateTime,
  formatTime,
  formatRelativeTime,
  formatNumber,
  formatDecimal,
  formatCurrency,
  formatPercent,
} from "@/utils/formatters";

export interface Formatters {
  /** "31/03/2026" (fr) or "03/31/2026" (en) */
  formatDate: (value: Date | string | number | null | undefined) => string;
  /** "31 mars 2026" (fr) or "March 31, 2026" (en) */
  formatDateLong: (value: Date | string | number | null | undefined) => string;
  /** "31/03/2026 14:30" (fr) or "03/31/2026, 2:30 PM" (en) */
  formatDateTime: (value: Date | string | number | null | undefined) => string;
  /** "14:30" (fr) or "2:30 PM" (en) */
  formatTime: (value: Date | string | number | null | undefined) => string;
  /** "il y a 5 minutes" (fr) or "5 minutes ago" (en) */
  formatRelativeTime: (value: Date | string | number | null | undefined) => string;
  /** "1 234,56" (fr) or "1,234.56" (en) */
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
  /** Fixed decimal places: "1 234,56" (fr) */
  formatDecimal: (value: number, decimalPlaces?: number) => string;
  /**
   * Money using tenant `locale.currency_code` from Settings.
   * Optional override only for rare multi-currency displays (e.g. FX line).
   */
  formatCurrency: (value: number, currencyOverride?: string) => string;
  /** "45,5 %" (fr) or "45.5%" (en) */
  formatPercent: (value: number) => string;
  /** Current locale code ("fr" or "en") */
  locale: string;
  /** Active tenant ISO 4217 currency code */
  currencyCode: string;
}

export function useFormatters(): Formatters {
  const locale = useLocaleStore((s) => s.activeLocale);
  const currencyCode = useLocaleStore((s) => s.activeCurrencyCode);

  return useMemo(
    () => ({
      formatDate: (v) => formatDate(v, locale),
      formatDateLong: (v) => formatDateLong(v, locale),
      formatDateTime: (v) => formatDateTime(v, locale),
      formatTime: (v) => formatTime(v, locale),
      formatRelativeTime: (v) => formatRelativeTime(v, locale),
      formatNumber: (v, opts) => formatNumber(v, locale, opts),
      formatDecimal: (v, dp) => formatDecimal(v, locale, dp),
      formatCurrency: (v, cur) => formatCurrency(v, locale, cur ?? currencyCode),
      formatPercent: (v) => formatPercent(v, locale),
      locale,
      currencyCode,
    }),
    [locale, currencyCode],
  );
}
