/**
 * Finance catalogs from the governed reference data (Données de référence): cost categories and
 * variance reasons. Finance never defines these lists itself — plants maintain them in the
 * reference manager. Labels: the catalog label (French), or the English term of a product value
 * when the interface is in English. A code missing from the catalog is shown as its code, never
 * as an invented label.
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type FinanceCatalogValue,
  type FinanceCatalogs,
  listFinanceCatalogs,
} from "@/services/finance-cost-service";

let cache: Promise<FinanceCatalogs> | null = null;

/** Clears the cached catalogs (after a reference publish, and between tests). */
export function resetFinanceCatalogCache(): void {
  cache = null;
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
}

export function useFinanceCatalogs(): FinanceCatalogsState {
  const { i18n, t } = useTranslation("budget");
  const [catalogs, setCatalogs] = useState<FinanceCatalogs | null>(null);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    let cancelled = false;
    cache ??= listFinanceCatalogs();
    cache
      .then((c) => {
        if (!cancelled) setCatalogs(c);
      })
      .catch((err: unknown) => {
        cache = null;
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, []);

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
    };
  }, [catalogs, error, language, t]);
}
