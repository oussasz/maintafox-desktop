/**
 * Collapsible inactive personnel (same UX pattern as DiArchivePanel / WoArchivePanel).
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { listPersonnel } from "@/services/personnel-service";
import { usePersonnelStore } from "@/stores/personnel-store";
import type { Personnel } from "@shared/ipc-types";

const INACTIVE_TABLE_BADGE = "bg-slate-100 text-slate-700";

export function PersonnelArchivePanel() {
  const { t } = useTranslation("personnel");
  const openPersonnel = usePersonnelStore((s) => s.openPersonnel);
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<Personnel[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listPersonnel({
        availability_status: ["inactive"],
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
  }, [open, load]);

  const columns: ColumnDef<Personnel>[] = useMemo(
    () => [
      {
        accessorKey: "employee_code",
        header: t("list.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.employee_code}</span>,
      },
      {
        accessorKey: "full_name",
        header: t("list.columns.fullName"),
        cell: ({ row }) => (
          <span className="max-w-[220px] truncate block text-xs">{row.original.full_name}</span>
        ),
      },
      {
        accessorKey: "availability_status",
        header: t("list.columns.status"),
        cell: () => (
          <Badge variant="outline" className={`text-[10px] border-0 ${INACTIVE_TABLE_BADGE}`}>
            {t("status.inactive")}
          </Badge>
        ),
      },
      {
        accessorKey: "position_name",
        header: t("list.columns.position"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.position_name ?? "—"}</span>
        ),
      },
    ],
    [t],
  );

  return (
    <div className="border-t border-surface-border">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-6 py-2.5 text-left transition-colors hover:bg-surface-1"
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
            skeletonRows={4}
            onRowClick={(row) => void openPersonnel(row.id)}
          />
        </div>
      )}
    </div>
  );
}
