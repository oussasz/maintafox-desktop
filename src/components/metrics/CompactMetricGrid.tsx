import type { ReactNode } from "react";

import { mfMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export type MetricTone = "neutral" | "warn" | "danger";

export interface CompactMetricCell {
  id: string;
  label: string;
  value: string;
  available?: boolean | undefined;
  tone?: MetricTone | undefined;
  /** Muted vs-prior delta; never colored (ISA-101: color is for current exception only). */
  delta?: string | null | undefined;
  quality?: boolean | undefined;
  qualityLabel?: string | undefined;
  onClick?: (() => void) | undefined;
}

const COLUMN_CLASS = {
  2: "grid-cols-2",
  3: "grid-cols-2 sm:grid-cols-3",
  4: "grid-cols-2 sm:grid-cols-4",
  6: "grid-cols-2 sm:grid-cols-3 lg:grid-cols-6",
  8: "grid-cols-2 sm:grid-cols-4 lg:grid-cols-8",
} as const;

function columnClass(count: number, columns?: 2 | 3 | 4 | 6 | 8 | undefined): string {
  if (columns) {
    return COLUMN_CLASS[columns];
  }
  if (count <= 3) {
    return COLUMN_CLASS[3];
  }
  if (count <= 4) {
    return COLUMN_CLASS[4];
  }
  if (count <= 6) {
    return COLUMN_CLASS[6];
  }
  return COLUMN_CLASS[8];
}

function valueClass(cell: CompactMetricCell): string {
  if (cell.available === false) {
    return mfMetric.valueMuted;
  }
  return mfMetric.value;
}

export function CompactMetricGrid(props: {
  title: string;
  hint?: string | undefined;
  headerAction?: ReactNode | undefined;
  cells: CompactMetricCell[];
  columns?: 2 | 3 | 4 | 6 | 8 | undefined;
  emptyLabel?: string | undefined;
  className?: string | undefined;
  "aria-label"?: string | undefined;
}) {
  const { title, hint, headerAction, cells, columns, emptyLabel, className } = props;

  return (
    <section className={cn(mfMetric.panel, className)} aria-label={props["aria-label"] ?? title}>
      <div className={mfMetric.header}>
        <p className={mfMetric.title}>{title}</p>
        {hint ? <p className={mfMetric.hint}>{hint}</p> : null}
        {headerAction ? <div className="shrink-0">{headerAction}</div> : null}
      </div>
      {cells.length === 0 ? (
        <p className="px-3 py-3 text-xs text-text-muted">{emptyLabel ?? "—"}</p>
      ) : (
        <div className={cn(mfMetric.grid, columnClass(cells.length, columns))}>
          {cells.map((cell) => {
            const inner = (
              <>
                <span className={mfMetric.label}>
                  {cell.label}
                  {cell.tone === "danger" ? (
                    <span className={mfMetric.chipDanger} aria-hidden>
                      !
                    </span>
                  ) : null}
                  {cell.tone === "warn" ? (
                    <span className={mfMetric.chipWarn} aria-hidden>
                      !
                    </span>
                  ) : null}
                </span>
                <span className={valueClass(cell)}>
                  {cell.available === false ? "—" : cell.value}
                </span>
                {cell.delta ? <span className={mfMetric.delta}>{cell.delta}</span> : null}
              </>
            );

            if (cell.onClick) {
              return (
                <button
                  key={cell.id}
                  type="button"
                  onClick={cell.onClick}
                  className={cn(mfMetric.cell, mfMetric.cellInteractive)}
                >
                  {inner}
                </button>
              );
            }

            return (
              <div key={cell.id} className={mfMetric.cell}>
                {inner}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

export function CompactChartPanel(props: {
  title: string;
  hint?: string | undefined;
  headerAction?: ReactNode | undefined;
  children: ReactNode;
  className?: string | undefined;
}) {
  return (
    <section className={cn(mfMetric.panel, props.className)}>
      <div className={mfMetric.header}>
        <p className={mfMetric.title}>{props.title}</p>
        {props.hint ? <p className={mfMetric.hint}>{props.hint}</p> : null}
        {props.headerAction ? <div className="shrink-0">{props.headerAction}</div> : null}
      </div>
      <div className={mfMetric.chartBody}>{props.children}</div>
    </section>
  );
}
