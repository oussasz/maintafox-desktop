/**
 * Dormant stock (UX L3-15): parts still on the shelf with no issue for a chosen number of
 * months, most valuable first, so the storekeeper can find obsolete stock that ties up money.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { KpiTile } from "@/components/metrics/KpiTile";
import { ErrorState } from "@/components/ui/data-state";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { type DormantStockRow, listInventoryDormantStock } from "@/services/inventory-service";

/** Periods offered in the "no issue for" filter. */
export const DORMANT_PERIODS = [6, 12, 24, 36] as const;

/** Total value of the dormant rows that have a known cost. */
export function dormantTotal(rows: readonly DormantStockRow[]): number {
  return rows.reduce((sum, r) => sum + (r.stock_value ?? 0), 0);
}

interface DormantStockPanelProps {
  reloadToken?: number;
  onOpenArticle?: (articleId: number) => void;
}

export function DormantStockPanel({ reloadToken = 0, onOpenArticle }: DormantStockPanelProps) {
  const { t } = useTranslation("inventory");
  const { formatCurrency, formatNumber, formatDate } = useFormatters();
  const [months, setMonths] = useState<number>(12);
  const [rows, setRows] = useState<DormantStockRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    listInventoryDormantStock(months)
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [months, reloadToken, version]);

  const columns = useMemo<ColumnDef<DormantStockRow>[]>(
    () => [
      {
        accessorKey: "article_code",
        header: t("dormant.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.article_code}</span>,
      },
      { accessorKey: "article_name", header: t("dormant.columns.name") },
      {
        accessorKey: "on_hand_qty",
        header: t("dormant.columns.quantity"),
        cell: ({ row }) => (
          <span className="tabular-nums">
            {formatNumber(row.original.on_hand_qty, { maximumFractionDigits: 3 })}
          </span>
        ),
      },
      {
        accessorKey: "stock_value",
        header: t("dormant.columns.value"),
        cell: ({ row }) =>
          row.original.stock_value != null ? (
            <span className="tabular-nums font-medium">
              {formatCurrency(row.original.stock_value)}
            </span>
          ) : (
            <span className="text-text-muted">—</span>
          ),
      },
      {
        accessorKey: "last_issue_at",
        header: t("dormant.columns.lastIssue"),
        cell: ({ row }) =>
          row.original.last_issue_at
            ? formatDate(row.original.last_issue_at)
            : t("dormant.neverIssued"),
      },
      {
        accessorKey: "last_movement_at",
        header: t("dormant.columns.lastMovement"),
        cell: ({ row }) =>
          row.original.last_movement_at ? formatDate(row.original.last_movement_at) : "—",
      },
    ],
    [formatCurrency, formatDate, formatNumber, t],
  );

  return (
    <section
      aria-labelledby="inv-dormant-title"
      className="rounded-lg border border-surface-border p-4"
    >
      <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 id="inv-dormant-title" className="text-base font-semibold">
            {t("dormant.title")}
          </h2>
          <p className="text-sm text-text-muted">{t("dormant.description")}</p>
        </div>
        <div className="space-y-1">
          <Label htmlFor="inv-dormant-months">{t("dormant.period.label")}</Label>
          <Select value={String(months)} onValueChange={(v) => v && setMonths(Number(v))}>
            <SelectTrigger id="inv-dormant-months" className="h-11 w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DORMANT_PERIODS.map((m) => (
                <SelectItem key={m} value={String(m)}>
                  {t("dormant.period.months", { count: m })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {error ? (
        <ErrorState
          error={error}
          title={t("dormant.loadError")}
          onRetry={() => setVersion((v) => v + 1)}
        />
      ) : (
        <>
          <div className="mb-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
            <KpiTile label={t("dormant.count")} value={formatNumber(rows.length)} />
            <KpiTile label={t("dormant.total")} value={formatCurrency(dormantTotal(rows))} />
          </div>
          <DataTable
            columns={columns}
            data={rows}
            isLoading={loading}
            searchable
            emptyMessage={t("dormant.empty", { count: months })}
            {...(onOpenArticle
              ? { onRowClick: (row: DormantStockRow) => onOpenArticle(row.article_id) }
              : {})}
          />
        </>
      )}
    </section>
  );
}
