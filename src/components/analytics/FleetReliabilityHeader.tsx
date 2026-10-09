import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { mfChip } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import type { AnalyticsExecutiveDashboard } from "@shared/ipc-types";
import { ANALYTICS_RAMS_BASE_PATH, RELIABILITY_RAM_EVIDENCE_HASH } from "@shared/kpi-definitions";

import { Sparkline } from "./Sparkline";

function seriesValues(): number[] {
  return [];
}

export function FleetReliabilityHeader(props: {
  data: AnalyticsExecutiveDashboard;
  horizonDays: number;
}) {
  const { t } = useTranslation("reports");
  const navigate = useNavigate();
  const { data, horizonDays } = props;

  const status = useMemo(() => {
    const avail = data.avg_availability.available ? data.avg_availability.value : null;
    const dq = data.avg_data_quality.available ? data.avg_data_quality.value : null;
    if (avail == null && dq == null) {
      return { key: "unknown" as const, chip: mfChip.neutral };
    }
    if ((avail != null && avail < 0.85) || (dq != null && dq < 0.5)) {
      return {
        key: "critical" as const,
        chip: cn(mfChip.neutralStrong, "border-status-danger/40 text-text-danger"),
      };
    }
    if ((avail != null && avail < 0.95) || (dq != null && dq < 0.75)) {
      return {
        key: "watch" as const,
        chip: cn(mfChip.neutralStrong, "border-status-warning/40 text-text-warning"),
      };
    }
    return {
      key: "stable" as const,
      chip: cn(mfChip.neutralStrong, "border-status-success/35 text-text-success"),
    };
  }, [data.avg_availability, data.avg_data_quality]);

  const cells = [
    {
      code: "MTBF",
      label: t("hub.dashboards.kpi.mtbf"),
      value: data.avg_mtbf_hours.available ? `${data.avg_mtbf_hours.value.toFixed(1)} h` : "—",
      spark: seriesValues(),
      risingIsBad: false,
      catalog: data.avg_mtbf_hours.catalog_id,
    },
    {
      code: "MTTR",
      label: t("hub.dashboards.kpi.mttr"),
      value: data.avg_mttr_hours.available ? `${data.avg_mttr_hours.value.toFixed(1)} h` : "—",
      spark: seriesValues(),
      risingIsBad: true,
      catalog: data.avg_mttr_hours.catalog_id,
    },
    {
      code: "A(t)",
      label: t("hub.dashboards.kpi.availability"),
      value: data.avg_availability.available
        ? `${(data.avg_availability.value * 100).toFixed(1)}%`
        : "—",
      spark: seriesValues(),
      risingIsBad: false,
      catalog: data.avg_availability.catalog_id,
    },
    {
      code: "DQ",
      label: t("hub.dashboards.kpi.dataQuality"),
      value: data.avg_data_quality.available ? data.avg_data_quality.value.toFixed(2) : "—",
      spark: [] as number[],
      risingIsBad: false,
      catalog: data.avg_data_quality.catalog_id,
    },
  ];

  return (
    <section className="overflow-hidden rounded-md border border-surface-border bg-surface-1 shadow-sm">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-surface-border bg-surface-2/50 px-3 py-2">
        <div className="min-w-0">
          <p className="font-mono text-[10px] font-semibold uppercase tracking-[0.14em] text-text-muted">
            {t("hub.dashboards.fleetHeader.eyebrow")}
          </p>
          <h3 className="text-sm font-semibold text-text-primary">
            {t("hub.dashboards.fleetHeader.title")}
          </h3>
          <p className="text-[11px] text-text-muted">
            {t("hub.dashboards.fleetHeader.subtitle", { days: horizonDays })}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <span className={cn("rounded-full px-2.5 py-0.5 text-[11px] font-medium", status.chip)}>
            {t(`hub.dashboards.fleetHeader.status.${status.key}`)}
          </span>
          <span className="font-mono text-[10px] tabular-nums text-text-muted">
            n={data.reliability_snapshot_count} · Σe={data.reliability_total_events}
          </span>
          <button
            type="button"
            className="text-[11px] font-medium text-primary underline-offset-2 hover:underline"
            onClick={() =>
              navigate(`${ANALYTICS_RAMS_BASE_PATH}/foundation${RELIABILITY_RAM_EVIDENCE_HASH}`)
            }
          >
            {t("hub.dashboards.fleetHeader.openEvidence")}
          </button>
        </div>
      </div>
      <div className="grid grid-cols-2 divide-x divide-y divide-surface-border lg:grid-cols-4 lg:divide-y-0">
        {cells.map((c) => (
          <div key={c.code} className="px-3 py-2.5">
            <div className="flex items-start justify-between gap-2">
              <div>
                <p className="font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-text-muted">
                  {c.code}
                  {c.catalog ? (
                    <span className="ms-1 font-normal normal-case tracking-normal opacity-60">
                      {c.catalog}
                    </span>
                  ) : null}
                </p>
                <p className="text-[11px] text-text-secondary">{c.label}</p>
              </div>
              {c.spark.length > 1 ? <Sparkline values={c.spark} /> : null}
            </div>
            <p className="mt-1.5 font-mono text-lg font-semibold tabular-nums tracking-tight text-text-primary">
              {c.value}
            </p>
          </div>
        ))}
      </div>
    </section>
  );
}
