/**
 * DiArchivePanel.tsx
 *
 * Collapsible SSOT for terminal (closed) DIs — including not-yet-archived.
 * Read-only — no edit actions.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { dispositionLabelKey } from "@/components/di/disposition-meta";
import { diStatusToI18nKey, DI_STATUS_STYLE } from "@/components/di/status-meta";
import { Badge } from "@/components/ui/badge";
import { listDis } from "@/services/di-service";
import type { InterventionRequest } from "@shared/ipc-types";

interface DiArchivePanelProps {
  /** Bump after main list reload so an open archive stays fresh. */
  refreshKey?: number;
  onRowClick?: (di: InterventionRequest) => void;
}

export function DiArchivePanel({ refreshKey = 0, onRowClick }: DiArchivePanelProps) {
  const { t } = useTranslation("di");
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<InterventionRequest[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listDis({
        status: ["closed"],
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
    if (!open) return;
    void load();
  }, [open, refreshKey, load]);

  const columns: ColumnDef<InterventionRequest>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("list.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.code}</span>,
      },
      {
        accessorKey: "title",
        header: t("list.columns.subject"),
        cell: ({ row }) => (
          <span className="max-w-[200px] truncate block text-xs">{row.original.title}</span>
        ),
      },
      {
        accessorKey: "status",
        header: t("list.columns.status"),
        cell: ({ row }) => {
          const s = row.original.status;
          const dispositionKey = dispositionLabelKey(row.original.disposition_code);
          return (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ${DI_STATUS_STYLE[s] ?? "bg-neutral-100 text-neutral-500"}`}
            >
              {dispositionKey
                ? t(dispositionKey as "disposition.other")
                : t(`status.${diStatusToI18nKey(s)}` as const)}
            </Badge>
          );
        },
      },
      {
        id: "archived",
        header: t("archive.archivedColumn"),
        cell: ({ row }) =>
          row.original.archived_at != null ? (
            <Badge variant="secondary" className="text-[10px]">
              {t("archive.archivedBadge")}
            </Badge>
          ) : (
            <span className="text-xs text-text-muted">—</span>
          ),
      },
      {
        accessorKey: "submitted_at",
        header: t("list.columns.reportedAt"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{formatDate(row.original.submitted_at)}</span>
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
        aria-expanded={open}
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
            skeletonRows={4}
            onRowClick={(row) => onRowClick?.(row)}
          />
        </div>
      )}
    </div>
  );
}

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
