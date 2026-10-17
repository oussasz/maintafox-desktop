import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import type { PmPlanRow as PmPlan } from "@/services/pm-service";

import { PmAssetCell } from "./pm-cells";

interface PmArchivePanelProps {
  plans: PmPlan[];
  onOpenPlan: (planId: number) => void;
}

export function PmArchivePanel({ plans, onOpenPlan }: PmArchivePanelProps) {
  const { t } = useTranslation("pm");
  const { formatDate } = useFormatters();
  const [open, setOpen] = useState(false);
  const retired = useMemo(() => plans.filter((p) => p.lifecycle_status === "retired"), [plans]);

  const columns: ColumnDef<PmPlan>[] = useMemo(
    () => [
      {
        accessorKey: "code",
        header: t("table.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.code}</span>,
      },
      {
        accessorKey: "title",
        header: t("table.title"),
        cell: ({ row }) => (
          <span className="max-w-[220px] truncate block text-xs">{row.original.title}</span>
        ),
      },
      {
        id: "asset",
        header: t("table.asset"),
        cell: ({ row }) => (
          <PmAssetCell code={row.original.asset_code} name={row.original.asset_name} />
        ),
      },
      {
        accessorKey: "strategy_type",
        header: t("table.strategy"),
        cell: ({ row }) =>
          t(`strategy.${row.original.strategy_type}`, {
            defaultValue: row.original.strategy_type,
          }),
      },
      {
        accessorKey: "lifecycle_status",
        header: t("table.lifecycle"),
        cell: () => <StatusChip label={t("lifecycle.retired")} tone="neutral" />,
      },
      {
        accessorKey: "updated_at",
        header: t("table.updated"),
        cell: ({ row }) => formatDate(row.original.updated_at),
      },
    ],
    [t, formatDate],
  );

  return (
    <div className="border-t border-border bg-muted/20">
      <button
        type="button"
        className="flex min-h-11 w-full items-center gap-2 px-4 py-2 text-sm font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {open ? (
          <ChevronDown className="h-4 w-4" aria-hidden />
        ) : (
          <ChevronRight className="h-4 w-4" aria-hidden />
        )}
        {t("archive.title")}
        <span className="text-text-muted">{t("archive.count", { count: retired.length })}</span>
      </button>
      {open ? (
        <div className="px-4 pb-4">
          <DataTable columns={columns} data={retired} onRowClick={(row) => onOpenPlan(row.id)} />
          {retired.length === 0 ? (
            <p className="py-4 text-center text-sm text-text-muted">{t("empty.noArchive")}</p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
