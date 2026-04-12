/**
 * WoDashboardView.tsx
 *
 * KPI cards + distribution charts + trend + entity breakdown.
 * Uses Tailwind-based visuals (no Recharts — same approach as DiDashboardView).
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import { AlertTriangle, CheckCircle2, ClipboardList, PlayCircle, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getWoStats } from "@/services/wo-service";
import type { WoStatsPayload } from "@shared/ipc-types";

// ── Component ─────────────────────────────────────────────────────────────────

export function WoDashboardView() {
  const { t } = useTranslation("ot");
  const [stats, setStats] = useState<WoStatsPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getWoStats();
      setStats(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("dashboard.loadError"));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  if (loading && !stats) {
    return (
      <div className="flex items-center justify-center py-16 text-text-muted">
        <RefreshCw className="h-5 w-5 animate-spin mr-2" />
        {t("dashboard.loading")}
      </div>
    );
  }

  if (error && !stats) {
    return (
      <div className="flex flex-col items-center justify-center py-16 gap-3">
        <div className="flex items-center gap-2 text-destructive">
          <AlertTriangle className="h-5 w-5" />
          <span className="text-sm font-medium">{error}</span>
        </div>
        <button
          type="button"
          onClick={() => void load()}
          className="inline-flex items-center gap-1.5 rounded-md border border-input bg-background px-3 py-1.5 text-xs font-medium hover:bg-accent"
        >
          <RefreshCw className="h-3.5 w-3.5" />
          {t("dashboard.retry")}
        </button>
      </div>
    );
  }

  if (!stats) return null;

  return (
    <div className="p-4 space-y-6 overflow-y-auto">
      {/* ── Row 1: KPI Cards ──────────────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiCard
          label={t("dashboard.total")}
          value={stats.total}
          color="bg-blue-50 text-blue-700"
          icon={<ClipboardList className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.inProgress")}
          value={stats.in_progress}
          color="bg-amber-50 text-amber-700"
          icon={<PlayCircle className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.completed")}
          value={stats.completed}
          color="bg-emerald-50 text-emerald-700"
          icon={<CheckCircle2 className="h-4 w-4" />}
        />
        <KpiCard
          label={t("dashboard.overdue")}
          value={stats.overdue}
          color="bg-red-50 text-red-700"
          icon={<AlertTriangle className="h-4 w-4" />}
        />
      </div>

      {/* ── Row 2: Distributions ──────────────────────────────────── */}
      <div className="grid md:grid-cols-2 gap-4">
        {/* Status Distribution (Donut-like) */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.statusDistribution")}
          </h3>
          <div className="space-y-2">
            {stats.by_status.map((d) => {
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

        {/* Urgency Distribution (Horizontal bar) */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.urgencyDistribution")}
          </h3>
          <div className="space-y-2">
            {stats.by_urgency.map((d) => {
              const pct = stats.total > 0 ? (d.count / stats.total) * 100 : 0;
              return (
                <div key={d.urgency} className="flex items-center gap-2 text-xs">
                  <span className="w-20 truncate text-text-muted">{d.urgency}</span>
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

      {/* ── Row 3: Completion Trend ───────────────────────────────── */}
      <div className="rounded-lg border border-surface-border p-4">
        <h3 className="text-sm font-semibold text-text-primary mb-3">
          {t("dashboard.completionTrend")}
        </h3>
        {stats.daily_completed.length === 0 ? (
          <p className="text-xs text-text-muted py-4 text-center">{t("empty.list")}</p>
        ) : (
          <div className="space-y-1.5">
            {stats.daily_completed.map((pt) => {
              const maxVal = Math.max(...stats.daily_completed.map((p) => p.count), 1);
              return (
                <div key={pt.date} className="flex items-center gap-2 text-xs">
                  <span className="w-20 text-text-muted font-mono">{pt.date}</span>
                  <div className="flex-1 h-3 rounded bg-surface-1">
                    <div
                      className="h-3 rounded bg-green-400"
                      style={{ width: `${(pt.count / maxVal) * 100}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono">{pt.count}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* ── Row 4: Backlog by Entity ──────────────────────────────── */}
      <div className="rounded-lg border border-surface-border p-4">
        <h3 className="text-sm font-semibold text-text-primary mb-3">
          {t("dashboard.backlogByEntity")}
        </h3>
        {stats.by_entity.length === 0 ? (
          <p className="text-xs text-text-muted py-4 text-center">{t("empty.list")}</p>
        ) : (
          <div className="space-y-2">
            {stats.by_entity.map((d) => {
              const maxVal = Math.max(...stats.by_entity.map((e) => e.count), 1);
              return (
                <div key={d.entity} className="flex items-center gap-2 text-xs">
                  <span className="w-32 truncate text-text-muted">{d.entity}</span>
                  <div className="flex-1 h-3 rounded bg-surface-1">
                    <div
                      className="h-3 rounded bg-blue-400"
                      style={{ width: `${(d.count / maxVal) * 100}%` }}
                    />
                  </div>
                  <span className="w-8 text-right font-mono">{d.count}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Sub-component ───────────────────────────────────────────────────────────

function KpiCard({
  label,
  value,
  color,
  icon,
}: {
  label: string;
  value: number;
  color: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className={`rounded-lg p-3 ${color}`}>
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium opacity-80">{label}</span>
        {icon}
      </div>
      <p className="text-2xl font-bold mt-1">{value}</p>
    </div>
  );
}
