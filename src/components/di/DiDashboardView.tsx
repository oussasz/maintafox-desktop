/**
 * DiDashboardView.tsx
 *
 * Request KPIs from the DI statistics (`get_di_stats`), all computed from the requests
 * themselves — no target or threshold is invented here:
 * - backlog now: open requests, waiting for a decision, SLA at risk, SLA breached (each tile
 *   opens its work queue);
 * - service over the period: SLA compliance, conversion rate into work orders, average
 *   response time (submission → screening) and time to work order;
 * - backlog by status and age, open backlog against SLA, closure reasons (dispositions),
 *   urgency mix, monthly trend, most reported equipment, overdue requests.
 */

import {
  AlertTriangle,
  CheckCircle2,
  Clock,
  Hourglass,
  Inbox,
  RefreshCw,
  Shield,
  Timer,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import type { DiQueueSelection } from "@/components/di/DiQueueBar";
import { dispositionLabelKey } from "@/components/di/disposition-meta";
import { diStatusToI18nKey } from "@/components/di/status-meta";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { useDiStatsStore } from "@/stores/di-stats-store";
import { useDiStore } from "@/stores/di-store";

type TrendPeriod = "3M" | "6M" | "12M" | "24M" | "all";
const PERIODS: TrendPeriod[] = ["3M", "6M", "12M", "24M", "all"];

function subtractMonths(months: number): string {
  const d = new Date();
  d.setMonth(d.getMonth() - months);
  return d.toISOString().slice(0, 10);
}

interface DiDashboardViewProps {
  /** Opens the work queue behind a backlog figure. */
  onOpenQueue?: (queue: DiQueueSelection) => void;
}

export function DiDashboardView({ onOpenQueue }: DiDashboardViewProps) {
  const { t } = useTranslation("di");
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const { formatNumber, formatPercent } = useFormatters();
  const stats = useDiStatsStore((s) => s.stats);
  const loading = useDiStatsStore((s) => s.loading);
  const error = useDiStatsStore((s) => s.error);
  const loadStats = useDiStatsStore((s) => s.loadStats);
  const setFilter = useDiStatsStore((s) => s.setFilter);
  const openDi = useDiStore((s) => s.openDi);

  const [period, setPeriod] = useState<TrendPeriod>("12M");

  useEffect(() => {
    setFilter({ date_from: subtractMonths(12), date_to: null });
    void loadStats();
  }, [loadStats, setFilter]);

  const changePeriod = useCallback(
    (p: TrendPeriod) => {
      setPeriod(p);
      const dateFrom = p === "all" ? null : subtractMonths(Number.parseInt(p, 10));
      setFilter({ date_from: dateFrom, date_to: null });
      void loadStats();
    },
    [setFilter, loadStats],
  );

  if (loading && !stats) return <LoadingState label={t("dashboard.loading")} />;
  if (error && !stats) {
    return (
      <div className="p-4">
        <ErrorState error={error} onRetry={() => void loadStats()} retrying={loading} />
      </div>
    );
  }
  if (!stats) return null;

  const hours = (v: number | null) =>
    v == null ? "—" : formatNumber(v, { maximumFractionDigits: v < 10 ? 1 : 0 });
  const days = (v: number) => formatNumber(v, { maximumFractionDigits: v < 10 ? 1 : 0 });
  const decided = stats.sla_met_count + stats.sla_breached_count;
  const slaCompliance = decided > 0 ? formatPercent((stats.sla_met_count / decided) * 100) : "—";
  const conversionRate =
    stats.closed_in_period > 0
      ? formatPercent((stats.converted_count / stats.closed_in_period) * 100)
      : "—";
  const periodHint =
    period === "all" ? t("dashboard.periodAll") : t("dashboard.periodMonths", { months: period });
  const openProps = (q: DiQueueSelection) => (onOpenQueue ? { onOpen: () => onOpenQueue(q) } : {});
  const openTotal =
    stats.sla_open.on_track +
    stats.sla_open.at_risk +
    stats.sla_open.breached +
    stats.sla_open.paused +
    stats.sla_open.no_target;
  const maxDisposition = Math.max(1, ...stats.disposition_distribution.map((d) => d.count));
  const maxTrend = Math.max(1, ...stats.monthly_trend.map((p) => Math.max(p.created, p.closed)));

  return (
    <div className="p-4 space-y-6 overflow-y-auto">
      {/* ── Period ───────────────────────────────────────────────── */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-text-primary">{t("dashboard.title")}</h2>
        <div className="flex items-center gap-1" role="group" aria-label={t("dashboard.period")}>
          {PERIODS.map((p) => (
            <Button
              key={p}
              variant={period === p ? "default" : "ghost"}
              size="sm"
              className="h-8 px-2.5 text-xs"
              aria-pressed={period === p}
              onClick={() => changePeriod(p)}
            >
              {p === "all" ? t("dashboard.allTime") : p}
            </Button>
          ))}
          {loading ? (
            <RefreshCw className="ml-1 h-3.5 w-3.5 animate-spin text-text-muted" aria-hidden />
          ) : null}
        </div>
      </div>

      {/* ── Backlog now ──────────────────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiTile
          label={t("dashboard.openBacklog")}
          value={formatNumber(openTotal)}
          hint={t("dashboard.now")}
          icon={Inbox}
          {...openProps("all")}
        />
        <KpiTile
          label={t("dashboard.pending")}
          value={formatNumber(stats.pending)}
          hint={periodHint}
          icon={Hourglass}
          {...openProps("to_screen")}
        />
        <KpiTile
          label={t("dashboard.slaAtRisk")}
          value={formatNumber(stats.sla_open.at_risk)}
          hint={t("dashboard.now")}
          icon={Clock}
          {...(stats.sla_open.at_risk > 0 ? { tone: "attention" as const } : {})}
          {...openProps("sla_at_risk")}
        />
        <KpiTile
          label={t("dashboard.overdue")}
          value={formatNumber(stats.sla_open.breached)}
          hint={t("dashboard.overdueHint")}
          icon={AlertTriangle}
          {...(stats.sla_open.breached > 0 ? { tone: "danger" as const } : {})}
          {...openProps("sla_at_risk")}
        />
      </div>

      {/* ── Service over the period ──────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiTile
          label={t("dashboard.slaCompliance")}
          value={slaCompliance}
          hint={t("dashboard.slaComplianceHint", {
            met: stats.sla_met_count,
            breached: stats.sla_breached_count,
          })}
          icon={CheckCircle2}
        />
        <KpiTile
          label={t("dashboard.conversionRate")}
          value={conversionRate}
          hint={t("dashboard.conversionHint", {
            converted: stats.converted_count,
            closed: stats.closed_in_period,
          })}
          icon={Wrench}
        />
        <KpiTile
          label={t("dashboard.avgResponse")}
          value={hours(stats.avg_response_hours)}
          {...(stats.avg_response_hours != null ? { unit: t("dashboard.hoursUnit") } : {})}
          hint={t("dashboard.avgResponseHint")}
          icon={Timer}
        />
        <KpiTile
          label={t("dashboard.avgConversion")}
          value={hours(stats.avg_conversion_hours)}
          {...(stats.avg_conversion_hours != null ? { unit: t("dashboard.hoursUnit") } : {})}
          hint={t("dashboard.avgConversionHint")}
          icon={Timer}
        />
      </div>

      <div className="grid md:grid-cols-2 gap-4">
        {/* Backlog by status and age */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.backlogByStatus")}
          </h3>
          {stats.backlog_by_status.length === 0 ? (
            <p className="text-xs text-text-muted">{t("dashboard.noBacklog")}</p>
          ) : (
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-text-muted border-b border-surface-border">
                  <th className="pb-1.5 font-medium">{t("list.columns.status")}</th>
                  <th className="pb-1.5 font-medium text-right">{t("dashboard.count")}</th>
                  <th className="pb-1.5 font-medium text-right">{t("dashboard.avgAge")}</th>
                  <th className="pb-1.5 font-medium text-right">{t("dashboard.oldest")}</th>
                </tr>
              </thead>
              <tbody>
                {stats.backlog_by_status.map((b) => (
                  <tr key={b.status} className="border-b border-surface-border last:border-0">
                    <td className="py-1.5">
                      {t(`status.${diStatusToI18nKey(b.status)}` as const)}
                    </td>
                    <td className="py-1.5 text-right font-mono tabular-nums">
                      {formatNumber(b.count)}
                    </td>
                    <td className="py-1.5 text-right font-mono tabular-nums">
                      {t("dashboard.daysValue", { days: days(b.avg_age_days) })}
                    </td>
                    <td className="py-1.5 text-right font-mono tabular-nums">
                      {t("dashboard.daysValue", { days: days(b.max_age_days) })}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>

        {/* Open backlog against SLA */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">{t("dashboard.slaOpen")}</h3>
          {openTotal === 0 ? (
            <p className="text-xs text-text-muted">{t("dashboard.noBacklog")}</p>
          ) : (
            <div className="space-y-2">
              {(
                [
                  ["on_track", stats.sla_open.on_track, mfTone.success.bar],
                  ["at_risk", stats.sla_open.at_risk, mfTone.attention.bar],
                  ["breached", stats.sla_open.breached, mfTone.danger.bar],
                  ["paused", stats.sla_open.paused, mfTone.info.bar],
                  ["none", stats.sla_open.no_target, mfTone.neutral.bar],
                ] as const
              ).map(([key, count, bar]) => (
                <div key={key} className="flex items-center gap-2 text-xs">
                  <span className="w-28 truncate text-text-muted">
                    {key === "none" ? t("dashboard.noTarget") : t(`sla.status.${key}`)}
                  </span>
                  <div className="flex-1 h-2 rounded bg-surface-2">
                    <div
                      className={cn("h-2 rounded", bar)}
                      style={{ width: `${(count / openTotal) * 100}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono tabular-nums">
                    {formatNumber(count)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Closure reasons */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-1">
            {t("dashboard.closureReasons")}
          </h3>
          <p className="mb-3 text-xs text-text-muted">{periodHint}</p>
          {stats.disposition_distribution.length === 0 ? (
            <p className="text-xs text-text-muted">{t("dashboard.noClosed")}</p>
          ) : (
            <div className="space-y-2">
              {stats.disposition_distribution.map((d) => {
                const key = dispositionLabelKey(d.disposition_code);
                return (
                  <div key={d.disposition_code} className="flex items-center gap-2 text-xs">
                    <span className="w-40 truncate text-text-muted">
                      {key && d.disposition_code !== "unknown"
                        ? t(key as "disposition.other")
                        : t("dashboard.noDisposition")}
                    </span>
                    <div className="flex-1 h-2 rounded bg-surface-2">
                      <div
                        className="h-2 rounded bg-primary"
                        style={{ width: `${(d.count / maxDisposition) * 100}%` }}
                      />
                    </div>
                    <span className="w-8 text-right font-mono tabular-nums">
                      {formatNumber(d.count)}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        {/* Urgency mix */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-1">
            {t("dashboard.priorityDistribution")}
          </h3>
          <p className="mb-3 text-xs text-text-muted">{periodHint}</p>
          {stats.priority_distribution.length === 0 ? (
            <p className="text-xs text-text-muted">{t("empty.list")}</p>
          ) : (
            <div className="space-y-2">
              {stats.priority_distribution.map((d) => (
                <div key={d.priority} className="flex items-center gap-2 text-xs">
                  <span className="w-24 truncate text-text-muted">{urgencyLabel(d.priority)}</span>
                  <div className="flex-1 h-2 rounded bg-surface-2">
                    <div
                      className="h-2 rounded bg-primary"
                      style={{ width: `${stats.total > 0 ? (d.count / stats.total) * 100 : 0}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono tabular-nums">
                    {formatNumber(d.count)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>

      {/* ── Trend ────────────────────────────────────────────────── */}
      <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
        <h3 className="text-sm font-semibold text-text-primary mb-3">{t("dashboard.trend")}</h3>
        {stats.monthly_trend.length === 0 ? (
          <p className="text-xs text-text-muted py-4 text-center">{t("empty.list")}</p>
        ) : (
          <div className="space-y-1.5">
            {stats.monthly_trend.map((pt) => (
              <div key={pt.period} className="flex items-center gap-2 text-xs">
                <span className="w-20 text-text-muted font-mono">{pt.period}</span>
                <div className="flex-1 flex gap-1">
                  <div
                    className="h-3 rounded bg-primary"
                    style={{ width: `${(pt.created / maxTrend) * 50}%` }}
                    title={`${t("dashboard.created")}: ${pt.created}`}
                  />
                  <div
                    className={cn("h-3 rounded", mfTone.success.bar)}
                    style={{ width: `${(pt.closed / maxTrend) * 50}%` }}
                    title={`${t("dashboard.closed")}: ${pt.closed}`}
                  />
                </div>
                <span className="w-16 text-right font-mono tabular-nums">
                  {pt.created}/{pt.closed}
                </span>
              </div>
            ))}
            <div className="flex gap-4 text-xs text-text-muted mt-2">
              <span className="flex items-center gap-1">
                <span className="w-3 h-2 rounded bg-primary inline-block" aria-hidden />
                {t("dashboard.created")}
              </span>
              <span className="flex items-center gap-1">
                <span
                  className={cn("w-3 h-2 rounded inline-block", mfTone.success.bar)}
                  aria-hidden
                />
                {t("dashboard.closed")}
              </span>
            </div>
          </div>
        )}
      </section>

      <div className="grid md:grid-cols-2 gap-4">
        {/* Most reported equipment */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.topEquipment")}
          </h3>
          {stats.top_equipment.length === 0 ? (
            <p className="text-xs text-text-muted">{t("empty.list")}</p>
          ) : (
            <div className="space-y-2">
              {stats.top_equipment.map((eq) => (
                <div key={eq.equipment_id} className="flex items-center gap-2 text-xs">
                  <span className="flex-1 truncate">{eq.asset_label}</span>
                  <div className="w-24 h-2 rounded bg-surface-2">
                    <div
                      className="h-2 rounded bg-primary"
                      style={{ width: `${eq.percentage}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono tabular-nums">
                    {formatNumber(eq.count)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Overdue requests */}
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.overdueDis")}
          </h3>
          {stats.overdue_dis.length === 0 ? (
            <p className="text-xs text-text-muted">{t("dashboard.noOverdue")}</p>
          ) : (
            <ul className="space-y-1">
              {stats.overdue_dis.map((d) => (
                <li key={d.id}>
                  <button
                    type="button"
                    onClick={() => void openDi(d.id)}
                    className="flex w-full items-center gap-2 rounded px-1 py-1.5 text-left text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <span className="font-mono text-text-muted">{d.code}</span>
                    <span className="min-w-0 flex-1 truncate">{d.title}</span>
                    <span className="text-text-muted">{urgencyLabel(d.priority)}</span>
                    <StatusChip
                      label={
                        d.hours_overdue >= 24
                          ? t("dashboard.overdueDays", { days: days(d.hours_overdue / 24) })
                          : t("dashboard.overdueHours", { hours: hours(d.hours_overdue) })
                      }
                      tone="danger"
                    />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      {stats.safety_issues > 0 ? (
        <p className="flex items-center gap-1.5 text-xs text-text-secondary">
          <Shield className="h-3.5 w-3.5" aria-hidden />
          {t("dashboard.safetyIssuesLine", { count: stats.safety_issues })}
        </p>
      ) : null}
    </div>
  );
}
