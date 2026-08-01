import { ChevronDown, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfTimeline } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

import { groupItemsByDay } from "./groupByDay";
import type {
  TimelineBadge,
  TimelineDensity,
  TimelineEntry,
  TimelineLink,
  TimelineProps,
  TimelineStepState,
  TimelineTone,
} from "./types";

const BADGE_TONE: Record<TimelineTone, string> = {
  default: "",
  success: "bg-status-success/15 text-status-success border-status-success/30",
  warning: "bg-status-warning/15 text-status-warning border-status-warning/30",
  danger: "bg-status-danger/15 text-status-danger border-status-danger/30",
  info: "bg-primary/15 text-primary border-primary/30",
  muted: "bg-surface-3/70 text-text-secondary",
};

const STEP_MARKER: Record<TimelineStepState, string> = {
  pending: "bg-muted-foreground/30 border-background",
  complete: "bg-primary border-background",
  current: "bg-primary border-background ring-2 ring-primary/40",
  skipped: "bg-muted-foreground/20 border-background",
  cancelled: "bg-status-danger border-background",
};

function formatTimeLabel(iso: string, locale?: string): string {
  try {
    return new Date(iso).toLocaleString(locale, {
      day: "2-digit",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

function EntryBadges({ badges }: { badges?: TimelineBadge[] | undefined }) {
  if (!badges?.length) return null;
  return (
    <>
      {badges.map((b, i) => (
        <Badge
          key={`${b.label}-${i}`}
          variant="outline"
          className={cn(mfTimeline.badge, b.tone ? BADGE_TONE[b.tone] : undefined)}
        >
          {b.label}
        </Badge>
      ))}
    </>
  );
}

function TimelineItemRow({
  entry,
  isLast,
  density,
  locale,
  itemTestId,
}: {
  entry: TimelineEntry;
  isLast: boolean;
  density: TimelineDensity;
  locale?: string | undefined;
  itemTestId?: string | undefined;
}) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const controlled = entry.detailsExpanded !== undefined;
  const detailsOpen = controlled ? Boolean(entry.detailsExpanded) : uncontrolledOpen;

  const toggleDetails = () => {
    if (controlled) {
      entry.onDetailsToggle?.();
    } else {
      setUncontrolledOpen((v) => !v);
    }
  };

  const hasIcon = entry.icon != null;
  const stepState = entry.stepState;
  const rowPad = density === "compact" ? "pb-4" : "pb-6";

  return (
    <li
      className={cn(
        mfTimeline.item,
        rowPad,
        isLast && "pb-0",
        entry.tone === "danger" && "rounded-md bg-status-danger/10 px-2 py-2",
      )}
      data-testid={itemTestId}
    >
      {!isLast && <div className={mfTimeline.connector} aria-hidden />}

      <div
        className={cn(
          mfTimeline.marker,
          "h-8 w-8",
          stepState
            ? STEP_MARKER[stepState]
            : hasIcon
              ? "bg-background border-surface-border"
              : "bg-background border-surface-border",
        )}
        aria-hidden
      >
        {hasIcon ? (
          <span className={mfTimeline.icon}>{entry.icon}</span>
        ) : (
          <span
            className={cn("h-2.5 w-2.5 rounded-full", stepState ? "bg-transparent" : "bg-primary")}
          />
        )}
      </div>

      <div className={cn(mfTimeline.content, "min-w-0 flex-1 pt-0.5")}>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex flex-wrap items-center gap-1.5">
              <span className={mfTimeline.title}>{entry.title}</span>
              <EntryBadges badges={entry.badges} />
            </div>
            {entry.subtitle ? <div className={mfTimeline.subtitle}>{entry.subtitle}</div> : null}
            {entry.description ? (
              <div className={mfTimeline.description}>{entry.description}</div>
            ) : null}
            <div className={mfTimeline.meta}>
              {entry.timestamp ? (
                <time dateTime={entry.timestamp}>{formatTimeLabel(entry.timestamp, locale)}</time>
              ) : null}
              {entry.actor ? (
                <>
                  {entry.timestamp ? <span aria-hidden>·</span> : null}
                  <span>{entry.actor}</span>
                </>
              ) : null}
            </div>
          </div>

          <div className="flex shrink-0 flex-wrap items-center gap-1">
            {entry.link ? (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                disabled={entry.link.disabled}
                onClick={entry.link.onClick}
              >
                {entry.link.label}
              </Button>
            ) : null}
            {entry.action}
          </div>
        </div>

        {entry.details != null ? (
          <div className="mt-2">
            <button
              type="button"
              onClick={toggleDetails}
              aria-expanded={detailsOpen}
              className="flex items-center gap-1 text-xs font-medium text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring rounded-sm"
            >
              {detailsOpen ? (
                <ChevronDown className="h-3 w-3" aria-hidden />
              ) : (
                <ChevronRight className="h-3 w-3" aria-hidden />
              )}
              {entry.detailsLabel ?? "Details"}
            </button>
            {detailsOpen ? <div className="mt-1">{entry.details}</div> : null}
          </div>
        ) : null}
      </div>
    </li>
  );
}

function defaultGetEntry(item: TimelineEntry): TimelineEntry {
  return item;
}

function groupCustom(
  entries: TimelineEntry[],
): { key: string; label: string; items: TimelineEntry[] }[] {
  const map = new Map<string, { key: string; label: string; items: TimelineEntry[] }>();
  for (const e of entries) {
    const key = e.groupKey ?? e.id;
    const label = e.groupLabel ?? key;
    const existing = map.get(key);
    if (existing) existing.items.push(e);
    else map.set(key, { key, label, items: [e] });
  }
  return Array.from(map.values());
}

/**
 * Single application-wide vertical timeline. Presentation-only — adapters own
 * fetch, filter, i18n, icons, and domain mapping.
 */
export function Timeline<T = TimelineEntry>({
  items,
  getEntry,
  density = "comfortable",
  groupBy = "none",
  todayLabel = "Today",
  locale,
  loading,
  error,
  empty,
  "aria-label": ariaLabel = "Timeline",
  className,
  itemTestId,
}: TimelineProps<T>) {
  const mapEntry = (getEntry ?? defaultGetEntry) as (item: T, index: number) => TimelineEntry;

  const entries = useMemo(() => items.map((item, i) => mapEntry(item, i)), [items, mapEntry]);

  const groups = useMemo(() => {
    if (groupBy === "day") {
      return groupItemsByDay(
        entries,
        (e) => e.timestamp ?? e.id,
        locale ?? (typeof navigator !== "undefined" ? navigator.language : "en"),
        todayLabel,
      ).map((g) => ({ key: g.key, label: g.label, items: g.items }));
    }
    if (groupBy === "custom") {
      return groupCustom(entries);
    }
    return [{ key: "all", label: "", items: entries }];
  }, [entries, groupBy, locale, todayLabel]);

  if (loading) {
    return (
      <div className={cn("space-y-4 py-4", className)} aria-busy="true" aria-label={ariaLabel}>
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="flex items-start gap-3">
            <div className="h-8 w-8 animate-pulse rounded-full bg-muted" />
            <div className="flex-1 space-y-2">
              <div className="h-4 w-3/4 animate-pulse rounded bg-muted" />
              <div className="h-3 w-1/2 animate-pulse rounded bg-muted" />
            </div>
          </div>
        ))}
      </div>
    );
  }

  if (error) {
    return (
      <div className={cn("py-2", className)} role="alert">
        {error}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div className={cn("py-6 text-center text-sm text-muted-foreground", className)}>
        {empty ?? "No events"}
      </div>
    );
  }

  return (
    <div className={cn(mfTimeline.root, className)} role="region" aria-label={ariaLabel}>
      {groups.map((group) => (
        <div key={group.key} className={group.label ? "space-y-2" : undefined}>
          {group.label ? <p className={mfTimeline.groupHeader}>{group.label}</p> : null}
          <ol className={mfTimeline.list}>
            {group.items.map((entry, idx) => (
              <TimelineItemRow
                key={entry.id}
                entry={entry}
                isLast={idx === group.items.length - 1}
                density={density}
                {...(locale !== undefined ? { locale } : {})}
                {...(itemTestId ? { itemTestId } : {})}
              />
            ))}
          </ol>
        </div>
      ))}
    </div>
  );
}

export type {
  TimelineEntry,
  TimelineProps,
  TimelineBadge,
  TimelineLink,
  TimelineTone,
  TimelineStepState,
};
