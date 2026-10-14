import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { StatusChip } from "@/components/ui/status-chip";
import { PM_PLAN_LIFECYCLE_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import type { PmPlanRow } from "@/services/pm-service";

import { PmAssetCell } from "./pm-cells";

interface PmPlansViewProps {
  plans: PmPlanRow[];
  loading: boolean;
  onOpenPlan: (planId: number) => void;
}

export function PmPlansView({ plans, loading, onOpenPlan }: PmPlansViewProps) {
  const { t } = useTranslation("pm");
  const { formatDate } = useFormatters();

  const columns: ColumnDef<PmPlanRow>[] = useMemo(
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
          <span className="block max-w-[240px] truncate text-sm">{row.original.title}</span>
        ),
      },
      {
        id: "asset",
        accessorFn: (p) => p.asset_code ?? "",
        header: t("table.asset"),
        cell: ({ row }) =>
          row.original.asset_scope_type === "equipment" ? (
            <PmAssetCell code={row.original.asset_code} name={row.original.asset_name} />
          ) : (
            <span className="text-xs text-text-muted">
              {t("labels.otherAssetScope", {
                scope: t(`scope.${row.original.asset_scope_type}`, {
                  defaultValue: row.original.asset_scope_type,
                }),
              })}
            </span>
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
          <StatusChip
            label={t(`lifecycle.${row.original.lifecycle_status}`, {
              defaultValue: row.original.lifecycle_status,
            })}
            tone={toneOf(PM_PLAN_LIFECYCLE_TONE, row.original.lifecycle_status)}
            dot
          />
        ),
      },
      {
        id: "crew",
        accessorFn: (p) => p.assigned_team_label || p.assigned_team_name || "",
        header: t("table.crew"),
        cell: ({ row }) =>
          row.original.assigned_team_label || row.original.assigned_team_name || t("labels.none"),
      },
      {
        id: "criticality",
        accessorFn: (p) => p.criticality_label ?? "",
        header: t("table.criticality"),
        cell: ({ row }) => row.original.criticality_label || t("labels.none"),
      },
      {
        id: "active",
        accessorFn: (p) => p.is_active,
        header: t("fields.isActive"),
        cell: ({ row }) =>
          row.original.is_active === 1 ? t("labels.active") : t("labels.inactive"),
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
    <div className="min-h-0 flex-1 overflow-auto p-4">
      <DataTable
        columns={columns}
        data={plans}
        isLoading={loading}
        onRowClick={(row) => onOpenPlan(row.id)}
      />
      {!loading && plans.length === 0 ? (
        <p className="py-6 text-center text-sm text-text-muted">{t("empty.noPlansMatch")}</p>
      ) : null}
    </div>
  );
}
