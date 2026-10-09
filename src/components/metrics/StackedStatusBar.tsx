import { mfMetric } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

import type { MetricTone } from "./CompactMetricGrid";

export interface StatusBarSegment {
  id: string;
  label: string;
  count: number;
  tone: MetricTone | "ok";
  onClick?: (() => void) | undefined;
}

function fillClass(tone: StatusBarSegment["tone"]): string {
  if (tone === "danger") {
    return "bg-status-danger";
  }
  if (tone === "warn") {
    return "bg-status-warning";
  }
  if (tone === "ok") {
    return "bg-status-success";
  }
  return "bg-surface-3";
}

function segClass(tone: StatusBarSegment["tone"]): string {
  return cn("h-full", fillClass(tone));
}

/** One stacked bar + counts. Color lives on the bar, not on numbers. */
export function StackedStatusBar(props: {
  title: string;
  segments: StatusBarSegment[];
  emptyLabel?: string | undefined;
  className?: string | undefined;
  "aria-label"?: string | undefined;
}) {
  const total = props.segments.reduce((sum, s) => sum + Math.max(0, s.count), 0);

  return (
    <section
      className={cn(mfMetric.panel, props.className)}
      aria-label={props["aria-label"] ?? props.title}
    >
      <div className={mfMetric.header}>
        <p className={mfMetric.title}>{props.title}</p>
        <p className={mfMetric.hint}>{total.toLocaleString()}</p>
      </div>
      <div className="space-y-2 px-3 py-2">
        {total === 0 ? (
          <p className="text-xs text-text-muted">{props.emptyLabel ?? "—"}</p>
        ) : (
          <div className={mfMetric.barTrack} role="img" aria-label={props.title}>
            {props.segments.map((s) => {
              const pct = (Math.max(0, s.count) / total) * 100;
              if (pct <= 0) {
                return null;
              }
              return (
                <div
                  key={s.id}
                  className={segClass(s.tone)}
                  style={{ width: `${pct}%` }}
                  title={`${s.label}: ${s.count}`}
                />
              );
            })}
          </div>
        )}
        <div className="flex flex-wrap gap-x-4 gap-y-1">
          {props.segments.map((s) => {
            const inner = (
              <>
                <span
                  className={cn("inline-block h-1.5 w-1.5 rounded-sm", fillClass(s.tone))}
                  aria-hidden
                />
                <span className="text-text-secondary">{s.label}</span>
                <span className="font-mono tabular-nums text-text-primary">
                  {s.count.toLocaleString()}
                </span>
              </>
            );
            if (s.onClick) {
              return (
                <button
                  key={s.id}
                  type="button"
                  onClick={s.onClick}
                  className="inline-flex items-center gap-1.5 text-[11px] hover:underline"
                >
                  {inner}
                </button>
              );
            }
            return (
              <span key={s.id} className="inline-flex items-center gap-1.5 text-[11px]">
                {inner}
              </span>
            );
          })}
        </div>
      </div>
    </section>
  );
}
