/**
 * Wording of the live fleet reliability figures (`study::fleet_kpis`): one source for the RAMS
 * home, the Analytique hub and the Vue d'ensemble cards, so the same figure is worded the same way
 * everywhere (L2-10).
 */
import { useTranslation } from "react-i18next";

/** Shortest window giving MTBF / MTTR / Aₒ (`fleet_kpis::MIN_RATE_WINDOW_DAYS`). */
export const MIN_RATE_WINDOW_DAYS = 90;

/** How many equipment count operating hours and how many calendar hours. */
export interface FleetBasisCounts {
  operating_basis_count: number;
  calendar_basis_count: number;
}

/** What a rate card needs of the live figures (`FleetKpis` has it all). */
export interface FleetRateCounts extends FleetBasisCounts {
  window_too_short: boolean;
  mtbf_excluded_missing_date: number;
  mtbf_excluded_outside_service: number;
  restore_excluded_missing_failed_at: number;
  restore_excluded_missing_restored_at: number;
  restore_excluded_inconsistent: number;
}

/** A rate figure of the fleet: its card states window, basis (MTBF, Aₒ) and exclusions. */
export type FleetRateFigure = "mtbf" | "mttr" | "availability";

export function useFleetKpiText() {
  const { t } = useTranslation("reliability");
  /** "sur heures de marche", "sur heures calendaires …" or the mix of both. */
  const basis = (k: FleetBasisCounts | null | undefined): string =>
    k == null
      ? ""
      : k.calendar_basis_count === 0
        ? t("rams.fleet.basisOperating")
        : k.operating_basis_count === 0
          ? t("rams.fleet.basisCalendar")
          : t("rams.fleet.basisMixed", {
              operating: k.operating_basis_count,
              calendar: k.calendar_basis_count,
            });
  /** "N exclus : <champ> manquant" parts of a figure, only the non-zero ones. */
  const exclusions = (parts: readonly (readonly [string, number])[]): string[] =>
    parts.filter(([, n]) => n > 0).map(([key, n]) => t(`rams.kpi.excluded.${key}`, { count: n }));
  /** "période trop courte pour le MTBF (moins de 90 jours)". */
  const tooShort = (): string => t("rams.kpi.tooShort", { days: MIN_RATE_WINDOW_DAYS });
  /** "12 derniers mois": the RAMS home's period wording. */
  const months = (count: number): string => t("rams.fleet.periodMonths", { count });
  /**
   * Context of a rate card, worded as on the RAMS home: definition with the period (and the hours
   * basis for MTBF and Aₒ), then what was left out; or why it is not given.
   */
  const rateContext = (
    figure: FleetRateFigure,
    k: FleetRateCounts | null,
    period: string,
  ): string => {
    if (k?.window_too_short) return `${period} · ${tooShort()}`;
    const b = basis(k);
    switch (figure) {
      case "mtbf":
        return [
          t("rams.kpi.mtbfHint", { period, basis: b }),
          ...exclusions([
            ["outsideService", k?.mtbf_excluded_outside_service ?? 0],
            ["missingDate", k?.mtbf_excluded_missing_date ?? 0],
          ]),
        ].join(" · ");
      case "mttr":
        return [
          t("rams.kpi.mttrHint", { period }),
          ...exclusions([
            ["failedAt", k?.restore_excluded_missing_failed_at ?? 0],
            ["restoredAt", k?.restore_excluded_missing_restored_at ?? 0],
            ["inconsistent", k?.restore_excluded_inconsistent ?? 0],
          ]),
        ].join(" · ");
      case "availability":
        return t("rams.kpi.availabilityHint", { period, basis: b });
    }
  };
  return { basis, exclusions, tooShort, months, rateContext };
}
