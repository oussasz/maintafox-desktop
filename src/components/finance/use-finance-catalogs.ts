/**
 * Finance catalogs from the governed reference data (Données de référence): cost categories and
 * variance reasons. Finance never defines these lists itself — plants maintain them in the
 * reference manager. Labels: the catalog label (French), or the English term of a product value
 * when the interface is in English. A code missing from the catalog is shown as its code, never
 * as an invented label.
 *
 * Read on every mount (never cached across screens), so a value published in the reference
 * manager or a fiscal-year change in Settings shows on the next screen. Components mounting
 * together share the request in flight.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type FinanceCatalogValue,
  type FinanceCatalogs,
  listFinanceCatalogs,
} from "@/services/finance-cost-service";

let inFlight: Promise<FinanceCatalogs> | null = null;

function fetchFinanceCatalogs(): Promise<FinanceCatalogs> {
  inFlight ??= listFinanceCatalogs().finally(() => {
    inFlight = null;
  });
  return inFlight;
}

/** Calendar month (1..12) of a fiscal period (1..12) for a fiscal year starting in `start`. */
export function periodCalendarMonth(period: number, start: number): number {
  return ((start - 1 + period - 1) % 12) + 1;
}

/** Display label of a catalog value in the interface language. */
export function catalogLabel(value: FinanceCatalogValue, language: string): string {
  return language.toLowerCase().startsWith("en") && value.description
    ? value.description
    : value.label;
}

export interface FinanceCatalogsState {
  /** Active cost categories, catalog order. */
  categories: FinanceCatalogValue[];
  /** Active variance reasons, catalog order. */
  reasons: FinanceCatalogValue[];
  categoryLabel: (code: string) => string;
  reasonLabel: (code: string) => string;
  /** Fiscal-year start month (Settings › Localization); null until loaded. */
  fiscalStartMonth: number | null;
  /** Name of the calendar month of a fiscal period (1..12), e.g. period 1 = "Juillet". */
  periodLabel: (period: number) => string;
  loaded: boolean;
  error: unknown;
  /** Loads the catalogs again after a failure. */
  retry: () => void;
}

export function useFinanceCatalogs(): FinanceCatalogsState {
  const { i18n, t } = useTranslation("budget");
  const [catalogs, setCatalogs] = useState<FinanceCatalogs | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt = explicit retry
  useEffect(() => {
    let cancelled = false;
    setError(null);
    fetchFinanceCatalogs()
      .then((c) => {
        if (!cancelled) setCatalogs(c);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const language = i18n.language ?? "fr";
  return useMemo(() => {
    const find = (list: FinanceCatalogValue[] | undefined, code: string) => {
      const value = list?.find((v) => v.code.toLowerCase() === code.toLowerCase());
      return value ? catalogLabel(value, language) : code;
    };
    return {
      categories: (catalogs?.cost_categories ?? []).filter((v) => v.is_active),
      reasons: (catalogs?.variance_reasons ?? []).filter((v) => v.is_active),
      categoryLabel: (code: string) => find(catalogs?.cost_categories, code),
      reasonLabel: (code: string) => find(catalogs?.variance_reasons, code),
      fiscalStartMonth: catalogs?.fiscal_year_start_month ?? null,
      periodLabel: (period: number) =>
        catalogs
          ? t(`month.${periodCalendarMonth(period, catalogs.fiscal_year_start_month)}`)
          : String(period),
      loaded: catalogs != null,
      error,
      retry,
    };
  }, [catalogs, error, language, t, retry]);
}
