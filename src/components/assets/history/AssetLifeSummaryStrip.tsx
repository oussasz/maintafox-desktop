import { useMemo } from "react";
import { useTranslation } from "react-i18next";

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

export function AssetLifeSummaryStrip({ summary }: { summary: AssetHistorySummary | null }) {
  const { t } = useTranslation("equipment");

  const rows = useMemo(
    () => [
      {
        label: t("history.summary.created"),
        value: summary?.created_at ? new Date(summary.created_at).toLocaleDateString() : "—",
      },
      {
        label: t("history.summary.age"),
        value: humanizeAge(summary?.age_days ?? null, t),
      },
      {
        label: t("history.summary.wo"),
        value: String(summary?.wo_count ?? 0),
      },
      {
        label: t("history.summary.pm"),
        value: String(summary?.pm_count ?? 0),
      },
      {
        label: t("history.summary.failures"),
        value: String(summary?.failure_count ?? 0),
      },
      {
        label: t("history.summary.availability"),
        value:
          summary?.availability_percent != null
            ? `${summary.availability_percent.toFixed(1)}%`
            : "—",
      },
    ],
    [summary, t],
  );

  return (
    <div className="rounded-md border border-surface-border bg-surface-1 p-3">
      <h3 className="text-sm font-semibold mb-2">{t("history.summary.title")}</h3>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {rows.map((row) => (
          <div key={row.label} className="rounded border border-surface-border/70 px-2 py-1.5">
            <p className="text-[10px] uppercase tracking-wide text-text-muted">{row.label}</p>
            <p className="text-sm font-medium">{row.value}</p>
          </div>
        ))}
      </div>
    </div>
  );
}

