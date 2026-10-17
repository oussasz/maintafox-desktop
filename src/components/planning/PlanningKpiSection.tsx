/**
 * Planning KPIs of a site against its frozen weekly baseline (planning step E): loading, schedule
 * compliance (SMRP 5.4.2, hours), break-ins and displaced jobs, over the last weeks. Weeks frozen
 * without approval are flagged and can be left out; weeks never frozen are counted, not measured.
 */

import { AlertTriangle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { addDays, errorText, parseDay } from "@/components/planning/weekly-schedule-model";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { mfMetric } from "@/design-system/tokens";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { getPlanningKpis } from "@/services/planning-service";
import type { PlanningKpis } from "@shared/ipc-types";

/** Weeks shown: the current one and the seven before it. */
export const KPI_WEEKS = 8;

export function PlanningKpiSection({ siteId, weekStart }: { siteId: number; weekStart: string }) {
  const { t } = useTranslation("planning");
  const { formatDate, formatNumber } = useFormatters();
  const [kpis, setKpis] = useState<PlanningKpis | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [excludeUnapproved, setExcludeUnapproved] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      setKpis(
        await getPlanningKpis({
          site_id: siteId,
          from_day: addDays(weekStart, -7 * (KPI_WEEKS - 1)),
          to_day: weekStart,
          exclude_unapproved: excludeUnapproved,
        }),
      );
    } catch (err) {
      setKpis(null);
      setError(errorText(err));
    }
  }, [siteId, weekStart, excludeUnapproved]);

  useEffect(() => {
    void load();
  }, [load]);

  const percent = (v: number | null) =>
    v == null ? "—" : `${formatNumber(v, { maximumFractionDigits: 1 })} %`;
  const hours = (v: number) =>
    t("kpis.hours", { hours: formatNumber(v, { maximumFractionDigits: 1 }) });

  return (
    <section className={mfMetric.panel} aria-label={t("kpis.title")}>
      <div className={mfMetric.header}>
        <h3 className={mfMetric.title}>{t("kpis.title")}</h3>
        <div className="flex items-center gap-2">
          <Switch
            id="kpi-exclude-unapproved"
            checked={excludeUnapproved}
            onCheckedChange={setExcludeUnapproved}
          />
          <Label htmlFor="kpi-exclude-unapproved" className="text-[11px] text-text-muted">
            {t("kpis.excludeUnapproved")}
          </Label>
        </div>
      </div>
      {error ? (
        <p role="alert" className="p-3 text-sm text-status-danger">
          {error}
        </p>
      ) : null}
      {kpis ? (
        <>
          <div className={cn(mfMetric.grid, "grid-cols-2 md:grid-cols-6")}>
            <Metric
              label={t("kpis.loading")}
              value={
                kpis.totals.weeks === 0 ? t("kpis.notMeasured") : percent(kpis.totals.loading_pct)
              }
              hint={t("kpis.loadingHint", {
                baseline: hours(kpis.totals.baseline_hours),
                available: hours(kpis.totals.available_hours),
                overtime: hours(kpis.totals.overtime_hours),
              })}
            />
            <Metric
              label={t("kpis.compliance")}
              value={
                kpis.totals.weeks_judged === 0
                  ? t("kpis.notMeasured")
                  : percent(kpis.totals.compliance_pct)
              }
              hint={
                kpis.totals.weeks_judged === 0
                  ? t("kpis.noJudgedWeek")
                  : t("kpis.complianceHint", {
                      done: hours(kpis.totals.done_hours),
                      baseline: hours(kpis.totals.judged_baseline_hours),
                      released: kpis.totals.baseline_released_jobs,
                    })
              }
            />
            <Metric
              label={t("kpis.breakIns")}
              value={
                kpis.totals.weeks === 0 ? t("kpis.notMeasured") : percent(kpis.totals.break_in_pct)
              }
              hint={hours(kpis.totals.break_in_hours)}
            />
            <Metric label={t("kpis.displaced")} value={String(kpis.totals.displaced_jobs)} />
            <Metric
              label={t("kpis.awaitingApproval")}
              value={String(kpis.totals.weeks_awaiting_approval)}
              tone={kpis.totals.weeks_awaiting_approval > 0 ? "warning" : undefined}
            />
            <Metric
              label={t("kpis.notFrozen")}
              value={String(kpis.totals.weeks_not_frozen)}
              tone={kpis.totals.weeks_not_frozen > 0 ? "warning" : undefined}
            />
          </div>
          {kpis.weeks.length === 0 ? (
            <p className="p-3 text-sm text-text-muted">{t("kpis.noFrozenWeek")}</p>
          ) : (
            <div className="overflow-x-auto p-3">
              <table className="w-full text-left text-xs">
                <thead>
                  <tr className="text-text-muted">
                    <th className="py-1 font-medium">{t("kpis.week")}</th>
                    <th className="py-1 text-right font-medium">{t("kpis.baseline")}</th>
                    <th className="py-1 text-right font-medium">{t("kpis.loading")}</th>
                    <th className="py-1 text-right font-medium">{t("kpis.compliance")}</th>
                    <th className="py-1 text-right font-medium">{t("kpis.breakIns")}</th>
                    <th className="py-1 text-right font-medium">{t("kpis.displaced")}</th>
                  </tr>
                </thead>
                <tbody>
                  {kpis.weeks.map((w) => (
                    <tr key={w.weekly_schedule_id} className="border-t border-surface-border">
                      <td className="py-1.5">
                        {t("weekly.range", {
                          from: formatDate(parseDay(w.week_start)),
                          to: formatDate(parseDay(w.week_end)),
                        })}
                        {w.awaiting_approval ? (
                          <Badge variant="outline" className={cn("ml-2", mfTone.warning.chip)}>
                            <AlertTriangle className="mr-1 h-3 w-3" aria-hidden />
                            {t("weekly.frozenWithoutApproval")}
                          </Badge>
                        ) : null}
                        {w.reopened ? (
                          <Badge
                            variant="outline"
                            className={cn("ml-2", mfTone.warning.chip)}
                            title={t("kpis.reopenedHint")}
                          >
                            {t("kpis.reopened")}
                          </Badge>
                        ) : null}
                      </td>
                      <td className="py-1.5 text-right tabular-nums">
                        {t("kpis.jobsHours", {
                          jobs: w.baseline_jobs,
                          hours: hours(w.baseline_hours),
                        })}
                      </td>
                      <td className="py-1.5 text-right tabular-nums">{percent(w.loading_pct)}</td>
                      <td className="py-1.5 text-right tabular-nums">
                        {w.done_hours == null ? t("kpis.inProgress") : percent(w.compliance_pct)}
                      </td>
                      <td className="py-1.5 text-right tabular-nums">{percent(w.break_in_pct)}</td>
                      <td className="py-1.5 text-right tabular-nums">{w.displaced_jobs}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      ) : null}
    </section>
  );
}

function Metric({
  label,
  value,
  hint,
  tone,
}: {
  label: string;
  value: string;
  hint?: string;
  tone?: "warning" | undefined;
}) {
  return (
    <div className={mfMetric.cell}>
      <span className={mfMetric.label}>{label}</span>
      <span className={cn(mfMetric.value, tone ? mfTone[tone].text : undefined)}>{value}</span>
      {hint ? <span className={mfMetric.delta}>{hint}</span> : null}
    </div>
  );
}
