export interface EntitySummaryKpi {
  id: string;
  label: string;
  value: string;
}

export interface EntitySummaryKpiStripProps {
  title?: string;
  kpis: EntitySummaryKpi[];
}

/** Summary metrics strip (Asset Health analogue) above Details sections. */
export function EntitySummaryKpiStrip({ title, kpis }: EntitySummaryKpiStripProps) {
  if (kpis.length === 0) return null;

  return (
    <div className="flex flex-wrap items-start gap-4 border-t border-surface-border/80 pt-3">
      {title ? (
        <div className="flex w-full flex-col gap-1 sm:w-auto">
          <span className="text-[10px] font-medium uppercase tracking-wide text-text-muted">
            {title}
          </span>
        </div>
      ) : null}
      {kpis.map((kpi) => (
        <div key={kpi.id} className="flex min-w-[4.5rem] flex-col gap-0.5">
          <span className="text-[10px] font-medium uppercase tracking-wide text-text-muted">
            {kpi.label}
          </span>
          <span className="text-sm font-semibold tabular-nums text-text-primary">{kpi.value}</span>
        </div>
      ))}
    </div>
  );
}
