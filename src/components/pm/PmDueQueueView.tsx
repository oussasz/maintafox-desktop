import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { Button } from "@/components/ui/button";
import type { PmOccurrenceRow } from "@/services/pm-service";
import { P } from "@shared/rbac/permissions.generated";

import { PmAssetCell, PmDueLabel, PmOccurrenceStatus } from "./pm-cells";
import { allowedOccurrenceTargets } from "./pm-labels";
import { canLogExecution } from "./pm-model";

interface PmDueQueueViewProps {
  occurrences: PmOccurrenceRow[];
  loading: boolean;
  onOpenPlan: (planId: number) => void;
  onTransition: (occurrence: PmOccurrenceRow) => void;
  onExecute: (occurrence: PmOccurrenceRow) => void;
}

export function PmDueQueueView({
  occurrences,
  loading,
  onOpenPlan,
  onTransition,
  onExecute,
}: PmDueQueueViewProps) {
  const { t } = useTranslation("pm");

  const columns: ColumnDef<PmOccurrenceRow>[] = useMemo(
    () => [
      {
        id: "due",
        accessorFn: (o) => o.due_at ?? "",
        header: t("table.due"),
        cell: ({ row }) => <PmDueLabel occurrence={row.original} />,
      },
      {
        id: "plan",
        accessorFn: (o) => o.plan_code ?? "",
        header: t("table.plan"),
        cell: ({ row }) => (
          <button
            type="button"
            className="min-h-9 text-left hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("actions.openPlan")}
            onClick={(e) => {
              e.stopPropagation();
              onOpenPlan(row.original.pm_plan_id);
            }}
          >
            <div className="font-mono text-xs">{row.original.plan_code}</div>
            <div className="max-w-[220px] truncate text-xs text-text-muted">
              {row.original.plan_title}
            </div>
          </button>
        ),
      },
      {
        id: "asset",
        accessorFn: (o) => o.asset_code ?? "",
        header: t("table.asset"),
        cell: ({ row }) => (
          <PmAssetCell code={row.original.asset_code} name={row.original.asset_name} />
        ),
      },
      {
        id: "strategy",
        accessorFn: (o) => o.strategy_type ?? "",
        header: t("table.strategy"),
        cell: ({ row }) =>
          row.original.strategy_type
            ? t(`strategy.${row.original.strategy_type}`, {
                defaultValue: row.original.strategy_type,
              })
            : t("labels.none"),
      },
      {
        accessorKey: "status",
        header: t("table.status"),
        cell: ({ row }) => <PmOccurrenceStatus occurrence={row.original} />,
      },
      {
        id: "wo",
        accessorFn: (o) => o.linked_work_order_code ?? "",
        header: t("table.workOrder"),
        cell: ({ row }) =>
          row.original.linked_work_order_code ? (
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.linked_work_order_code}
              entityId={row.original.linked_work_order_id}
            />
          ) : (
            <span className="text-xs text-text-muted">{t("labels.none")}</span>
          ),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => {
          const occ = row.original;
          const hasTargets = allowedOccurrenceTargets(occ.status).length > 0;
          return (
            <div
              className="flex flex-wrap gap-1"
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => e.stopPropagation()}
            >
              <PermissionGate permission={P.PM_EDIT}>
                {hasTargets ? (
                  <Button size="sm" variant="outline" onClick={() => onTransition(occ)}>
                    {t("actions.transitionOccurrence")}
                  </Button>
                ) : null}
                {canLogExecution(occ) ? (
                  <Button size="sm" variant="secondary" onClick={() => onExecute(occ)}>
                    {t("actions.logExecution")}
                  </Button>
                ) : null}
              </PermissionGate>
            </div>
          );
        },
      },
    ],
    [t, onOpenPlan, onTransition, onExecute],
  );

  return (
    <div className="min-h-0 flex-1 overflow-auto p-4">
      <DataTable
        columns={columns}
        data={occurrences}
        isLoading={loading}
        onRowClick={(row) => onOpenPlan(row.pm_plan_id)}
      />
      {!loading && occurrences.length === 0 ? (
        <p className="py-6 text-center text-sm text-text-muted">{t("empty.noDue")}</p>
      ) : null}
    </div>
  );
}
