/**
 * Picks the work order a stock movement belongs to (exit to an OT, return from an OT).
 *
 * Searches the work-order list through the existing `listWos` service (code or title) among
 * work orders that can still consume parts, and returns the chosen work order's code, which the
 * movement keeps as its source reference.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { useCanReadList } from "@/lib/list-access";
import { cn } from "@/lib/utils";
import { listWos } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

/** Work-order statuses in which parts can still be issued to or returned from the job. */
export const PARTS_OPEN_WO_STATUSES = ["planning", "ready", "in_progress", "on_hold"] as const;

/** How many matches the picker lists at once (the search narrows further). */
const PICKER_PAGE_SIZE = 20;

export interface WorkOrderReferencePickerProps {
  id: string;
  /** Selected work order code, or null. */
  value: string | null;
  onChange: (code: string | null) => void;
  /** Receives the whole work order (id, code, title) when one is picked; null when cleared. */
  onSelect?: (workOrder: WorkOrder | null) => void;
  disabled?: boolean;
}

export function WorkOrderReferencePicker({
  id,
  value,
  onChange,
  onSelect,
  disabled = false,
}: WorkOrderReferencePickerProps) {
  const { t } = useTranslation("inventory");
  const [search, setSearch] = useState("");
  const [items, setItems] = useState<WorkOrder[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);
  // The list is `list_wo` (ot.view): without it the picker is not searched and says why.
  const canRead = useCanReadList("workOrders");

  useEffect(() => {
    if (!canRead) return;
    let cancelled = false;
    const handle = window.setTimeout(() => {
      setLoading(true);
      setError(null);
      listWos({
        search: search.trim() || null,
        status_codes: [...PARTS_OPEN_WO_STATUSES],
        limit: PICKER_PAGE_SIZE,
        offset: 0,
      })
        .then((page) => {
          if (!cancelled) setItems(page.items);
        })
        .catch((err: unknown) => {
          if (!cancelled) setError(err);
        })
        .finally(() => {
          if (!cancelled) setLoading(false);
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [search, version, canRead]);

  if (!canRead) return <ListAccessNote list="workOrders" />;

  return (
    <div className="space-y-2">
      <Input
        id={id}
        type="search"
        value={search}
        disabled={disabled}
        onChange={(e) => setSearch(e.target.value)}
        placeholder={t("adjustStock.workOrder.searchPlaceholder")}
        aria-describedby={`${id}-hint`}
      />
      <p id={`${id}-hint`} className="text-xs text-text-muted">
        {value
          ? t("adjustStock.workOrder.selected", { code: value })
          : t("adjustStock.workOrder.hint")}
      </p>
      {error ? (
        <ErrorState
          error={error}
          title={t("adjustStock.workOrder.loadError")}
          onRetry={() => setVersion((v) => v + 1)}
        />
      ) : (
        <div
          role="listbox"
          aria-label={t("adjustStock.workOrder.listLabel")}
          className="max-h-40 overflow-y-auto rounded-md border border-surface-border"
        >
          {loading && items.length === 0 ? (
            <p className="px-3 py-2 text-xs text-text-muted">
              {t("adjustStock.workOrder.loading")}
            </p>
          ) : items.length === 0 ? (
            <p className="px-3 py-2 text-xs text-text-muted">{t("adjustStock.workOrder.empty")}</p>
          ) : (
            items.map((wo) => {
              const selected = wo.code === value;
              return (
                <button
                  key={wo.id}
                  type="button"
                  role="option"
                  aria-selected={selected}
                  disabled={disabled}
                  onClick={() => {
                    onChange(selected ? null : wo.code);
                    onSelect?.(selected ? null : wo);
                  }}
                  className={cn(
                    "flex min-h-10 w-full items-center gap-2 px-3 py-1.5 text-left text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    selected ? "bg-primary/10 font-medium text-text-primary" : "hover:bg-surface-2",
                  )}
                >
                  <span className="font-mono text-xs">{wo.code}</span>
                  <span className="truncate text-text-secondary">{wo.title}</span>
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
