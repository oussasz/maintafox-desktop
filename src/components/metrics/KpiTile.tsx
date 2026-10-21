/**
 * KpiTile — one headline figure with its label, unit and optional drill-down.
 *
 * Grey-scale by default (ISA-101 spirit): colour only signals an exception, through `tone`.
 * When `onOpen` is given the whole tile is a button that opens the filtered list behind the
 * figure, so every number on a dashboard leads somewhere.
 */
import type { LucideIcon } from "lucide-react";
import { ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";

import { type Tone, mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

export interface KpiTileProps {
  label: string;
  /** Pre-formatted value (use `useFormatters`); "—" when unknown. */
  value: string;
  unit?: string;
  /** One line explaining the figure or its period, e.g. "Last 30 days". */
  hint?: string;
  icon?: LucideIcon;
  /** Exception colour; omit for a normal value. */
  tone?: Tone;
  onOpen?: () => void;
  className?: string;
}

export function KpiTile({
  label,
  value,
  unit,
  hint,
  icon: Icon,
  tone,
  onOpen,
  className,
}: KpiTileProps) {
  const { t } = useTranslation("common");
  const body = (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium uppercase tracking-wide text-text-muted">{label}</span>
        {Icon ? (
          <Icon
            className={cn("h-4 w-4 shrink-0", tone ? mfTone[tone].text : "text-text-muted")}
            aria-hidden
          />
        ) : null}
      </div>
      <div className="mt-1.5 flex items-baseline gap-1.5">
        <span
          className={cn(
            "font-mono text-2xl font-semibold tabular-nums leading-none",
            tone ? mfTone[tone].text : "text-text-primary",
          )}
        >
          {value}
        </span>
        {unit ? <span className="text-sm text-text-muted">{unit}</span> : null}
      </div>
      {hint || onOpen ? (
        <div className="mt-1.5 flex items-center justify-between gap-2 text-xs text-text-muted">
          <span className="truncate">{hint}</span>
          {onOpen ? <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden /> : null}
        </div>
      ) : null}
    </>
  );

  const frame = cn(
    "rounded-lg border bg-surface-1 p-3 text-start shadow-sm",
    tone && tone !== "neutral" ? mfTone[tone].panel : "border-surface-border",
    className,
  );

  if (onOpen) {
    return (
      <button
        type="button"
        onClick={onOpen}
        title={t("ds.kpi.open")}
        aria-label={`${label}: ${value}${unit ? ` ${unit}` : ""}. ${t("ds.kpi.open")}`}
        className={cn(
          frame,
          "min-h-[44px] w-full transition-colors hover:border-primary/40 hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        )}
      >
        {body}
      </button>
    );
  }
  return <div className={frame}>{body}</div>;
}
