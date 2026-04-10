/**
 * WoArchivePanel.tsx
 *
 * Collapsible section showing closed + cancelled WOs.
 * Read-only — click row opens WoDetailDialog.
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { listWos } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

// ── Props ───────────────────────────────────────────────────────────────────

interface WoArchivePanelProps {
  onRowClick?: (wo: WorkOrder) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoArchivePanel({ onRowClick }: WoArchivePanelProps) {
  const { t } = useTranslation("ot");
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<WorkOrder[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listWos({
        status_codes: ["closed", "cancelled"],
        limit: 50,
        offset: 0,
      });
      setItems(page.items);
      setTotal(page.total);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open && items.length === 0) {
      void load();
    }
  }, [open, items.length, load]);

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
              className={`text-[10px] border-0 ${isCancelled ? "bg-red-100 text-red-700" : "bg-neutral-100 text-neutral-500"}`}
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
            {row.original.closed_at ? formatDate(row.original.closed_at) : "—"}
          </span>
        ),
      },
    ],
    [t],
  );

  return (
    <div className="border-t border-surface-border">
      <button
        type="button"
        className="flex items-center gap-2 w-full px-6 py-2.5 hover:bg-surface-1 transition-colors text-left"
        onClick={() => setOpen(!open)}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 text-text-muted" />
        ) : (
          <ChevronRight className="h-4 w-4 text-text-muted" />
        )}
        <span className="text-sm font-medium text-text-primary">{t("archive.title")}</span>
        <Badge variant="secondary" className="text-xs">
          {total}
        </Badge>
      </button>

      {open && (
        <div className="px-6 pb-4">
          <DataTable
            columns={columns}
            data={items}
            searchable
            pageSize={10}
            isLoading={loading}
            skeletonRows={5}
            onRowClick={(row) => onRowClick?.(row)}
          />
        </div>
      )}
    </div>
  );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString("fr-FR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}
