/**
 * WoArchivePanel.tsx
 *
 * Collapsible section showing closed + cancelled WOs.
 * Read-only — click row opens WoDetailDialog.
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { listWos } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { formatDate } from "@/utils/format-date";
import type { WorkOrder } from "@shared/ipc-types";

// ── Props ───────────────────────────────────────────────────────────────────

interface WoArchivePanelProps {
  onRowClick?: (wo: WorkOrder) => void;
}

const ARCHIVE_PAGE = 50;

// ── Component ───────────────────────────────────────────────────────────────

export function WoArchivePanel({ onRowClick }: WoArchivePanelProps) {
  const { t, i18n } = useTranslation("ot");
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<WorkOrder[]>([]);
  const [total, setTotal] = useState(0);
  // The whole archive, whatever the search: what the header badge shows.
  const [archiveTotal, setArchiveTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A failed "load more" keeps the loaded rows and says so above the button.
  const [moreError, setMoreError] = useState(false);
  const [search, setSearch] = useState("");
  const latest = useRef(0);
  // A work order closed or cancelled on this screen changes the list: the archive follows it.
  const archivedKey = useWoStore((s) =>
    s.items
      .filter((w) => w.status_code === "closed" || w.status_code === "cancelled")
      .map((w) => w.id)
      .join(","),
  );

  // The count is read when the screen opens (not only once the panel is unfolded), and the
  // search is done by the server over every archived work order, not over the 50 loaded rows.
  const load = useCallback(
    async (text: string) => {
      const ticket = ++latest.current;
      setLoading(true);
      setError(null);
      try {
        const page = await listWos({
          status_codes: ["closed", "cancelled"],
          search: text.trim() || null,
          limit: ARCHIVE_PAGE,
          offset: 0,
        });
        if (ticket !== latest.current) return;
        setItems(page.items);
        setTotal(page.total);
        setMoreError(false);
      } catch (e) {
        if (ticket !== latest.current) return;
        setError(t("archive.loadError"));
        console.error("WoArchivePanel.load", e);
      } finally {
        if (ticket === latest.current) setLoading(false);
      }
    },
    [t],
  );

  useEffect(() => {
    const timer = setTimeout(() => void load(search), search ? 300 : 0);
    return () => clearTimeout(timer);
  }, [search, load, archivedKey]);

  // The badge is the whole archive, whatever the search: its own unfiltered count.
  useEffect(() => {
    let current = true;
    listWos({ status_codes: ["closed", "cancelled"], search: null, limit: 1, offset: 0 })
      .then((page) => {
        if (current) setArchiveTotal(page.total);
      })
      // The badge keeps its last value; the list below shows its own error if the service is down.
      .catch((e) => console.error("WoArchivePanel.count", e));
    return () => {
      current = false;
    };
  }, [archivedKey]);

  const loadMore = useCallback(async () => {
    const ticket = latest.current;
    setLoadingMore(true);
    setMoreError(false);
    try {
      const page = await listWos({
        status_codes: ["closed", "cancelled"],
        search: search.trim() || null,
        limit: ARCHIVE_PAGE,
        offset: items.length,
      });
      if (ticket !== latest.current) return;
      const known = new Set(items.map((w) => w.id));
      setItems([...items, ...page.items.filter((w) => !known.has(w.id))]);
      setTotal(page.total);
    } catch (e) {
      if (ticket === latest.current) setMoreError(true);
      console.error("WoArchivePanel.loadMore", e);
    } finally {
      setLoadingMore(false);
    }
  }, [items, search]);

  const columns: ColumnDef<WorkOrder>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.code}</span>,
      },
      {
        accessorKey: "title",
        header: t("list.columns.title"),
        cell: ({ row }) => (
          <span className="max-w-[200px] truncate block text-xs">{row.original.title}</span>
        ),
      },
      {
        accessorKey: "status",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.status_code;
          const isCancelled = s === "cancelled";
          return (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ${isCancelled ? "bg-tone-danger-bg text-tone-danger-fg" : "bg-tone-neutral-bg text-tone-neutral-fg"}`}
            >
              {isCancelled ? t("status.cancelled") : t("status.closed")}
            </Badge>
          );
        },
      },
      {
        accessorKey: "type_label",
        header: t("list.columns.type"),
        cell: ({ row }) => <span className="text-xs">{row.original.type_label ?? "—"}</span>,
      },
      {
        accessorKey: "closed_at",
        header: t("list.columns.closedAt"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.closed_at ? formatDate(row.original.closed_at, i18n.language) : "—"}
          </span>
        ),
      },
    ],
    [t, i18n.language],
  );

  return (
    <div className="border-t border-surface-border">
      <button
        type="button"
        className="flex items-center gap-2 w-full px-6 py-2.5 hover:bg-surface-1 transition-colors text-left"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 text-text-muted" />
        ) : (
          <ChevronRight className="h-4 w-4 text-text-muted" />
        )}
        <span className="text-sm font-medium text-text-primary">{t("archive.title")}</span>
        <Badge variant="secondary" className="text-xs">
          {archiveTotal}
        </Badge>
      </button>

      {open && (
        <div className="px-6 pb-4">
          {error ? (
            <div className="flex items-center justify-between rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              <span>{error}</span>
              <button
                className="ml-2 text-xs underline hover:no-underline"
                onClick={() => void load(search)}
              >
                {t("archive.retry")}
              </button>
            </div>
          ) : (
            <div className="space-y-3">
              <Input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("search.placeholder")}
                className="max-w-sm"
                aria-label={t("search.placeholder")}
              />
              <DataTable
                columns={columns}
                data={items}
                pageSize={10}
                isLoading={loading}
                skeletonRows={5}
                onRowClick={(row) => onRowClick?.(row)}
              />
              {moreError ? (
                <p className="text-xs text-destructive">{t("archive.loadError")}</p>
              ) : null}
              {items.length < total ? (
                <div className="flex items-center gap-3 text-xs text-text-muted">
                  <span>{t("archive.shown", { shown: items.length, total })}</span>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={loadingMore || loading}
                    onClick={() => void loadMore()}
                  >
                    {t("archive.loadMore", { count: total - items.length })}
                  </Button>
                </div>
              ) : null}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
