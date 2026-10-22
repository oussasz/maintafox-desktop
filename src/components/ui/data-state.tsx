/**
 * Loading and error states for module content (lists, panels, detail tabs).
 *
 * Rules (docs/DESIGN_SYSTEM.md):
 *  - A failed *load* of the main content shows `ErrorState` in place of the content, with a
 *    retry. Never an empty "no results" table.
 *  - A failed *action* (save, transition) is reported by the global toast only; do not repeat
 *    it inline unless the message belongs to a specific field.
 *  - Empty data uses `EmptyState` (components/ui/empty-state.tsx).
 */
import { AlertTriangle, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";

export interface LoadingStateProps {
  label?: string;
  className?: string;
}

export function LoadingState({ label, className }: LoadingStateProps) {
  const { t } = useTranslation("common");
  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        "flex items-center justify-center gap-2 px-6 py-10 text-sm text-text-muted",
        className,
      )}
    >
      <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
      <span>{label ?? t("ds.state.loading")}</span>
    </div>
  );
}

export interface ErrorStateProps {
  error: unknown;
  /** Heading; defaults to a generic "could not be loaded". */
  title?: string;
  onRetry?: () => void;
  retrying?: boolean;
  className?: string;
}

export function ErrorState({
  error,
  title,
  onRetry,
  retrying = false,
  className,
}: ErrorStateProps) {
  const { t } = useTranslation("common");
  const described = describeAppError(error);
  return (
    <div
      role="alert"
      className={cn(
        "flex flex-col items-center justify-center gap-2 rounded-md border border-tone-danger-border bg-tone-danger-bg px-6 py-8 text-center",
        className,
      )}
    >
      <AlertTriangle className="h-5 w-5 text-tone-danger-fg" aria-hidden />
      <p className="text-sm font-semibold text-text-primary">
        {title ?? t("ds.state.loadErrorTitle")}
      </p>
      <p className="max-w-md text-sm text-text-secondary">{described.title}</p>
      {described.description ? (
        <p className="max-w-md text-sm text-text-secondary">{described.description}</p>
      ) : null}
      {described.technical ? (
        <details className="max-w-md text-xs text-text-muted">
          <summary className="cursor-pointer select-none">{t("ds.state.technicalDetails")}</summary>
          <p className="mt-1 whitespace-pre-wrap break-words text-start">{described.technical}</p>
        </details>
      ) : null}
      {onRetry ? (
        <Button
          size="sm"
          variant="outline"
          className="mt-2 h-10"
          onClick={onRetry}
          disabled={retrying}
        >
          {t("ds.state.retry")}
        </Button>
      ) : null}
    </div>
  );
}

export interface TruncationNoticeProps {
  shown: number;
  total: number;
  /** Loads the next page; shows a "load more" button when given. */
  onLoadMore?: () => void;
  loadingMore?: boolean;
  className?: string;
}

/** Honest notice when a view shows only part of the matching records. */
export function TruncationNotice({
  shown,
  total,
  onLoadMore,
  loadingMore = false,
  className,
}: TruncationNoticeProps) {
  const { t } = useTranslation("common");
  if (total <= shown) return null;
  return (
    <div
      role="status"
      className={cn(
        "flex flex-wrap items-center justify-between gap-2 rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-xs text-tone-info-fg",
        className,
      )}
    >
      <span>{t("ds.state.truncated", { shown, total })}</span>
      {onLoadMore ? (
        <Button
          size="sm"
          variant="outline"
          className="h-10"
          onClick={onLoadMore}
          disabled={loadingMore}
        >
          {loadingMore ? t("ds.state.loading") : t("ds.state.loadMore")}
        </Button>
      ) : null}
    </div>
  );
}
