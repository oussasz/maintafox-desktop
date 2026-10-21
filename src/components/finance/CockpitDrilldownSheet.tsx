/**
 * Documents behind one cockpit figure: the actuals (OT settlements, manual entries, reversals)
 * and the open commitments (purchase orders and requisitions raised for OTs, manual ones) of a
 * cost centre and category.
 */
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { ErrorState, LoadingState, TruncationNotice } from "@/components/ui/data-state";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { StatusChip } from "@/components/ui/status-chip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { mfTable } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type BudgetCockpitDrilldown,
  type BudgetCockpitRow,
  getBudgetCockpitDrilldown,
} from "@/services/finance-cost-service";

/** Actual lines read per page (the server allows 1 to 500). */
const DRILLDOWN_PAGE = 200;

export interface CockpitDrilldownSheetProps {
  row: BudgetCockpitRow | null;
  fiscalYear: number;
  budgetVersionId: number | null;
  currency: string | null;
  onClose: () => void;
}

export function CockpitDrilldownSheet({
  row,
  fiscalYear,
  budgetVersionId,
  currency,
  onClose,
}: CockpitDrilldownSheetProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatDate } = useFormatters();
  const { categoryLabel, periodLabel } = useFinanceCatalogs();
  const [data, setData] = useState<BudgetCockpitDrilldown | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [loadingMore, setLoadingMore] = useState(false);
  /**
   * Which figure the sheet is showing. Bumped whenever the row, year, version or retry changes,
   * so an answer for an earlier figure (a late "load more" page) is dropped instead of being
   * appended to the lines and totals of the figure now on screen.
   */
  const generation = useRef(0);

  /** Next page of actual lines (the server pages them; the totals cover every line). */
  const loadMore = () => {
    if (!row || !data || loadingMore) return;
    const asked = generation.current;
    setLoadingMore(true);
    getBudgetCockpitDrilldown({
      fiscal_year: fiscalYear,
      budget_version_id: budgetVersionId,
      cost_center_id: row.cost_center_id,
      bucket: row.bucket,
      limit: DRILLDOWN_PAGE,
      offset: data.actuals.length,
    })
      .then((page) => {
        if (asked !== generation.current) return;
        setData((prev) => {
          if (!prev) return prev;
          const known = new Set(prev.actuals.map((a) => a.actual_id));
          return {
            ...prev,
            actuals: [...prev.actuals, ...page.actuals.filter((a) => !known.has(a.actual_id))],
            actuals_total: page.actuals_total,
            actuals_amount: page.actuals_amount,
          };
        });
      })
      .catch(() => {
        // The global IPC toast reports the failure; the loaded lines stay.
      })
      .finally(() => {
        if (asked === generation.current) setLoadingMore(false);
      });
  };

  useEffect(() => {
    generation.current += 1;
    setLoadingMore(false);
    if (!row) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setData(null);
    getBudgetCockpitDrilldown({
      fiscal_year: fiscalYear,
      budget_version_id: budgetVersionId,
      cost_center_id: row.cost_center_id,
      bucket: row.bucket,
      limit: DRILLDOWN_PAGE,
      offset: 0,
    })
      .then((d) => {
        if (!cancelled) setData(d);
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
  }, [row, fiscalYear, budgetVersionId, attempt]);

  const money = (v: number) => formatCurrency(v, currency ?? undefined);

  return (
    <Sheet open={row != null} onOpenChange={(open) => (open ? undefined : onClose())}>
      <SheetContent side="right" className="w-full overflow-y-auto sm:max-w-2xl">
        {row ? (
          <>
            <SheetHeader>
              <SheetTitle>
                {t("drill.title", {
                  cc: row.cost_center_code,
                  bucket: categoryLabel(row.bucket),
                })}
              </SheetTitle>
              <SheetDescription>
                {t("drill.summary", {
                  actual: money(row.actual),
                  committed: money(row.committed),
                  budget: money(row.budget),
                })}
              </SheetDescription>
            </SheetHeader>
            <div className="mt-4 space-y-6 text-sm">
              {loading ? <LoadingState /> : null}
              {error ? (
                <ErrorState
                  error={error}
                  title={t("drill.loadError")}
                  onRetry={() => setAttempt((n) => n + 1)}
                />
              ) : null}
              {data ? (
                <>
                  <section aria-labelledby="drill-actuals">
                    <h3 id="drill-actuals" className="mb-2 font-semibold text-text-primary">
                      {t("drill.actuals", { count: data.actuals_total })}
                    </h3>
                    {data.actuals_total > 0 ? (
                      <p className="mb-2 text-text-secondary" data-testid="drill-actuals-total">
                        {t("drill.actualsTotal", {
                          count: data.actuals_total,
                          amount: money(data.actuals_amount),
                        })}
                      </p>
                    ) : null}
                    {data.actuals.length === 0 ? (
                      <p className="text-text-muted">{t("drill.noActuals")}</p>
                    ) : (
                      <Table className="text-xs">
                        <TableHeader>
                          <TableRow className={mfTable.header}>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.period")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.source")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.asset")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-end font-medium">
                              {t("drill.col.amount")}
                            </TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {data.actuals.map((a) => (
                            <TableRow key={a.actual_id}>
                              <TableCell className="py-1.5 align-top">
                                {a.period_month ? periodLabel(a.period_month) : "—"}
                                <div className="text-text-muted">{formatDate(a.event_at)}</div>
                              </TableCell>
                              <TableCell className="py-1.5 align-top">
                                {a.work_order_id ? (
                                  <LinkedEntityBadge
                                    entity="work_order"
                                    code={a.work_order_code}
                                    entityId={a.work_order_id}
                                    title={a.work_order_title}
                                  />
                                ) : (
                                  <span>
                                    {t(`drill.sourceType.${a.source_type}`, {
                                      defaultValue: a.source_type,
                                    })}
                                  </span>
                                )}
                                <div className="mt-0.5 flex flex-wrap gap-1">
                                  {a.rate_card_lane ? (
                                    <span className="text-text-muted">
                                      {t(`drill.lane.${a.rate_card_lane}`, {
                                        defaultValue: a.rate_card_lane,
                                      })}
                                    </span>
                                  ) : null}
                                  {a.is_reversal || a.posting_status === "reversed" ? (
                                    <StatusChip
                                      tone="neutral"
                                      label={
                                        a.is_reversal ? t("drill.reversal") : t("drill.reversed")
                                      }
                                    />
                                  ) : null}
                                </div>
                              </TableCell>
                              <TableCell className="py-1.5 align-top">
                                {a.equipment_code ?? "—"}
                              </TableCell>
                              <TableCell
                                className={cn(
                                  "py-1.5 text-end align-top font-mono tabular-nums",
                                  a.amount < 0 && "text-text-muted",
                                )}
                              >
                                {money(a.amount)}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    )}
                    <TruncationNotice
                      className="mt-2"
                      shown={data.actuals.length}
                      total={data.actuals_total}
                      onLoadMore={loadMore}
                      loadingMore={loadingMore}
                    />
                  </section>
                  <section aria-labelledby="drill-commitments">
                    <h3 id="drill-commitments" className="mb-2 font-semibold text-text-primary">
                      {t("drill.commitments", { count: data.commitments.length })}
                    </h3>
                    {data.commitments.length === 0 ? (
                      <p className="text-text-muted">{t("drill.noCommitments")}</p>
                    ) : (
                      <Table className="text-xs">
                        <TableHeader>
                          <TableRow className={mfTable.header}>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.document")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.workOrder")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-start font-medium">
                              {t("drill.col.due")}
                            </TableHead>
                            <TableHead scope="col" className="h-9 text-end font-medium">
                              {t("drill.col.open")}
                            </TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {data.commitments.map((c, i) => (
                            <TableRow
                              // biome-ignore lint/suspicious/noArrayIndexKey: a document can appear on several lines
                              key={`${c.kind}-${c.reference}-${i}`}
                            >
                              <TableCell className="py-1">
                                <span className="font-medium">{c.reference}</span>
                                <div className="text-text-muted">
                                  {t(`drill.kind.${c.kind}`)}
                                  {c.supplier_name ? ` · ${c.supplier_name}` : ""}
                                </div>
                              </TableCell>
                              <TableCell className="py-1">
                                {c.work_order_id ? (
                                  <LinkedEntityBadge
                                    entity="work_order"
                                    code={c.work_order_code}
                                    entityId={c.work_order_id}
                                  />
                                ) : (
                                  "—"
                                )}
                              </TableCell>
                              <TableCell className="py-1">
                                {c.due_at ? formatDate(c.due_at) : "—"}
                              </TableCell>
                              <TableCell className="py-1 text-end font-mono tabular-nums">
                                {money(c.amount)}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    )}
                  </section>
                </>
              ) : null}
            </div>
          </>
        ) : null}
      </SheetContent>
    </Sheet>
  );
}
