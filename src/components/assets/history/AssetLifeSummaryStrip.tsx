import { useMemo } from "react";
import { useTranslation } from "react-i18next";

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
 * Life summary of an asset. Availability is the operational availability of the latest
 * reliability analysis of the asset (RAMS), with the date it covers; without one it reads "—"
 * and says why. No figure is estimated here, and no count reads 0 before the summary is loaded.
 */
export function AssetLifeSummaryStrip({ summary }: { summary: AssetHistorySummary | null }) {
  const { t } = useTranslation("equipment");
  const { formatDate, formatNumber, formatPercent } = useFormatters();

  const rows = useMemo(() => {
    const count = (n: number | undefined) => (summary && n != null ? formatNumber(n) : "—");
    return [
      {
        label: t("history.summary.created"),
        value: summary?.created_at ? formatDate(summary.created_at) : "—",
        hint: null,
      },
      {
        label: t("history.summary.age"),
        value: humanizeAge(summary?.age_days ?? null, t),
        hint: null,
      },
      { label: t("history.summary.wo"), value: count(summary?.wo_count), hint: null },
      { label: t("history.summary.pm"), value: count(summary?.pm_count), hint: null },
      { label: t("history.summary.failures"), value: count(summary?.failure_count), hint: null },
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
  }, [summary, t, formatDate, formatNumber, formatPercent]);

  return (
    <div className="rounded-md border border-surface-border bg-surface-1 p-3">
      <h3 className="mb-2 text-sm font-semibold">{t("history.summary.title")}</h3>
      <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {rows.map((row) => (
          <div key={row.label} className="rounded border border-surface-border/70 px-2 py-1.5">
            <dt className="text-xs text-text-muted">{row.label}</dt>
            <dd className="text-sm font-medium">{row.value}</dd>
            {row.hint ? <dd className="text-xs text-text-muted">{row.hint}</dd> : null}
          </div>
        ))}
      </dl>
    </div>
  );
}
