/**
 * DiDashboardView.tsx
 *
 * KPI cards + distribution tables + trend + top equipment + overdue list.
 * Uses Tailwind-based visuals (no Recharts — the app uses D3).
 * Phase 2 – Sub-phase 04 – File 04 – Sprint S4.
 */

import { AlertTriangle, Clock, RefreshCw, Shield, TrendingUp } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useDiStatsStore } from "@/stores/di-stats-store";

// ── Period helper ─────────────────────────────────────────────────────────────

type TrendPeriod = "3M" | "6M" | "12M" | "24M" | "all";

function subtractMonths(months: number): string {
  const d = new Date();
  d.setMonth(d.getMonth() - months);
  return d.toISOString().slice(0, 10);
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiDashboardView() {
  const { t } = useTranslation("di");
  const stats = useDiStatsStore((s) => s.stats);
  const loading = useDiStatsStore((s) => s.loading);
  const loadStats = useDiStatsStore((s) => s.loadStats);
  const setFilter = useDiStatsStore((s) => s.setFilter);

  const [period, setPeriod] = useState<TrendPeriod>("12M");

  useEffect(() => {
    void loadStats();
  }, [loadStats]);

  const changePeriod = useCallback(
    (p: TrendPeriod) => {
      setPeriod(p);
      const dateFrom = p === "all" ? null : subtractMonths(Number.parseInt(p));
      setFilter({ date_from: dateFrom, date_to: null });
      void loadStats();
    },
    [setFilter, loadStats],
  );

  if (loading && !stats) {
    return (
      <div className="flex items-center justify-center py-16 text-text-muted">
        <RefreshCw className="h-5 w-5 animate-spin mr-2" />
        {t("sla.loading")}
      </div>
    );
  }

  if (!stats) return null;

  const slaPercent =
    stats.sla_total > 0 ? Math.round((stats.sla_met_count / stats.sla_total) * 100) : 0;
  const slaOk = slaPercent >= 80;

  return (
    <div className="p-4 space-y-6 overflow-y-auto">
      {/* ── Row 1: KPI Cards ──────────────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiCard
          label={t("dashboard.total")}
          value={stats.total}
          color="bg-gray-50 text-gray-700"
        />
        <KpiCard
          label={t("dashboard.pending")}
          value={stats.pending}
          color="bg-amber-50 text-amber-700"
          icon={<Clock className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.inProgress")}
          value={stats.in_progress}
          color="bg-blue-50 text-blue-700"
          icon={<TrendingUp className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.slaCompliance")}
          value={`${slaPercent}%`}
          color={slaOk ? "bg-green-50 text-green-700" : "bg-red-50 text-red-700"}
        />
        <KpiCard
          label={t("dashboard.closedThisMonth")}
          value={stats.closed_this_month}
          color="bg-teal-50 text-teal-700"
        />
        <KpiCard
          label={t("dashboard.overdue")}
          value={stats.overdue}
          color="bg-red-50 text-red-700"
          icon={<AlertTriangle className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.safetyIssues")}
          value={stats.safety_issues}
          color="bg-rose-50 text-rose-700"
          icon={<Shield className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.avgAge")}
          value={`${Math.round(stats.avg_age_days)}d`}
          color="bg-gray-50 text-gray-500"
        />
      </div>

      {/* ── Row 2: Distributions ──────────────────────────────────── */}
      <div className="grid md:grid-cols-2 gap-4">
        {/* Status Distribution */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.statusDistribution")}
          </h3>
          <div className="space-y-2">
            {stats.status_distribution.map((d) => {
              const pct = stats.total > 0 ? (d.count / stats.total) * 100 : 0;
              return (
                <div key={d.status} className="flex items-center gap-2 text-xs">
                  <span className="w-28 truncate text-text-muted">{d.status}</span>
                  <div className="flex-1 h-2 rounded bg-surface-1">
                    <div className="h-2 rounded bg-brand-primary" style={{ width: `${pct}%` }} />
                  </div>
                  <span className="w-8 text-right font-mono">{d.count}</span>
                </div>
              );
            })}
          </div>
        </div>

        {/* Priority Distribution */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.priorityDistribution")}
          </h3>
          <div className="space-y-2">
            {stats.priority_distribution.map((d) => {
              const pct = stats.total > 0 ? (d.count / stats.total) * 100 : 0;
              return (
                <div key={d.priority} className="flex items-center gap-2 text-xs">
                  <span className="w-20 truncate text-text-muted">
                    {t(`priority.${d.priority}`)}
                  </span>
                  <div className="flex-1 h-2 rounded bg-surface-1">
                    <div className="h-2 rounded bg-amber-400" style={{ width: `${pct}%` }} />
                  </div>
                  <span className="w-8 text-right font-mono">{d.count}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* ── Row 3: Trend ──────────────────────────────────────────── */}
      <div className="rounded-lg border border-surface-border p-4">
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-sm font-semibold text-text-primary">{t("dashboard.trend")}</h3>
          <div className="flex gap-1">
            {(["3M", "6M", "12M", "24M", "all"] as TrendPeriod[]).map((p) => (
              <Button
                key={p}
                variant={period === p ? "default" : "ghost"}
                size="sm"
                className="h-6 px-2 text-xs"
                onClick={() => changePeriod(p)}
              >
                {p === "all" ? t("dashboard.allTime") : p}
              </Button>
            ))}
          </div>
        </div>
        {stats.monthly_trend.length === 0 ? (
          <p className="text-xs text-text-muted py-4 text-center">{t("empty.list")}</p>
        ) : (
          <div className="space-y-1.5">
            {stats.monthly_trend.map((pt) => {
              const maxVal = Math.max(
                ...stats.monthly_trend.map((p) => Math.max(p.created, p.closed)),
                1,
              );
              return (
                <div key={pt.period} className="flex items-center gap-2 text-xs">
                  <span className="w-20 text-text-muted font-mono">{pt.period}</span>
                  <div className="flex-1 flex gap-1">
                    <div
                      className="h-3 rounded bg-blue-400"
                      style={{ width: `${(pt.created / maxVal) * 100}%` }}
                      title={`${t("dashboard.created")}: ${pt.created}`}
                    />
                    <div
                      className="h-3 rounded bg-green-400"
                      style={{ width: `${(pt.closed / maxVal) * 100}%` }}
                      title={`${t("dashboard.closed")}: ${pt.closed}`}
                    />
                  </div>
                  <span className="w-14 text-right font-mono">
                    {pt.created}/{pt.closed}
                  </span>
                </div>
              );
            })}
            <div className="flex gap-4 text-[10px] text-text-muted mt-2">
              <span className="flex items-center gap-1">
                <span className="w-3 h-2 rounded bg-blue-400 inline-block" />{" "}
                {t("dashboard.created")}
              </span>
              <span className="flex items-center gap-1">
                <span className="w-3 h-2 rounded bg-green-400 inline-block" />{" "}
                {t("dashboard.closed")}
              </span>
            </div>
          </div>
        )}
      </div>

      {/* ── Row 4: Tables ─────────────────────────────────────────── */}
      <div className="grid md:grid-cols-2 gap-4">
        {/* Top Equipment */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.topEquipment")}
          </h3>
          {stats.top_equipment.length === 0 ? (
            <p className="text-xs text-text-muted">{t("empty.list")}</p>
          ) : (
            <div className="space-y-2">
              {stats.top_equipment.map((eq) => (
                <div key={eq.asset_id} className="flex items-center gap-2 text-xs">
                  <span className="flex-1 truncate">{eq.asset_label}</span>
                  <div className="w-24 h-2 rounded bg-surface-1">
                    <div
                      className="h-2 rounded bg-brand-primary"
                      style={{ width: `${eq.percentage}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono">{eq.count}</span>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Overdue DIs */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.overdueDis")}
          </h3>
          {stats.overdue_dis.length === 0 ? (
            <p className="text-xs text-text-muted">{t("empty.list")}</p>
          ) : (
            <table className="w-full text-xs">
              <thead>
                <tr className="text-left text-text-muted border-b border-surface-border">
                  <th className="pb-1.5 font-medium">{t("list.columns.number")}</th>
                  <th className="pb-1.5 font-medium">{t("list.columns.subject")}</th>
                  <th className="pb-1.5 font-medium text-right">{t("dashboard.daysOverdue")}</th>
                </tr>
              </thead>
              <tbody>
                {stats.overdue_dis.map((d) => (
                  <tr key={d.id} className="border-b border-surface-border last:border-0">
                    <td className="py-1.5 font-mono">{d.code}</td>
                    <td className="py-1.5 max-w-[150px] truncate">{d.title}</td>
                    <td className="py-1.5 text-right">
                      <Badge className="bg-red-100 text-red-700 border-0 text-[10px]">
                        {d.days_overdue}d
                      </Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  );
}

// ── KPI Card ──────────────────────────────────────────────────────────────────

function KpiCard({
  label,
  value,
  color,
  icon,
}: {
  label: string;
  value: number | string;
  color: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className={`rounded-lg p-3 ${color}`}>
      <div className="flex items-center gap-1.5 mb-1">
        {icon}
        <span className="text-[10px] font-medium uppercase tracking-wide opacity-70">{label}</span>
      </div>
      <p className="text-2xl font-bold">{value}</p>
    </div>
  );
}
