/**
 * Shared pieces of the reliability (RAMS) module screens: section card, callout, field and the
 * period presets of the fleet view.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { mfCard } from "@/design-system/tokens";
import { type Tone, mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

export function RamsCard({
  title,
  subtitle,
  actions,
  children,
  className,
  headingId,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
  headingId?: string;
}) {
  return (
    <section className={cn(mfCard.panel, "min-w-0", className)} aria-labelledby={headingId}>
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 id={headingId} className="text-sm font-semibold text-text-primary">
            {title}
          </h3>
          {subtitle ? <p className="mt-1 text-xs text-text-muted">{subtitle}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
      {children ? <div className="mt-3">{children}</div> : null}
    </section>
  );
}

export function RamsCallout({
  tone,
  title,
  children,
  action,
}: {
  tone: Tone;
  title: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div
      className={cn("rounded-md border px-3 py-2.5 text-sm text-text-primary", mfTone[tone].panel)}
    >
      <p className={cn("font-semibold", mfTone[tone].text)}>{title}</p>
      {children ? <div className="mt-0.5 text-text-secondary">{children}</div> : null}
      {action ? <div className="mt-2 flex flex-wrap gap-2">{action}</div> : null}
    </div>
  );
}

export const RAMS_SELECT_CLASS =
  "h-10 min-w-[10rem] rounded-md border border-surface-border bg-surface-0 px-2.5 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60";

export const RAMS_TABLE_FRAME =
  "overflow-hidden rounded-md border border-surface-border bg-surface-1";

export const RAMS_FILTER_LABEL = "text-[11px] font-medium uppercase tracking-wide text-text-muted";

/** Period presets of the fleet view, in months back from today. */
export const RAMS_PERIOD_MONTHS = [6, 12, 24, 36] as const;
export type RamsPeriodMonths = (typeof RAMS_PERIOD_MONTHS)[number];

export function periodFromMonths(
  months: number,
  now: Date = new Date(),
): { start: string; end: string } {
  const start = new Date(now);
  start.setMonth(start.getMonth() - months);
  return { start: start.toISOString(), end: now.toISOString() };
}

/** `YYYY-MM-DD` of an ISO instant (date inputs). */
export function isoDay(iso: string): string {
  return iso.slice(0, 10);
}

/** Start or end of a day typed in a date input, as an ISO instant (UTC). */
export function dayToIso(day: string, end: boolean): string {
  return `${day}T${end ? "23:59:59" : "00:00:00"}Z`;
}

/** Text alternative for a chart: a visually hidden table. */
export function ChartDataTable({
  caption,
  headers,
  rows,
}: {
  caption: string;
  headers: string[];
  rows: (string | number)[][];
}) {
  return (
    <table className="sr-only">
      <caption>{caption}</caption>
      <thead>
        <tr>
          {headers.map((h) => (
            <th key={h} scope="col">
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {r.map((c, j) => (
              <td key={j}>{c}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Label of a plant rating level in the interface language. */
export function useScaleLabel(): (
  level: { label_fr: string; label_en: string } | undefined,
) => string {
  const { i18n } = useTranslation("reliability");
  const fr = (i18n.resolvedLanguage ?? i18n.language ?? "fr").toLowerCase().startsWith("fr");
  return (level) => (level ? (fr ? level.label_fr : level.label_en) : "");
}
