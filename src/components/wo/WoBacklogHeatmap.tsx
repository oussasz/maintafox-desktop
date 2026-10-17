/**
 * Backlog heatmap (PRD §6.5): weeks × days grid of planned work against crew capacity, the
 * work that is not on the calendar, and blocked work by delay reason category.
 */
import { AlertTriangle, CalendarRange } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { getWoBacklogHeatmap } from "@/services/wo-service";
import type { WoBacklogHeatmap as Heatmap, WoBacklogDay } from "@shared/ipc-types";

const WEEK_OPTIONS = [2, 4, 8, 12] as const;

/** Load class of a day: share of capacity, or of the busiest day when no capacity is known. */
export function loadLevel(day: WoBacklogDay, maxHours: number): 0 | 1 | 2 | 3 | 4 {
  if (day.planned_hours <= 0) return 0;
  const ratio =
    day.capacity_hours != null && day.capacity_hours > 0
      ? day.planned_hours / day.capacity_hours
      : day.planned_hours / Math.max(maxHours, 1);
  if (day.overloaded || ratio > 1) return 4;
  if (ratio > 0.75) return 3;
  if (ratio > 0.4) return 2;
  return 1;
}

const LEVEL_CLASS = [
  "bg-surface-1",
  "bg-primary/15",
  "bg-primary/35",
  "bg-primary/60",
  "bg-tone-danger-bg text-tone-danger-fg",
];

export function WoBacklogHeatmap() {
  const { t } = useTranslation("ot");
  const { formatDate, formatNumber } = useFormatters();
  const [weeks, setWeeks] = useState<number>(4);
  const [data, setData] = useState<Heatmap | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setData(await getWoBacklogHeatmap({ weeks }));
      setError(null);
    } catch (err) {
      setError(describeAppError(err).title);
    }
  }, [weeks]);

  useEffect(() => {
    void load();
  }, [load]);

  const h = (v: number) => formatNumber(v, { maximumFractionDigits: 1 });
  const maxHours = Math.max(0, ...(data?.days.map((d) => d.planned_hours) ?? []));
  const rows: WoBacklogDay[][] = [];
  for (let i = 0; i < (data?.days.length ?? 0); i += 7) rows.push(data?.days.slice(i, i + 7) ?? []);

  return (
    <section
      className="rounded-lg border border-surface-border p-4"
      aria-label={t("dashboard.backlogHeatmap.title")}
      data-testid="wo-backlog-heatmap"
    >
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <CalendarRange className="h-4 w-4 text-text-muted" aria-hidden />
        <h3 className="text-sm font-semibold text-text-primary">
          {t("dashboard.backlogHeatmap.title")}
        </h3>
        <div className="ml-auto flex items-center gap-1 text-xs">
          <span className="text-text-muted">{t("dashboard.backlogHeatmap.weeks")}</span>
          {WEEK_OPTIONS.map((w) => (
            <button
              key={w}
              type="button"
              aria-pressed={weeks === w}
              onClick={() => setWeeks(w)}
              className={cn(
                "rounded border px-2 py-1",
                weeks === w ? "border-primary text-primary" : "border-surface-border",
              )}
            >
              {w}
            </button>
          ))}
        </div>
      </div>
      <p className="mb-2 text-xs text-text-muted">
        {t("dashboard.backlogHeatmap.subtitle", { weeks })}
      </p>
      {error ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {error}
        </p>
      ) : null}
      {data && !data.capacity_configured ? (
        <p className="mb-2 text-xs text-tone-warning-fg">
          {t("dashboard.backlogHeatmap.noCapacity")}
        </p>
      ) : null}
      {data ? (
        <>
          <div className="space-y-1" role="grid">
            {rows.map((week) => (
              <div key={week[0]?.date} className="grid grid-cols-7 gap-1" role="row">
                {week.map((d) => {
                  const title = [
                    formatDate(d.date),
                    t("dashboard.backlogHeatmap.hours", { hours: h(d.planned_hours) }),
                    d.capacity_hours != null
                      ? t("dashboard.backlogHeatmap.capacity", { hours: h(d.capacity_hours) })
                      : null,
                    t("dashboard.backlogHeatmap.orders", { count: d.wo_count }),
                    d.unestimated_count > 0
                      ? t("dashboard.backlogHeatmap.unestimated", { count: d.unestimated_count })
                      : null,
                    d.overloaded ? t("dashboard.backlogHeatmap.overloaded") : null,
                  ]
                    .filter(Boolean)
                    .join(" · ");
                  return (
                    <div
                      key={d.date}
                      role="gridcell"
                      title={title}
                      aria-label={title}
                      data-level={loadLevel(d, maxHours)}
                      className={cn(
                        "flex h-10 flex-col items-center justify-center rounded text-[10px] leading-tight",
                        LEVEL_CLASS[loadLevel(d, maxHours)],
                        d.overloaded && "ring-2 ring-tone-danger-fg",
                      )}
                    >
                      <span className="text-text-muted">{formatDate(d.date).slice(0, 5)}</span>
                      {d.planned_hours > 0 ? (
                        <span className="font-semibold">{h(d.planned_hours)} h</span>
                      ) : null}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          <div className="mt-4 grid gap-3 md:grid-cols-3">
            <div className="rounded border border-surface-border p-2 text-xs">
              <p className="font-medium">{t("dashboard.backlogHeatmap.unscheduled")}</p>
              <p>
                {t("dashboard.backlogHeatmap.orders", { count: data.unscheduled.wo_count })} ·{" "}
                {t("dashboard.backlogHeatmap.hours", {
                  hours: h(data.unscheduled.estimated_hours),
                })}
              </p>
            </div>
            <div className="rounded border border-surface-border p-2 text-xs">
              <p className="flex items-center gap-1 font-medium">
                <AlertTriangle className="h-3.5 w-3.5 text-tone-warning-fg" aria-hidden />
                {t("dashboard.backlogHeatmap.overdue")}
              </p>
              <p>
                {t("dashboard.backlogHeatmap.orders", { count: data.overdue.wo_count })} ·{" "}
                {t("dashboard.backlogHeatmap.hours", { hours: h(data.overdue.estimated_hours) })}
              </p>
            </div>
            <div className="rounded border border-surface-border p-2 text-xs">
              <p className="mb-1 font-medium">{t("dashboard.backlogHeatmap.blocked")}</p>
              {data.blocked.length === 0 ? (
                <p className="text-text-muted">{t("dashboard.backlogHeatmap.noBlocked")}</p>
              ) : (
                <ul className="space-y-0.5">
                  {data.blocked.map((b) => (
                    <li key={b.category} className="flex justify-between gap-2">
                      <span>
                        {t(`dashboard.backlogHeatmap.category.${b.category}`, {
                          defaultValue: b.category,
                        })}
                      </span>
                      <span className="text-text-muted">
                        {t("dashboard.backlogHeatmap.orders", { count: b.wo_count })} ·{" "}
                        {t("dashboard.backlogHeatmap.waiting", { hours: h(b.waiting_hours) })}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </>
      ) : null}
    </section>
  );
}
