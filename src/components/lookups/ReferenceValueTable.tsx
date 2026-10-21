/**
 * Shared Reference Manager value-table chrome.
 * Domain behavior comes from adapters via props/slots — not from forked panels.
 */

import { AlertTriangle } from "lucide-react";
import type { ReactNode } from "react";

import {
  REF_TABLE_BODY_CELL_CLASS,
  REF_TABLE_HEADER_CELL_CLASS,
  REF_TABLE_ROOT_CLASS,
  REF_TABLE_ROW_CLASS,
  REF_TABLE_SCROLL_CLASS,
} from "@/components/lookups/reference-table-ui";
import type { ReferenceTableConfirmState } from "@/components/lookups/reference-value-table-types";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

export interface ReferenceValueTableProps {
  /** Full-pane loading (no rows yet). */
  loading?: boolean;
  /**
   * The rows could not be loaded: shown in place of the table with a retry.
   * Action failures are reported by the global toast, not here.
   */
  loadError?: unknown;
  onRetry?: () => void;
  /** Inline message about the row being created or edited (translated text). */
  error?: string | null;
  /** Left side of header: title + badges. */
  title: ReactNode;
  /** Right side of header: discard / add / custom. */
  toolbar?: ReactNode;
  /** Optional banner(s) below header (read-only, create-draft, lifecycle hint). */
  banner?: ReactNode;
  /** Slot above table (e.g. PublishReadinessPanel). */
  aboveHeader?: ReactNode;
  emptyLabel: string;
  /** When true, show empty state instead of children table. */
  showEmpty: boolean;
  /** Table element (thead/tbody) or custom body. */
  children: ReactNode;
  /** Below scroll area: aliases, pagination, etc. */
  footer?: ReactNode;
  confirm?: ReferenceTableConfirmState | null;
  className?: string;
}

export function ReferenceValueTable({
  loading = false,
  loadError = null,
  onRetry,
  error = null,
  title,
  toolbar,
  banner,
  aboveHeader,
  emptyLabel,
  showEmpty,
  children,
  footer,
  confirm = null,
  className,
}: ReferenceValueTableProps) {
  if (loading) {
    return <LoadingState className={cn("h-full", className)} />;
  }

  return (
    <div className={cn(REF_TABLE_ROOT_CLASS, className)}>
      {aboveHeader}

      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-surface-border px-4 py-3">
        <div className="flex min-w-0 flex-wrap items-center gap-2">{title}</div>
        {toolbar ? <div className="flex shrink-0 items-center gap-2">{toolbar}</div> : null}
      </div>

      {banner}

      {error ? (
        <div
          role="alert"
          className={cn(
            "flex items-center gap-2 border-b px-4 py-2 text-sm",
            mfTone.danger.panel,
            mfTone.danger.text,
          )}
        >
          <AlertTriangle className="h-4 w-4 shrink-0" aria-hidden />
          {error}
        </div>
      ) : null}

      <div className={REF_TABLE_SCROLL_CLASS}>
        {loadError ? (
          <div className="p-4">
            {onRetry ? (
              <ErrorState error={loadError} onRetry={onRetry} />
            ) : (
              <ErrorState error={loadError} />
            )}
          </div>
        ) : showEmpty ? (
          <div className="flex h-full items-center justify-center p-6">
            <p className="text-center text-sm text-text-muted">{emptyLabel}</p>
          </div>
        ) : (
          children
        )}
      </div>

      {footer}

      {confirm ? (
        <ConfirmDialog
          open={confirm.open}
          onOpenChange={confirm.onOpenChange}
          title={confirm.title}
          description={confirm.description}
          confirmLabel={confirm.confirmLabel}
          destructive={confirm.destructive !== false}
          onConfirm={confirm.onConfirm}
        >
          {confirm.hint ? <p className="text-xs text-text-muted">{confirm.hint}</p> : null}
        </ConfirmDialog>
      ) : null}
    </div>
  );
}

/** Shared sticky table chrome for adapters. */
export function ReferenceValueTableGrid({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return <table className={cn("w-full text-sm", className)}>{children}</table>;
}

export function ReferenceValueTableHead({ children }: { children: ReactNode }) {
  return (
    <thead className="sticky top-0 z-10 border-b border-surface-border bg-surface-0">
      {children}
    </thead>
  );
}

export function ReferenceValueTableHeadCell({
  children,
  align = "left",
  sortable = false,
  sortDirection = null,
  onSort,
  className,
}: {
  children: ReactNode;
  align?: "left" | "right" | "center";
  sortable?: boolean;
  /** Current sort of this column, announced to assistive technology. */
  sortDirection?: "ascending" | "descending" | null;
  onSort?: () => void;
  className?: string;
}) {
  const alignClass =
    align === "right" ? "text-right" : align === "center" ? "text-center" : "text-left";
  return (
    <th
      scope="col"
      aria-sort={sortable ? (sortDirection ?? "none") : undefined}
      className={cn(REF_TABLE_HEADER_CELL_CLASS, alignClass, className)}
    >
      {sortable ? (
        <button
          type="button"
          onClick={onSort}
          className="inline-flex min-h-9 items-center gap-1 rounded font-medium text-text-muted hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {children}
          {sortDirection === "ascending" ? (
            <span aria-hidden>↑</span>
          ) : sortDirection === "descending" ? (
            <span aria-hidden>↓</span>
          ) : null}
        </button>
      ) : (
        children
      )}
    </th>
  );
}

export function ReferenceValueTableBody({ children }: { children: ReactNode }) {
  return <tbody>{children}</tbody>;
}

export function ReferenceValueTableRow({
  children,
  className,
  highlighted = false,
}: {
  children: ReactNode;
  className?: string | undefined;
  highlighted?: boolean;
}) {
  return (
    <tr className={cn(REF_TABLE_ROW_CLASS, highlighted && "bg-primary/5", className)}>
      {children}
    </tr>
  );
}

export function ReferenceValueTableCell({
  children,
  align = "left",
  className,
}: {
  children: ReactNode;
  align?: "left" | "right" | "center";
  className?: string;
}) {
  const alignClass =
    align === "right"
      ? "text-right align-middle"
      : align === "center"
        ? "text-center align-middle"
        : "align-middle";
  return <td className={cn(REF_TABLE_BODY_CELL_CLASS, alignClass, className)}>{children}</td>;
}
