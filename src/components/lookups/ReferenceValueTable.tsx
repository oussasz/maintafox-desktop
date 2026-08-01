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
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

export interface ReferenceValueTableProps {
  /** Full-pane loading (no rows yet). */
  loading?: boolean;
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
    return (
      <div className={cn("flex h-full items-center justify-center", className)}>
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  return (
    <div className={cn(REF_TABLE_ROOT_CLASS, className)}>
      {aboveHeader}

      <div className="flex items-center justify-between gap-3 border-b border-surface-border px-4 py-3">
        <div className="flex min-w-0 items-center gap-2">{title}</div>
        {toolbar ? <div className="flex shrink-0 items-center gap-2">{toolbar}</div> : null}
      </div>

      {banner}

      {error ? (
        <div className="flex items-center gap-2 bg-red-50 px-4 py-2 text-sm text-status-danger dark:bg-red-950/20">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      <div className={REF_TABLE_SCROLL_CLASS}>
        {showEmpty ? (
          <div className="flex h-full items-center justify-center p-6">
            <p className="text-center text-sm text-text-muted">{emptyLabel}</p>
          </div>
        ) : (
          children
        )}
      </div>

      {footer}

      {confirm ? (
        <Dialog open={confirm.open} onOpenChange={confirm.onOpenChange}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{confirm.title}</DialogTitle>
              <DialogDescription>{confirm.description}</DialogDescription>
            </DialogHeader>
            {confirm.hint ? <p className="text-xs text-text-muted">{confirm.hint}</p> : null}
            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => confirm.onOpenChange(false)}
                disabled={confirm.busy}
              >
                {confirm.cancelLabel}
              </Button>
              <Button
                variant={confirm.destructive === false ? "default" : "destructive"}
                onClick={confirm.onConfirm}
                disabled={confirm.busy}
              >
                {confirm.confirmLabel}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
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
  onSort,
  className,
}: {
  children: ReactNode;
  align?: "left" | "right" | "center";
  sortable?: boolean;
  onSort?: () => void;
  className?: string;
}) {
  const alignClass =
    align === "right" ? "text-right" : align === "center" ? "text-center" : "text-left";
  return (
    <th
      className={cn(
        REF_TABLE_HEADER_CELL_CLASS,
        alignClass,
        sortable && "cursor-pointer select-none",
        className,
      )}
      onClick={sortable ? onSort : undefined}
      onKeyDown={
        sortable
          ? (e) => {
              if (e.key === "Enter") onSort?.();
            }
          : undefined
      }
    >
      {children}
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
