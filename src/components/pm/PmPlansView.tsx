import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { formatDate } from "@/utils/format-date";
import type { PmPlan } from "@shared/ipc-types";

interface PmPlansViewProps {
  plans: PmPlan[];
  loading: boolean;
  onOpenPlan: (planId: number) => void;
}

export function PmPlansView({ plans, loading, onOpenPlan }: PmPlansViewProps) {
  const { t, i18n } = useTranslation("pm");

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
          <span className="max-w-[240px] truncate block text-sm">{row.original.title}</span>
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
        cell: ({ row }) => (
          <Badge variant="secondary">
            {t(`lifecycle.${row.original.lifecycle_status}`, {
              defaultValue: row.original.lifecycle_status,
            })}
          </Badge>
        ),
      },
      {
        id: "crew",
        header: t("table.crew"),
        cell: ({ row }) =>
          row.original.assigned_team_label || row.original.assigned_team_name || t("labels.none"),
      },
      {
        id: "criticality",
        header: t("table.criticality"),
        cell: ({ row }) => row.original.criticality_label || t("labels.none"),
      },
      {
        id: "active",
        header: t("fields.isActive"),
        cell: ({ row }) =>
          row.original.is_active === 1 ? t("labels.active") : t("labels.inactive"),
      },
      {
        accessorKey: "updated_at",
        header: t("table.updated"),
        cell: ({ row }) => formatDate(row.original.updated_at, i18n.language),
      },
    ],
    [t, i18n.language],
  );

  return (
    <div className="min-h-0 flex-1 overflow-auto p-4">
      <DataTable
        columns={columns}
        data={plans}
        isLoading={loading}
        onRowClick={(row) => onOpenPlan(row.id)}
      />
      {!loading && plans.length === 0 ? (
        <p className="py-6 text-center text-sm text-muted-foreground">{t("empty.noPlansMatch")}</p>
      ) : null}
    </div>
  );
}
