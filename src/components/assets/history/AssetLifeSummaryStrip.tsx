import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { useFormatters } from "@/hooks/use-formatters";
import type { AssetHistorySummary } from "@shared/ipc-types";

type Translator = ReturnType<typeof useTranslation<"equipment">>["t"];

function humanizeAge(days: number | null, t: Translator) {
  if (days == null || days < 0) return "—";
  const years = Math.floor(days / 365);
  const months = Math.floor((days % 365) / 30);
  if (years > 0) return t("history.summary.ageYm", { years, months });
  if (months > 0) return t("history.summary.ageM", { months });
  return t("history.summary.ageD", { days });
}

/**
 * Life summary of an asset. Age runs from the commissioning date, never from the record creation;
 * without a commissioning date it reads "—" and says what to fill in (L2-06). Availability is the operational availability of the latest
 * reliability analysis of the asset (RAMS), with the date it covers; without one it reads "—"
 * and says why. No figure is estimated here, and no count reads 0 before the summary is loaded.
 */
export function AssetLifeSummaryStrip({
  summary,
  failures,
}: {
  summary: AssetHistorySummary | null;
  /** Failure count of the RAMS calculation (one source); null = the summary's own count. */
  failures?: { count: number | null; months: number } | null;
}) {
  const { t } = useTranslation("equipment");
  const { formatDate, formatNumber, formatPercent } = useFormatters();

  const rows = useMemo(() => {
    const count = (n: number | undefined) => (summary && n != null ? formatNumber(n) : "—");
    return [
      {
        label: t("history.summary.commissioned"),
        value: summary?.commissioned_at ? formatDate(summary.commissioned_at) : "—",
        hint: summary && !summary.commissioned_at ? t("history.summary.commissionedMissing") : null,
      },
      {
        label: t("history.summary.age"),
        value: humanizeAge(summary?.age_days ?? null, t),
        hint: null,
      },
      { label: t("history.summary.wo"), value: count(summary?.wo_count), hint: null },
      { label: t("history.summary.pm"), value: count(summary?.pm_count), hint: null },
      failures
        ? {
            label: t("history.summary.failures"),
            value: failures.count == null ? "—" : formatNumber(failures.count),
            hint: t("history.summary.failuresRams", { months: failures.months }),
          }
        : {
            label: t("history.summary.failures"),
            value: count(summary?.failure_count),
            hint: null,
          },
      {
        label: t("history.summary.availability"),
        value:
          summary?.availability_percent != null ? formatPercent(summary.availability_percent) : "—",
        hint:
          summary?.availability_percent != null && summary.availability_period_end
            ? t("history.summary.availabilityAsOf", {
                date: formatDate(summary.availability_period_end),
              })
            : summary
              ? t("history.summary.availabilityNone")
              : null,
      },
    ];
  }, [summary, failures, t, formatDate, formatNumber, formatPercent]);

  return (
    <section aria-label={t("history.summary.title")} className="space-y-2">
      <h3 className="text-sm font-semibold">{t("history.summary.title")}</h3>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {rows.map((row) => (
          <KpiTile
            key={row.label}
            label={row.label}
            value={row.value}
            {...(row.hint ? { hint: row.hint } : {})}
          />
        ))}
      </div>
    </section>
  );
}
