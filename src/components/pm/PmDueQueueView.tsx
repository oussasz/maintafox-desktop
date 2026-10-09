import type { ColumnDef } from "@tanstack/react-table";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatDate } from "@/utils/format-date";
import type { PmOccurrence } from "@shared/ipc-types";

interface PmDueQueueViewProps {
  occurrences: PmOccurrence[];
  loading: boolean;
  onOpenPlan: (planId: number) => void;
  onTransition: (occurrence: PmOccurrence) => void;
  onExecute: (occurrence: PmOccurrence) => void;
}

export function PmDueQueueView({
  occurrences,
  loading,
  onOpenPlan,
  onTransition,
  onExecute,
}: PmDueQueueViewProps) {
  const { t, i18n } = useTranslation("pm");

  const columns: ColumnDef<PmOccurrence>[] = useMemo(
    () => [
      {
        id: "due",
        header: t("table.due"),
        cell: ({ row }) => {
          const o = row.original;
          if (o.due_at) return formatDate(o.due_at, i18n.language);
          if (o.due_meter_value != null) return String(o.due_meter_value);
          return t("labels.none");
        },
      },
      {
        id: "plan",
        header: t("table.plan"),
        cell: ({ row }) => (
          <button
            type="button"
            className="text-left hover:underline"
            onClick={(e) => {
              e.stopPropagation();
              onOpenPlan(row.original.pm_plan_id);
            }}
          >
            <div className="font-mono text-xs">{row.original.plan_code}</div>
            <div className="max-w-[200px] truncate text-xs text-muted-foreground">
              {row.original.plan_title}
            </div>
          </button>
        ),
      },
      {
        id: "strategy",
        header: t("table.strategy"),
        cell: ({ row }) =>
          t(`strategy.${row.original.strategy_type ?? ""}`, {
            defaultValue: row.original.strategy_type ?? t("labels.none"),
          }),
      },
      {
        accessorKey: "status",
        header: t("table.status"),
        cell: ({ row }) => (
          <Badge variant="secondary">
            {t(`occurrenceStatus.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        id: "wo",
        header: t("table.workOrder"),
        cell: ({ row }) => row.original.linked_work_order_code || t("labels.none"),
      },
      {
        id: "actions",
        header: t("table.actions"),
        cell: ({ row }) => (
          <div className="flex gap-1" onClick={(e) => e.stopPropagation()}>
            <Button size="sm" variant="outline" onClick={() => onTransition(row.original)}>
              {t("actions.transitionOccurrence")}
            </Button>
            <Button size="sm" variant="secondary" onClick={() => onExecute(row.original)}>
              {t("actions.logExecution")}
            </Button>
          </div>
        ),
      },
    ],
    [t, i18n.language, onOpenPlan, onTransition, onExecute],
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
        <p className="py-6 text-center text-sm text-muted-foreground">{t("empty.noDue")}</p>
      ) : null}
    </div>
  );
}
