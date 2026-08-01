import type { ReactNode } from "react";

/** Semantic tone for badges and row emphasis. */
export type TimelineTone = "default" | "success" | "warning" | "danger" | "info" | "muted";

/** Process / lifecycle step marker (procurement, repairable, etc.). */
export type TimelineStepState = "pending" | "complete" | "current" | "skipped" | "cancelled";

export type TimelineDensity = "compact" | "comfortable";

export interface TimelineBadge {
  label: string;
  tone?: TimelineTone | undefined;
}

export interface TimelineLink {
  /** Business code/label only — never raw FK ids. */
  label: string;
  onClick?: (() => void) | undefined;
  disabled?: boolean | undefined;
}

/**
 * Normalized presentation entry. Adapters map domain DTOs into this shape;
 * the Timeline never fetches or interprets domain enums.
 */
export interface TimelineEntry {
  id: string;
  /** ISO timestamp; omit for pending process steps. */
  timestamp?: string | null | undefined;
  title: ReactNode;
  subtitle?: ReactNode | undefined;
  description?: ReactNode | undefined;
  /** Already-formatted actor display name. */
  actor?: ReactNode | undefined;
  icon?: ReactNode | undefined;
  badges?: TimelineBadge[] | undefined;
  tone?: TimelineTone | undefined;
  stepState?: TimelineStepState | undefined;
  link?: TimelineLink | undefined;
  /** Expandable body (diff JSON, SLA grid, correlation chain, …). */
  details?: ReactNode | undefined;
  detailsExpanded?: boolean | undefined;
  onDetailsToggle?: (() => void) | undefined;
  detailsLabel?: string | undefined;
  /** Arbitrary CTA slot (close downtime, open entity, …). */
  action?: ReactNode | undefined;
  /** Day / custom group key when grouping is enabled. */
  groupKey?: string | undefined;
  groupLabel?: string | undefined;
}

export interface TimelineProps<T = TimelineEntry> {
  items: T[];
  /** Map domain item → presentation entry. Defaults to identity when T is TimelineEntry. */
  getEntry?: ((item: T, index: number) => TimelineEntry) | undefined;
  density?: TimelineDensity | undefined;
  groupBy?: "none" | "day" | "custom" | undefined;
  /** Used when groupBy === "day". */
  todayLabel?: string | undefined;
  locale?: string | undefined;
  loading?: boolean | undefined;
  error?: ReactNode | undefined;
  empty?: ReactNode | undefined;
  "aria-label"?: string | undefined;
  className?: string | undefined;
  /** Forwarded to each item root (e.g. org audit testids). */
  itemTestId?: string | undefined;
}
