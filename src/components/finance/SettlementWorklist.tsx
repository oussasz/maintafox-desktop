/**
 * Settlement worklist: every closed OT and what happened to its cost — posted to a cost centre
 * and month, pending with the reason and what to fix, or reversed by a reopen. Finance retries a
 * pending settlement once the cause is fixed; nothing is posted silently or guessed. Late costs
 * (a settled OT whose cost changes later) are booked automatically; "Settle late costs"
 * catches up those brought by a sync.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { RefreshCcw, Receipt } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { SETTLEMENT_STATUS_TONE } from "@/components/finance/finance-tone";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type WoCostSettlement,
  listWoSettlements,
  postLateWoCosts,
  retryWoSettlement,
} from "@/services/finance-cost-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

const FILTERS = ["pending", "posted", "reversed", "all"] as const;
type Filter = (typeof FILTERS)[number];

export interface SettlementWorklistProps {
  fiscalYear: number;
  reloadToken?: number;
  /** Called after a settlement was posted so the cockpit reloads. */
  onChanged?: () => void;
}

export function SettlementWorklist({
  fiscalYear,
  reloadToken = 0,
  onChanged,
}: SettlementWorklistProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatDate } = useFormatters();
  const { can } = usePermissions();
  const { categoryLabel, periodLabel } = useFinanceCatalogs();
  const [filter, setFilter] = useState<Filter>("pending");
  const [rows, setRows] = useState<WoCostSettlement[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [catchingUp, setCatchingUp] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(
        await listWoSettlements({
          fiscal_year: fiscalYear,
          status: filter === "all" ? null : filter,
          limit: 1000,
        }),
      );
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [fiscalYear, filter]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken forces a refresh
  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const retry = useCallback(
    async (s: WoCostSettlement) => {
      setBusyId(s.id);
      try {
        const result = await retryWoSettlement(s.id);
        if (result.status === "posted") {
          pushAppToast({
            title: t("settlements.retryPosted", {
              code: result.work_order_code,
              amount: formatCurrency(result.total_amount, result.currency_code || undefined),
            }),
            variant: "success",
          });
          onChanged?.();
        } else {
          pushAppToast({
            title: t("settlements.retryStillPending", { code: result.work_order_code }),
            description: t(`settlement.fix.${result.pending_reason ?? "unknown"}`),
          });
        }
        await load();
      } catch {
        // The IPC layer already toasts the translated failure (describeAppError).
      } finally {
        setBusyId(null);
      }
    },
    [t, formatCurrency, load, onChanged],
  );

  const catchUpLateCosts = useCallback(async () => {
    setCatchingUp(true);
    try {
      const touched = await postLateWoCosts();
      pushAppToast({
        title:
          touched > 0 ? t("settlements.lateBooked", { count: touched }) : t("settlements.lateNone"),
        variant: touched > 0 ? "success" : "default",
      });
      if (touched > 0) onChanged?.();
      await load();
    } catch {
      // The IPC layer already toasts the translated failure (describeAppError).
    } finally {
      setCatchingUp(false);
    }
  }, [t, load, onChanged]);

  const canPost = can(P.FIN_POST);
  const columns = useMemo<ColumnDef<WoCostSettlement>[]>(
    () => [
      {
        id: "wo",
        header: t("settlements.col.workOrder"),
        accessorFn: (s) => s.work_order_code,
        cell: ({ row }) => (
          <div className="min-w-0">
            <LinkedEntityBadge
              entity="work_order"
              code={row.original.work_order_code}
              entityId={row.original.work_order_id}
              title={row.original.work_order_title}
            />
            {row.original.work_order_title ? (
              <div className="truncate text-xs text-text-muted">
                {row.original.work_order_title}
              </div>
            ) : null}
          </div>
        ),
      },
      {
        id: "asset",
        header: t("settlements.col.asset"),
        accessorFn: (s) => s.equipment_code ?? "",
        cell: ({ row }) =>
          row.original.equipment_id ? (
            <LinkedEntityBadge
              entity="asset"
              code={row.original.equipment_code}
              entityId={row.original.equipment_id}
            />
          ) : (
            "—"
          ),
      },
      {
        id: "closed",
        header: t("settlements.col.closedAt"),
        accessorFn: (s) => s.closed_at ?? "",
        cell: ({ row }) => formatDate(row.original.closed_at),
      },
      {
        id: "status",
        header: t("settlements.col.status"),
        accessorFn: (s) => s.status,
        cell: ({ row }) => {
          const s = row.original;
          return (
            <div className="space-y-0.5">
              <StatusChip
                tone={toneOf(SETTLEMENT_STATUS_TONE, s.status)}
                label={t(`settlement.status.${s.status}`)}
              />
              {s.duplicate_active ? (
                <div className="max-w-xs text-xs text-tone-danger-fg">
                  {t("settlements.duplicate")}
                </div>
              ) : null}
              {s.status === "pending" ? (
                <div className="max-w-xs text-xs text-text-secondary">
                  {t(`settlement.reason.${s.pending_reason ?? "unknown"}`)}
                  <div className="text-text-muted">
                    {t(`settlement.fix.${s.pending_reason ?? "unknown"}`)}
                  </div>
                </div>
              ) : s.status === "reversed" && s.reversal_reason ? (
                <div className="max-w-xs truncate text-xs text-text-muted">{s.reversal_reason}</div>
              ) : null}
            </div>
          );
        },
      },
      {
        id: "cc",
        header: t("settlements.col.costCenter"),
        accessorFn: (s) => s.cost_center_code ?? "",
        cell: ({ row }) =>
          row.original.cost_center_code ? (
            <span title={row.original.cost_center_name ?? undefined}>
              {row.original.cost_center_code}
            </span>
          ) : (
            "—"
          ),
      },
      {
        id: "period",
        header: t("settlements.col.period"),
        accessorFn: (s) => (s.period_month ?? 0) + (s.fiscal_year ?? 0) * 100,
        cell: ({ row }) =>
          row.original.period_month
            ? `${periodLabel(row.original.period_month)} · ${row.original.fiscal_year}`
            : "—",
      },
      {
        id: "total",
        header: () => <span className="block text-end">{t("settlements.col.total")}</span>,
        accessorFn: (s) => s.total_amount,
        cell: ({ row }) => {
          const s = row.original;
          const cur = s.currency_code || undefined;
          return (
            <div className="text-end">
              <div className="font-mono tabular-nums">{formatCurrency(s.total_amount, cur)}</div>
              <div className="text-[11px] text-text-muted">
                {(
                  [
                    ["labor", s.labor_amount],
                    ["parts", s.parts_amount],
                    ["services", s.services_amount],
                    ["other", s.other_amount],
                  ] as const
                )
                  .map(([code, amount]) => `${categoryLabel(code)} ${formatCurrency(amount, cur)}`)
                  .join(" · ")}
              </div>
            </div>
          );
        },
      },
      {
        id: "actions",
        header: "",
        enableSorting: false,
        cell: ({ row }) =>
          row.original.status === "pending" && canPost ? (
            <Button
              size="sm"
              variant="outline"
              className="h-7 gap-1 text-xs"
              disabled={busyId === row.original.id}
              onClick={(e) => {
                e.stopPropagation();
                void retry(row.original);
              }}
            >
              <RefreshCcw className="h-3.5 w-3.5" aria-hidden />
              {t("settlements.retry")}
            </Button>
          ) : null,
      },
    ],
    [t, formatDate, formatCurrency, canPost, busyId, retry, categoryLabel, periodLabel],
  );

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-3xl text-sm text-text-secondary">{t("settlements.intro")}</p>
        <div className="flex items-center gap-2">
          {canPost ? (
            <Button
              size="sm"
              variant="outline"
              className="h-8 gap-1 text-xs"
              disabled={catchingUp}
              title={t("settlements.lateHint")}
              onClick={() => void catchUpLateCosts()}
            >
              <Receipt className="h-3.5 w-3.5" aria-hidden />
              {t("settlements.lateCatchUp")}
            </Button>
          ) : null}
          <label htmlFor="settlement-filter" className="text-xs text-text-muted">
            {t("settlements.filter")}
          </label>
          <Select value={filter} onValueChange={(v) => setFilter(v as Filter)}>
            <SelectTrigger id="settlement-filter" className="h-8 w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FILTERS.map((f) => (
                <SelectItem key={f} value={f}>
                  {t(`settlements.filters.${f}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {loading && rows.length === 0 ? <LoadingState /> : null}
      {error ? (
        <ErrorState error={error} title={t("settlements.loadError")} onRetry={() => void load()} />
      ) : null}
      {!loading && !error && rows.length === 0 ? (
        <EmptyState
          title={t(`settlements.empty.${filter}`)}
          description={t("settlements.emptyBody", { year: fiscalYear })}
        />
      ) : null}
      {rows.length > 0 ? <DataTable columns={columns} data={rows} pageSize={50} /> : null}
    </div>
  );
}
