/**
 * Module figures (PRD §6.25): rounds due / done / overdue / missed, compliance and open
 * anomalies. Each tile opens the list that reproduces its figure (`kpiDrillDown`).
 */
import {
  AlertTriangle,
  CalendarCheck,
  CalendarClock,
  CalendarX,
  Gauge,
  ListChecks,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { useFormatters } from "@/hooks/use-formatters";
import type { InspectionKpis } from "@shared/ipc-types";

import type { InspectionKpiKey } from "./inspection-rules";

/** Periods offered for the figures (days). */
export const KPI_PERIOD_CHOICES = [7, 30, 90] as const;

interface InspectionKpiStripProps {
  kpis: InspectionKpis | null;
  error: unknown;
  loading: boolean;
  periodDays: number;
  onPeriodChange: (days: number) => void;
  onOpen: (key: InspectionKpiKey) => void;
  onRetry: () => void;
}

export function InspectionKpiStrip({
  kpis,
  error,
  loading,
  periodDays,
  onPeriodChange,
  onOpen,
  onRetry,
}: InspectionKpiStripProps) {
  const { t } = useTranslation("inspections");
  const { formatNumber } = useFormatters();

  if (error != null && kpis == null) {
    return (
      <ErrorState error={error} title={t("kpi.loadError")} onRetry={onRetry} retrying={loading} />
    );
  }
  const value = (n: number | undefined) => (n == null ? "—" : formatNumber(n));
  const periodHint = t("kpi.periodHint", { count: periodDays });

  return (
    <section aria-label={t("kpi.title")} className="space-y-2" data-testid="inspection-kpis">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text-primary">{t("kpi.title")}</h2>
        <div className="flex items-center gap-2">
          <label htmlFor="inspection-kpi-period" className="text-xs text-text-muted">
            {t("kpi.period")}
          </label>
          <Select value={String(periodDays)} onValueChange={(v) => onPeriodChange(Number(v))}>
            <SelectTrigger id="inspection-kpi-period" className="h-11 w-40">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {KPI_PERIOD_CHOICES.map((d) => (
                <SelectItem key={d} value={String(d)}>
                  {t("kpi.periodOption", { count: d })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <KpiTile
          label={t("kpi.due")}
          value={value(kpis?.due_count)}
          hint={periodHint}
          icon={CalendarClock}
          onOpen={() => onOpen("due")}
        />
        <KpiTile
          label={t("kpi.done")}
          value={value(kpis?.done_count)}
          hint={periodHint}
          icon={CalendarCheck}
          onOpen={() => onOpen("done")}
        />
        <KpiTile
          label={t("kpi.overdue")}
          value={value(kpis?.overdue_count)}
          hint={t("kpi.nowHint")}
          icon={AlertTriangle}
          {...(kpis && kpis.overdue_count > 0 ? { tone: "danger" as const } : {})}
          onOpen={() => onOpen("overdue")}
        />
        <KpiTile
          label={t("kpi.missed")}
          value={value(kpis?.missed_count)}
          hint={periodHint}
          icon={CalendarX}
          {...(kpis && kpis.missed_count > 0 ? { tone: "danger" as const } : {})}
          onOpen={() => onOpen("missed")}
        />
        <KpiTile
          label={t("kpi.compliance")}
          value={
            kpis?.compliance_pct == null
              ? "—"
              : formatNumber(kpis.compliance_pct, { maximumFractionDigits: 0 })
          }
          {...(kpis?.compliance_pct == null ? {} : { unit: "%" })}
          hint={
            kpis && kpis.due_count > 0
              ? t("kpi.complianceHint", { done: kpis.done_on_time_count, due: kpis.due_count })
              : t("kpi.complianceNone")
          }
          icon={Gauge}
          onOpen={() => onOpen("compliance")}
        />
        <KpiTile
          label={t("kpi.openAnomalies")}
          value={value(kpis?.open_anomaly_count)}
          hint={t("kpi.nowHint")}
          icon={ListChecks}
          {...(kpis && kpis.open_anomaly_count > 0 ? { tone: "attention" as const } : {})}
          onOpen={() => onOpen("anomalies")}
        />
      </div>
    </section>
  );
}
