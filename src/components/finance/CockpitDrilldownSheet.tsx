/**
 * Documents behind one cockpit figure: the actuals (OT settlements, manual entries, reversals)
 * and the open commitments (purchase orders and requisitions raised for OTs, manual ones) of a
 * cost centre and category.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { StatusChip } from "@/components/ui/status-chip";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type BudgetCockpitDrilldown,
  type BudgetCockpitRow,
  getBudgetCockpitDrilldown,
} from "@/services/finance-cost-service";

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
  const [data, setData] = useState<BudgetCockpitDrilldown | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
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
  }, [row, fiscalYear, budgetVersionId]);

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
                  bucket: t(`bucket.${row.bucket}`, { defaultValue: row.bucket }),
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
              {error ? <ErrorState error={error} title={t("drill.loadError")} /> : null}
              {data ? (
                <>
                  <section aria-labelledby="drill-actuals">
                    <h3 id="drill-actuals" className="mb-2 font-semibold text-text-primary">
                      {t("drill.actuals", { count: data.actuals.length })}
                    </h3>
                    {data.actuals.length === 0 ? (
                      <p className="text-text-muted">{t("drill.noActuals")}</p>
                    ) : (
                      <table className="w-full text-xs">
                        <thead>
                          <tr className="border-b border-surface-border text-text-muted">
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.period")}
                            </th>
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.source")}
                            </th>
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.asset")}
                            </th>
                            <th scope="col" className="py-1 text-end font-medium">
                              {t("drill.col.amount")}
                            </th>
                          </tr>
                        </thead>
                        <tbody>
                          {data.actuals.map((a) => (
                            <tr key={a.actual_id} className="border-b border-surface-border/60">
                              <td className="py-1 align-top">
                                {a.period_month ? String(a.period_month).padStart(2, "0") : "—"}
                                <div className="text-text-muted">{formatDate(a.event_at)}</div>
                              </td>
                              <td className="py-1 align-top">
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
                              </td>
                              <td className="py-1 align-top">{a.equipment_code ?? "—"}</td>
                              <td
                                className={cn(
                                  "py-1 text-end align-top font-mono tabular-nums",
                                  a.amount < 0 && "text-text-muted",
                                )}
                              >
                                {money(a.amount)}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                  </section>
                  <section aria-labelledby="drill-commitments">
                    <h3 id="drill-commitments" className="mb-2 font-semibold text-text-primary">
                      {t("drill.commitments", { count: data.commitments.length })}
                    </h3>
                    {data.commitments.length === 0 ? (
                      <p className="text-text-muted">{t("drill.noCommitments")}</p>
                    ) : (
                      <table className="w-full text-xs">
                        <thead>
                          <tr className="border-b border-surface-border text-text-muted">
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.document")}
                            </th>
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.workOrder")}
                            </th>
                            <th scope="col" className="py-1 text-start font-medium">
                              {t("drill.col.due")}
                            </th>
                            <th scope="col" className="py-1 text-end font-medium">
                              {t("drill.col.open")}
                            </th>
                          </tr>
                        </thead>
                        <tbody>
                          {data.commitments.map((c, i) => (
                            <tr
                              // biome-ignore lint/suspicious/noArrayIndexKey: a document can appear on several lines
                              key={`${c.kind}-${c.reference}-${i}`}
                              className="border-b border-surface-border/60"
                            >
                              <td className="py-1">
                                <span className="font-medium">{c.reference}</span>
                                <div className="text-text-muted">
                                  {t(`drill.kind.${c.kind}`)}
                                  {c.supplier_name ? ` · ${c.supplier_name}` : ""}
                                </div>
                              </td>
                              <td className="py-1">
                                {c.work_order_id ? (
                                  <LinkedEntityBadge
                                    entity="work_order"
                                    code={c.work_order_code}
                                    entityId={c.work_order_id}
                                  />
                                ) : (
                                  "—"
                                )}
                              </td>
                              <td className="py-1">{c.due_at ? formatDate(c.due_at) : "—"}</td>
                              <td className="py-1 text-end font-mono tabular-nums">
                                {money(c.amount)}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
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
