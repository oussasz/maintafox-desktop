import { cn } from "@/lib/utils";

import { Sparkline } from "./Sparkline";

export interface EngineeringMetricTileProps {
  code: string;
  label: string;
  value: string;
  deltaLabel?: string | null;
  sparkline?: number[];
  tone?: "neutral" | "ok" | "warn" | "danger";
  available?: boolean;
  onOpen?: () => void;
  catalogId?: string | null | undefined;
}

const TONE_BORDER: Record<NonNullable<EngineeringMetricTileProps["tone"]>, string> = {
  neutral: "border-surface-border",
  ok: "border-status-success/35",
  warn: "border-status-warning/40",
  danger: "border-status-danger/40",
};

const TONE_BAR: Record<NonNullable<EngineeringMetricTileProps["tone"]>, string> = {
  neutral: "bg-text-muted/40",
  ok: "bg-status-success",
  warn: "bg-status-warning",
  danger: "bg-status-danger",
};

/** Dense industrial metric cell — not a marketing KPI card. */
export function EngineeringMetricTile({
  code,
  label,
  value,
  deltaLabel,
  sparkline,
  tone = "neutral",
  available = true,
  onOpen,
  catalogId,
}: EngineeringMetricTileProps) {
  const body = (
    <>
      <div className={cn("absolute inset-y-0 start-0 w-0.5", TONE_BAR[tone])} aria-hidden />
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="font-mono text-[10px] font-semibold uppercase tracking-[0.12em] text-text-muted">
            {code}
            {catalogId ? (
              <span className="ms-1.5 font-normal normal-case tracking-normal opacity-70">
                {catalogId}
              </span>
            ) : null}
          </p>
          <p className="mt-0.5 truncate text-xs text-text-secondary">{label}</p>
        </div>
        {sparkline && sparkline.length > 1 ? <Sparkline values={sparkline} /> : null}
      </div>
      <div className="mt-2 flex items-end justify-between gap-2">
        <p
          className={cn(
            "font-mono text-xl font-semibold tabular-nums leading-none tracking-tight",
            available ? "text-text-primary" : "text-text-muted",
          )}
        >
          {available ? value : "—"}
        </p>
        {deltaLabel ? (
          <span className="font-mono text-[10px] tabular-nums text-text-muted">{deltaLabel}</span>
        ) : null}
      </div>
    </>
  );

  const className = cn(
    "relative overflow-hidden rounded-md border bg-surface-1/90 px-3 py-2.5 text-start shadow-sm",
    TONE_BORDER[tone],
    onOpen &&
      "transition-colors hover:bg-surface-2/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  );

  if (onOpen) {
    return (
      <button type="button" className={className} onClick={onOpen}>
        {body}
      </button>
    );
  }
  return <div className={className}>{body}</div>;
}
