/**
 * Stock valuation report (by article, warehouse or family, as at a date) and valuation periods.
 *
 * The report replays the stock ledger up to the date at weighted-average cost; at the end of a
 * closed period it shows the frozen snapshot taken when the period was closed. Closing a period
 * (finance posting permission) freezes its valuation and refuses later postings dated inside it.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Lock } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type InventoryValuationPeriod,
  type ValuationGroupBy,
  type ValuationReport,
  closeInventoryValuationPeriod,
  getInventoryValuationReport,
  listInventoryValuationPeriods,
} from "@/services/inventory-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

const GROUPS: readonly ValuationGroupBy[] = ["article", "warehouse", "family"];

/** Local calendar date as YYYY-MM-DD. */
export function isoDay(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** The previous calendar month (the usual period to close). */
export function previousMonth(today: Date): { start: string; end: string } {
  const start = new Date(today.getFullYear(), today.getMonth() - 1, 1);
  const end = new Date(today.getFullYear(), today.getMonth(), 0);
  return { start: isoDay(start), end: isoDay(end) };
}

/** Why the close form cannot be submitted, or null. */
export function closePeriodBlocker(
  start: string,
  end: string,
  today: string,
): "dates" | "order" | "future" | null {
  if (!start || !end) return "dates";
  if (end < start) return "order";
  if (end >= today) return "future";
  return null;
}

type Row = ValuationReport["rows"][number];

export interface InventoryValuationPanelProps {
  reloadToken?: number;
}

export function InventoryValuationPanel({ reloadToken = 0 }: InventoryValuationPanelProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatCurrency, formatNumber, formatDate, formatDateTime, currencyCode } =
    useFormatters();
  const qty = useCallback(
    (v: number) => formatNumber(v, { maximumFractionDigits: 3 }),
    [formatNumber],
  );
  const today = isoDay(new Date());

  const [groupBy, setGroupBy] = useState<ValuationGroupBy>("warehouse");
  // `null` is the live stock (today), whatever the clock says later: a screen left open past
  // midnight stays live instead of turning into "as at yesterday". A date is a chosen past day.
  const [asAt, setAsAt] = useState<string | null>(null);
  const [report, setReport] = useState<ValuationReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);

  const [periods, setPeriods] = useState<InventoryValuationPeriod[]>([]);
  const [periodsError, setPeriodsError] = useState<unknown>(null);
  const [periodsLoading, setPeriodsLoading] = useState(false);

  const [closeOpen, setCloseOpen] = useState(false);
  const [closeStart, setCloseStart] = useState("");
  const [closeEnd, setCloseEnd] = useState("");
  const [closeNotes, setCloseNotes] = useState("");
  const [closing, setClosing] = useState(false);
  const [closeError, setCloseError] = useState<unknown>(null);

  useEffect(() => {
    // A cleared date field reads nothing until a date is entered.
    if (asAt === "") return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    getInventoryValuationReport(groupBy, asAt)
      .then((r) => {
        if (!cancelled) setReport(r);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [groupBy, asAt, version, reloadToken]);

  const loadPeriods = useCallback(async () => {
    setPeriodsLoading(true);
    setPeriodsError(null);
    try {
      setPeriods(await listInventoryValuationPeriods());
    } catch (err) {
      setPeriodsError(err);
    } finally {
      setPeriodsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPeriods();
  }, [loadPeriods, reloadToken]);

  const openClose = () => {
    // Next period to close: the day after the last closed period, else last month.
    const lastEnd = periods.reduce<string | null>(
      (max, p) => (max === null || p.period_end > max ? p.period_end : max),
      null,
    );
    const suggestion = previousMonth(new Date());
    if (lastEnd) {
      const next = new Date(`${lastEnd}T00:00:00`);
      next.setDate(next.getDate() + 1);
      const start = isoDay(next);
      const monthEnd = isoDay(new Date(next.getFullYear(), next.getMonth() + 1, 0));
      setCloseStart(start);
      setCloseEnd(monthEnd < today ? monthEnd : suggestion.end >= start ? suggestion.end : "");
    } else {
      setCloseStart(suggestion.start);
      setCloseEnd(suggestion.end);
    }
    setCloseNotes("");
    setCloseError(null);
    setCloseOpen(true);
  };

  const blocker = closePeriodBlocker(closeStart, closeEnd, today);

  const submitClose = async () => {
    if (blocker || closing) return;
    setClosing(true);
    setCloseError(null);
    try {
      const period = await closeInventoryValuationPeriod({
        period_start: closeStart,
        period_end: closeEnd,
        notes: closeNotes.trim() || null,
      });
      pushAppToast({
        title: t("valuation.close.done", {
          code: period.period_code,
          value: formatCurrency(period.total_value),
        }),
        variant: "success",
      });
      setCloseOpen(false);
      await loadPeriods();
      setVersion((v) => v + 1);
    } catch (err) {
      setCloseError(err);
    } finally {
      setClosing(false);
    }
  };

  const groupName = useCallback(
    (row: Row) =>
      groupBy === "family" && row.group_id == null ? t("valuation.noFamily") : row.group_name,
    [groupBy, t],
  );

  const columns = useMemo<ColumnDef<Row>[]>(
    () => [
      {
        accessorKey: "group_code",
        header: t("valuation.columns.code"),
        cell: ({ row }) => (
          <span className="font-mono text-xs">{row.original.group_code || "—"}</span>
        ),
      },
      {
        accessorKey: "group_name",
        header: t("valuation.columns.name"),
        cell: ({ row }) => groupName(row.original),
      },
      ...(groupBy === "article"
        ? []
        : [
            {
              accessorKey: "article_count",
              header: t("valuation.columns.articles"),
              cell: ({ row }: { row: { original: Row } }) => (
                <span className="tabular-nums">{formatNumber(row.original.article_count)}</span>
              ),
            } satisfies ColumnDef<Row>,
          ]),
      // Quantities are only added up within one article (one stock unit): a warehouse or family
      // total would add metres to litres.
      ...(groupBy === "article"
        ? [
            {
              accessorKey: "quantity",
              header: t("valuation.columns.quantity"),
              cell: ({ row }: { row: { original: Row } }) => (
                <span className="tabular-nums">{qty(row.original.quantity)}</span>
              ),
            } satisfies ColumnDef<Row>,
          ]
        : []),
      {
        accessorKey: "value",
        header: t("valuation.columns.value"),
        cell: ({ row }) => (
          <span className="tabular-nums font-medium">{formatCurrency(row.original.value)}</span>
        ),
      },
      {
        accessorKey: "unvalued_qty",
        header: t("valuation.columns.unvalued"),
        cell: ({ row }) =>
          row.original.unvalued_qty > 0 ? (
            <StatusChip
              label={
                groupBy === "article"
                  ? qty(row.original.unvalued_qty)
                  : t("valuation.unvaluedPresent")
              }
              tone="warning"
            />
          ) : (
            <span className="text-text-muted">—</span>
          ),
      },
    ],
    [formatCurrency, formatNumber, groupBy, groupName, qty, t],
  );

  const periodColumns = useMemo<ColumnDef<InventoryValuationPeriod>[]>(
    () => [
      {
        accessorKey: "period_code",
        header: t("valuation.periods.columns.code"),
        cell: ({ row }) => (
          <span className="flex items-center gap-1 font-mono text-xs">
            <Lock className="h-3.5 w-3.5 text-text-muted" aria-hidden />
            {row.original.period_code}
          </span>
        ),
      },
      {
        id: "dates",
        header: t("valuation.periods.columns.dates"),
        cell: ({ row }) =>
          t("valuation.periods.range", {
            start: formatDate(row.original.period_start),
            end: formatDate(row.original.period_end),
          }),
      },
      {
        accessorKey: "total_value",
        header: t("valuation.periods.columns.value"),
        cell: ({ row }) => (
          <span className="tabular-nums">
            {formatCurrency(row.original.total_value, row.original.currency_code)}
          </span>
        ),
      },
      {
        accessorKey: "closed_by_name",
        header: t("valuation.periods.columns.closedBy"),
        cell: ({ row }) => row.original.closed_by_name ?? "—",
      },
      {
        accessorKey: "closed_at",
        header: t("valuation.periods.columns.closedAt"),
        cell: ({ row }) => formatDateTime(row.original.closed_at),
      },
      {
        accessorKey: "notes",
        header: t("valuation.periods.columns.notes"),
        cell: ({ row }) => row.original.notes ?? "—",
      },
    ],
    [formatCurrency, formatDate, formatDateTime, t],
  );

  return (
    <div className="space-y-6">
      <section
        aria-labelledby="inv-valuation-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-4 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h2 id="inv-valuation-title" className="text-base font-semibold">
              {t("valuation.title")}
            </h2>
            <p className="text-sm text-text-muted">
              {t("valuation.description", { currency: report?.currency_code ?? currencyCode })}
            </p>
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <div className="space-y-1">
              <Label htmlFor="inv-valuation-group">{t("valuation.groupBy.label")}</Label>
              <Select value={groupBy} onValueChange={(v) => v && setGroupBy(v as ValuationGroupBy)}>
                <SelectTrigger id="inv-valuation-group" className="h-11 w-44">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {GROUPS.map((g) => (
                    <SelectItem key={g} value={g}>
                      {t(`valuation.groupBy.${g}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-valuation-asat">{t("valuation.asAt.label")}</Label>
              <DateField
                id="inv-valuation-asat"
                className="h-11 w-44"
                max={today}
                value={asAt ?? today}
                onChange={(v) => setAsAt(v === today ? null : v)}
              />
            </div>
          </div>
        </div>

        {error ? (
          <ErrorState
            error={error}
            title={t("valuation.loadError")}
            onRetry={() => setVersion((v) => v + 1)}
          />
        ) : (
          <>
            <div className="mb-3 grid grid-cols-1 gap-2 sm:grid-cols-3">
              <KpiTile
                label={t("valuation.total")}
                value={report ? formatCurrency(report.total_value, report.currency_code) : "—"}
              />
              <KpiTile
                label={t("valuation.unvaluedKpi")}
                value={
                  report
                    ? report.unvalued_qty > 0
                      ? t("valuation.unvaluedPresent")
                      : t("valuation.unvaluedNone")
                    : "—"
                }
                {...(report && report.unvalued_qty > 0 ? { tone: "warning" as const } : {})}
                hint={t("valuation.unvaluedByArticle")}
              />
              <KpiTile
                label={t("valuation.basis")}
                value={
                  report?.frozen_period_code
                    ? t("valuation.frozenValue", { code: report.frozen_period_code })
                    : t("valuation.liveValue")
                }
                hint={
                  report?.frozen_period_code ? t("valuation.frozenHint") : t("valuation.liveHint")
                }
              />
            </div>
            {!loading && report && report.rows.length === 0 ? (
              <p className="text-sm text-text-muted">{t("valuation.empty")}</p>
            ) : (
              <DataTable
                columns={columns}
                data={report?.rows ?? []}
                isLoading={loading}
                searchable={groupBy === "article"}
              />
            )}
          </>
        )}
      </section>

      <section
        aria-labelledby="inv-periods-title"
        className="rounded-lg border border-surface-border p-4"
      >
        <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 id="inv-periods-title" className="text-base font-semibold">
              {t("valuation.periods.title")}
            </h2>
            <p className="text-sm text-text-muted">{t("valuation.periods.description")}</p>
          </div>
          <PermissionGate permission={P.FIN_POST}>
            <Button className="h-11" onClick={openClose}>
              <Lock className="mr-1 h-4 w-4" aria-hidden />
              {t("valuation.periods.close")}
            </Button>
          </PermissionGate>
        </div>
        {periodsError ? (
          <ErrorState
            error={periodsError}
            title={t("valuation.periods.loadError")}
            onRetry={() => void loadPeriods()}
          />
        ) : !periodsLoading && periods.length === 0 ? (
          <p className="text-sm text-text-muted">{t("valuation.periods.empty")}</p>
        ) : (
          <DataTable
            columns={periodColumns}
            data={periods}
            isLoading={periodsLoading}
            searchable={false}
            onRowClick={(p) => setAsAt(p.period_end)}
          />
        )}
      </section>

      <Dialog open={closeOpen} onOpenChange={(open) => !closing && setCloseOpen(open)}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("valuation.close.title")}</DialogTitle>
            <DialogDescription>{t("valuation.close.description")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="inv-close-start">{t("valuation.close.start")}</Label>
              <DateField
                id="inv-close-start"
                className="h-11"
                value={closeStart}
                onChange={(v) => setCloseStart(v)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-close-end">{t("valuation.close.end")}</Label>
              <DateField
                id="inv-close-end"
                className="h-11"
                value={closeEnd}
                onChange={(v) => setCloseEnd(v)}
              />
            </div>
            <div className="space-y-1 sm:col-span-2">
              <Label htmlFor="inv-close-notes">{t("valuation.close.notes")}</Label>
              <Textarea
                id="inv-close-notes"
                rows={2}
                value={closeNotes}
                onChange={(e) => setCloseNotes(e.target.value)}
              />
            </div>
          </div>
          {blocker ? (
            <p id="inv-close-blocker" role="status" className="text-xs text-text-muted">
              {t(`valuation.close.blockers.${blocker}`)}
            </p>
          ) : null}
          {closeError ? <ErrorState error={closeError} title={t("valuation.close.error")} /> : null}
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={closing}
              onClick={() => setCloseOpen(false)}
            >
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={closing || blocker !== null}
              aria-describedby={blocker ? "inv-close-blocker" : undefined}
              onClick={() => void submitClose()}
            >
              {t("valuation.close.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
