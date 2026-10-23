/**
 * WoDashboardView.tsx
 *
 * KPI cards + distribution charts + trend + entity breakdown.
 * Uses Tailwind-based visuals (no Recharts — same approach as DiDashboardView).
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import { AlertTriangle, CheckCircle2, ClipboardList, PlayCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { KpiTile } from "@/components/metrics/KpiTile";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { WoBacklogHeatmap } from "@/components/wo/WoBacklogHeatmap";
import { useFormatters } from "@/hooks/use-formatters";
import { getWoStats } from "@/services/wo-service";
import { isWoLifecycleState } from "@/utils/wo-lifecycle";
import { statusToI18nKey } from "@/utils/wo-status";
import type { WoStatsPayload } from "@shared/ipc-types";

// ── Component ─────────────────────────────────────────────────────────────────

export function WoDashboardView({ reloadSignal = 0 }: { reloadSignal?: number }) {
  const { t } = useTranslation("ot");
  const { labelOf: urgencyLabel } = useUrgencyScale();
  const [stats, setStats] = useState<WoStatsPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const { formatNumber } = useFormatters();

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getWoStats();
      setStats(data);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `reloadSignal` is the page Refresh button.
  useEffect(() => {
    void load();
  }, [load, reloadSignal]);

  if (loading && !stats) {
    return <LoadingState label={t("dashboard.loading")} />;
  }

  if (error != null && !stats) {
    return (
      <div className="p-4">
        <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
      </div>
    );
  }

  if (!stats) return null;

  return (
    <div className="p-4 space-y-6 overflow-y-auto">
      {/* ── Row 1: KPI tiles ──────────────────────────────────────── */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <KpiTile
          label={t("dashboard.total")}
          value={formatNumber(stats.total)}
          icon={ClipboardList}
        />
        <KpiTile
          label={t("dashboard.inProgress")}
          value={formatNumber(stats.in_progress)}
          icon={PlayCircle}
        />
        <KpiTile
          label={t("dashboard.completed")}
          value={formatNumber(stats.completed)}
          icon={CheckCircle2}
        />
        <KpiTile
          label={t("dashboard.overdue")}
          value={formatNumber(stats.overdue)}
          icon={AlertTriangle}
          {...(stats.overdue > 0 ? { tone: "danger" as const } : {})}
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
                  <span className="w-28 truncate text-text-muted">
                    {t(`status.${statusToI18nKey(d.status)}`)}
                  </span>
                  <div className="flex-1 h-2 rounded bg-surface-1">
                    <div className="h-2 rounded bg-brand-primary" style={{ width: `${pct}%` }} />
                  </div>
                  <span className="w-8 text-right font-mono">{d.count}</span>
                </div>
              );
            })}
          </div>
        </div>

        {/* PRD lifecycle distribution (derived states) */}
        <div className="rounded-lg border border-surface-border p-4">
          <h3 className="text-sm font-semibold text-text-primary mb-3">
            {t("dashboard.lifecycleDistribution")}
          </h3>
          <div className="space-y-2">
            {stats.by_lifecycle.map((d) => {
              const pct = stats.total > 0 ? (d.count / stats.total) * 100 : 0;
              return (
                <div key={d.status} className="flex items-center gap-2 text-xs">
                  <span className="w-40 truncate text-text-muted">
                    {isWoLifecycleState(d.status) ? t(`lifecycle.${d.status}`) : d.status}
                  </span>
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
                  <span className="w-20 truncate text-text-muted">
                    {d.urgency ? urgencyLabel(d.urgency) : t("dashboard.noUrgency")}
                  </span>
                  <div className="flex-1 h-2 rounded bg-surface-1">
                    <div
                      className="h-2 rounded bg-tone-attention-fg"
                      style={{ width: `${pct}%` }}
                    />
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
                      className="h-3 rounded bg-tone-success-fg"
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

      <WoBacklogHeatmap />

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
                      className="h-3 rounded bg-brand-primary"
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
