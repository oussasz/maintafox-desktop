import type { ReactNode } from "react";

import { Sparkline } from "@/components/analytics/Sparkline";
import { mfMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

import type { MetricTone } from "./CompactMetricGrid";

export interface HealthStripCell {
  id: string;
  label: string;
  value: string;
  available?: boolean | undefined;
  delta?: string | null | undefined;
  spark?: number[] | undefined;
  alert?: MetricTone | undefined;
  alertLabel?: string | undefined;
  onClick?: (() => void) | undefined;
}

/** Equal-flex composite strip: label + value + vs-prior + optional spark. Values stay text-primary. */
export function HealthStrip(props: {
  title: string;
  hint?: string | undefined;
  headerAction?: ReactNode | undefined;
  cells: HealthStripCell[];
  emptyLabel?: string | undefined;
  className?: string | undefined;
  "aria-label"?: string | undefined;
}) {
  const { title, hint, headerAction, cells, emptyLabel, className } = props;

  return (
    <section className={cn(mfMetric.panel, className)} aria-label={props["aria-label"] ?? title}>
      <div className={mfMetric.header}>
        <p className={mfMetric.title}>{title}</p>
        {hint ? <p className={mfMetric.hint}>{hint}</p> : null}
        {headerAction ? <div className="shrink-0">{headerAction}</div> : null}
      </div>
      {cells.length === 0 ? (
        <p className="px-3 py-2 text-xs text-text-muted">{emptyLabel ?? "—"}</p>
      ) : (
        <div className={mfMetric.strip}>
          {cells.map((cell) => {
            const inner = (
              <>
                <div className="min-w-0">
                  <p className={mfMetric.label}>
                    {cell.label}
                    {cell.alert === "danger" ? (
                      <span className={mfMetric.chipDanger} title={cell.alertLabel}>
                        !
                      </span>
                    ) : null}
                    {cell.alert === "warn" ? (
                      <span className={mfMetric.chipWarn} title={cell.alertLabel}>
                        !
                      </span>
                    ) : null}
                  </p>
                  <p className={cell.available === false ? mfMetric.valueMuted : mfMetric.value}>
                    {cell.available === false ? "—" : cell.value}
                  </p>
                  {cell.delta ? <p className={mfMetric.delta}>{cell.delta}</p> : null}
                </div>
                {cell.spark && cell.spark.length >= 2 ? (
                  <Sparkline values={cell.spark} className="shrink-0" />
                ) : null}
              </>
            );

            if (cell.onClick) {
              return (
                <button
                  key={cell.id}
                  type="button"
                  onClick={cell.onClick}
                  className={cn(mfMetric.stripCell, mfMetric.cellInteractive)}
                >
                  {inner}
                </button>
              );
            }

            return (
              <div key={cell.id} className={mfMetric.stripCell}>
                {inner}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
