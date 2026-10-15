/**
 * OT cost panel (OT detail): estimated vs planned vs committed vs actual by category, the cost
 * centre the OT settles to and its settlement into the budget (posted / pending with the reason).
 *
 * Every figure comes from the record that carries it (labour entries, stock ledger, purchase
 * documents, OT estimate) — see `finance::wo_costing`. Nothing is typed twice and no missing
 * figure is shown as zero: a missing estimate or plan reads "—", unpriced hours and unvalued
 * parts are listed so they can be fixed.
 */
import { AlertTriangle, ChevronDown, ChevronRight, Coins, PencilLine } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { WoCostEstimateDialog } from "@/components/finance/WoCostEstimateDialog";
import { COST_BUCKETS, SETTLEMENT_STATUS_TONE } from "@/components/finance/finance-tone";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { type Tone, mfTone, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type WoCostFigures,
  type WoCostView,
  getWoCostView,
} from "@/services/finance-cost-service";
import { P } from "@shared/rbac/permissions.generated";

export interface WoCostPanelProps {
  woId: number;
  /** OT status code; a change reloads the panel (close settles, reopen reverses). */
  status: string;
}

const ROWS = [...COST_BUCKETS, "total"] as const;
type Row = (typeof ROWS)[number];

/** Variance of the actual against the estimate, else against the plan. */
export function costVariance(
  f: WoCostFigures,
): { base: "estimated" | "planned"; amount: number } | null {
  if (f.estimated != null && f.estimated > 0) {
    return { base: "estimated", amount: f.actual - f.estimated };
  }
  if (f.planned != null && f.planned > 0) return { base: "planned", amount: f.actual - f.planned };
  return null;
}

export function WoCostPanel({ woId, status }: WoCostPanelProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatNumber, formatDate } = useFormatters();
  const { can } = usePermissions();
  const { categoryLabel, periodLabel } = useFinanceCatalogs();
  const [view, setView] = useState<WoCostView | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [showLines, setShowLines] = useState(false);
  const [estimateOpen, setEstimateOpen] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setView(await getWoCostView(woId));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [woId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: a status change re-reads the cost
  useEffect(() => {
    void load();
  }, [load, status]);

  if (loading && !view) {
    return (
      <Card>
        <CardContent className="p-3">
          <LoadingState label={t("woCost.loading")} />
        </CardContent>
      </Card>
    );
  }
  if (error && !view) {
    return (
      <Card>
        <CardContent className="p-3">
          <ErrorState error={error} title={t("woCost.loadError")} onRetry={() => void load()} />
        </CardContent>
      </Card>
    );
  }
  if (!view) return null;

  const b = view.breakdown;
  const money = (v: number | null | undefined) =>
    v == null ? "—" : formatCurrency(v, b.currency_code);
  const figures: Record<Row, WoCostFigures> = {
    labor: b.labor,
    parts: b.parts,
    services: b.services,
    other: b.other,
    total: b.total,
  };
  const settlement = view.settlements[0] ?? null;
  const editable = can(P.OT_EDIT) && b.status_code !== "closed" && b.status_code !== "cancelled";

  const notices: { key: string; tone: Tone; text: string }[] = [];
  if (!b.cost_center) {
    notices.push({ key: "cc", tone: "attention", text: t("woCost.notice.noCostCenter") });
  }
  if (b.unpriced_labor_hours > 0) {
    notices.push({
      key: "rate",
      tone: "warning",
      text: t("woCost.notice.unpricedHours", { hours: formatNumber(b.unpriced_labor_hours) }),
    });
  }
  if (b.unvalued_parts_qty > 0) {
    notices.push({
      key: "unvalued",
      tone: "warning",
      text: t("woCost.notice.unvaluedParts", { qty: formatNumber(b.unvalued_parts_qty) }),
    });
  }
  if (!b.overtime_captured && b.labor_hours > 0) {
    notices.push({ key: "ot", tone: "info", text: t("woCost.notice.overtimeNotCaptured") });
  }
  if (settlement?.duplicate_active) {
    notices.push({ key: "dup", tone: "danger", text: t("woCost.notice.duplicateSettlement") });
  }

  return (
    <Card>
      <CardContent className="space-y-3 p-3">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <h4 className="flex items-center gap-1.5 text-sm font-semibold text-text-primary">
              <Coins className="h-4 w-4 text-text-muted" aria-hidden />
              {t("woCost.title")}
            </h4>
            <p className="mt-0.5 text-xs text-text-muted">
              {b.cost_center
                ? t("woCost.costCenter", {
                    code: b.cost_center.code,
                    name: b.cost_center.name,
                    source: t(`woCost.ccSource.${b.cost_center.source}`),
                  })
                : t("woCost.noCostCenter")}
            </p>
          </div>
          <div className="flex items-center gap-2">
            {settlement ? (
              <StatusChip
                tone={toneOf(SETTLEMENT_STATUS_TONE, settlement.status)}
                label={t(`settlement.status.${settlement.status}`)}
                {...(settlement.pending_reason
                  ? { title: t(`settlement.reason.${settlement.pending_reason}`) }
                  : {})}
              />
            ) : null}
            {editable ? (
              <Button
                size="sm"
                variant="outline"
                className="h-7 gap-1 text-xs"
                onClick={() => setEstimateOpen(true)}
              >
                <PencilLine className="h-3.5 w-3.5" aria-hidden />
                {b.estimate ? t("woCost.editEstimate") : t("woCost.addEstimate")}
              </Button>
            ) : null}
          </div>
        </div>

        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <caption className="sr-only">
              {t("woCost.tableCaption", { code: b.work_order_code })}
            </caption>
            <thead>
              <tr className="border-b border-surface-border text-text-muted">
                <th scope="col" className="py-1 pr-2 text-start font-medium">
                  {t("woCost.col.category")}
                </th>
                {(["estimated", "planned", "committed", "actual", "variance"] as const).map((c) => (
                  <th key={c} scope="col" className="px-2 py-1 text-end font-medium">
                    {t(`woCost.col.${c}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {ROWS.map((row) => {
                const f = figures[row];
                const variance = costVariance(f);
                const over = variance != null && variance.amount > 0.005;
                return (
                  <tr
                    key={row}
                    className={cn(
                      "border-b border-surface-border/60",
                      row === "total" && "font-semibold text-text-primary",
                    )}
                  >
                    <th scope="row" className="py-1 pr-2 text-start font-normal">
                      {row === "total" ? t("woCost.total") : categoryLabel(row)}
                    </th>
                    <td className="px-2 py-1 text-end font-mono tabular-nums">
                      {money(f.estimated)}
                    </td>
                    <td className="px-2 py-1 text-end font-mono tabular-nums">
                      {money(f.planned)}
                    </td>
                    <td className="px-2 py-1 text-end font-mono tabular-nums">
                      {f.committed > 0 ? money(f.committed) : "—"}
                    </td>
                    <td className="px-2 py-1 text-end font-mono tabular-nums">{money(f.actual)}</td>
                    <td
                      className={cn(
                        "px-2 py-1 text-end font-mono tabular-nums",
                        over ? mfTone.danger.text : "text-text-muted",
                      )}
                      title={variance ? t(`woCost.varianceBase.${variance.base}`) : undefined}
                    >
                      {variance ? `${over ? "+" : ""}${money(variance.amount)}` : "—"}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {b.labor_overtime_amount > 0 ? (
          <p className="text-xs text-text-muted">
            {t("woCost.overtimeShare", { amount: money(b.labor_overtime_amount) })}
          </p>
        ) : null}
        {b.planned_labor_gap ? (
          <p className="text-xs text-text-muted">{t(`woCost.plannedGap.${b.planned_labor_gap}`)}</p>
        ) : null}

        {notices.length > 0 ? (
          <ul className="space-y-1">
            {notices.map((n) => (
              <li
                key={n.key}
                className={cn(
                  "flex items-start gap-1.5 rounded-md border px-2 py-1 text-xs",
                  mfTone[n.tone].panel,
                )}
              >
                <AlertTriangle
                  className={cn("mt-0.5 h-3.5 w-3.5 shrink-0", mfTone[n.tone].text)}
                  aria-hidden
                />
                <span>{n.text}</span>
              </li>
            ))}
          </ul>
        ) : null}

        {settlement ? (
          <div className="rounded-md border border-surface-border bg-surface-2/50 px-2 py-1.5 text-xs">
            {settlement.status === "posted" ? (
              <span>
                {t("woCost.settlement.posted", {
                  amount: money(settlement.total_amount),
                  cc: settlement.cost_center_code ?? "—",
                  period: settlement.period_month
                    ? `${periodLabel(settlement.period_month)} · ${settlement.fiscal_year ?? ""}`
                    : "—",
                  date: formatDate(settlement.posted_at),
                })}
                {Math.abs(view.late_cost_total) >= 0.005
                  ? ` ${t("woCost.settlement.lateCosts", { amount: money(view.late_cost_total) })}`
                  : null}
              </span>
            ) : settlement.status === "pending" ? (
              <span>
                {t("woCost.settlement.pending", {
                  reason: t(`settlement.reason.${settlement.pending_reason ?? "unknown"}`),
                })}{" "}
                {t(`settlement.fix.${settlement.pending_reason ?? "unknown"}`)}
              </span>
            ) : (
              <span>
                {t("woCost.settlement.reversed", {
                  date: formatDate(settlement.reversed_at),
                  reason: settlement.reversal_reason ?? "—",
                })}
              </span>
            )}
          </div>
        ) : b.status_code === "closed" ? null : (
          <p className="text-xs text-text-muted">{t("woCost.settlement.onClose")}</p>
        )}

        <button
          type="button"
          className="flex items-center gap-1 text-xs font-medium text-text-secondary hover:text-text-primary"
          aria-expanded={showLines}
          onClick={() => setShowLines((v) => !v)}
        >
          {showLines ? (
            <ChevronDown className="h-3.5 w-3.5" aria-hidden />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" aria-hidden />
          )}
          {t("woCost.details", {
            labor: b.labor_lines.length,
            parts: b.parts_lines.length,
            purchases: b.purchase_lines.length,
          })}
        </button>
        {showLines ? (
          <div className="space-y-3 text-xs">
            <CostLines
              title={t("woCost.lines.labor")}
              empty={
                view.labor_lines_hidden ? t("woCost.lines.laborHidden") : t("woCost.lines.noLabor")
              }
              head={[
                t("woCost.lines.who"),
                t("woCost.lines.craft"),
                t("woCost.lines.hours"),
                t("woCost.lines.rate"),
                t("woCost.lines.amount"),
              ]}
              rows={b.labor_lines.map((l) => [
                `${l.person_name ?? "—"}${l.is_overtime ? ` · ${t("woCost.lines.overtime")}` : ""}`,
                l.skill_label ?? "—",
                formatNumber(l.hours),
                l.hourly_rate == null
                  ? t("woCost.lines.noRate")
                  : l.rate_source === "rate_card"
                    ? `${money(l.hourly_rate)} · ${t("woCost.lines.rateCard")}`
                    : money(l.hourly_rate),
                money(l.amount),
              ])}
            />
            <CostLines
              title={t("woCost.lines.parts")}
              empty={t("woCost.lines.noParts")}
              head={[
                t("woCost.lines.article"),
                t("woCost.lines.issued"),
                t("woCost.lines.returned"),
                t("woCost.lines.amount"),
              ]}
              rows={b.parts_lines.map((l) => [
                `${l.article_code} — ${l.article_name}${l.stock_backed ? "" : ` · ${t("woCost.lines.nonStock")}`}`,
                formatNumber(l.issued_qty),
                formatNumber(l.returned_qty),
                money(l.amount),
              ])}
            />
            <CostLines
              title={t("woCost.lines.purchases")}
              empty={t("woCost.lines.noPurchases")}
              head={[
                t("woCost.lines.po"),
                t("woCost.lines.item"),
                t("woCost.lines.ordered"),
                t("woCost.lines.received"),
                t("woCost.lines.open"),
              ]}
              rows={b.purchase_lines.map((l) => [
                `${l.po_number}${l.supplier_name ? ` · ${l.supplier_name}` : ""}`,
                `${l.article_name} (${t(`woCost.lines.demand.${l.demand_source_type}`)})`,
                money(l.ordered_amount),
                money(l.received_amount),
                l.open_commitment > 0 ? money(l.open_commitment) : "—",
              ])}
            />
          </div>
        ) : null}
      </CardContent>
      {estimateOpen ? (
        <WoCostEstimateDialog
          open={estimateOpen}
          onOpenChange={setEstimateOpen}
          workOrderId={woId}
          workOrderCode={b.work_order_code}
          estimate={b.estimate}
          currentCostCenterLabel={
            b.cost_center ? `${b.cost_center.code} — ${b.cost_center.name}` : null
          }
          canChangeCostCenter={can(P.FIN_POST)}
          resolvedCostCenterLabel={
            b.cost_center && b.cost_center.source !== "estimate_override"
              ? `${b.cost_center.code} — ${b.cost_center.name}`
              : null
          }
          onSaved={() => void load()}
        />
      ) : null}
    </Card>
  );
}

function CostLines({
  title,
  empty,
  head,
  rows,
}: {
  title: string;
  empty: string;
  head: string[];
  rows: string[][];
}) {
  return (
    <section aria-label={title}>
      <h5 className="mb-1 font-semibold text-text-secondary">{title}</h5>
      {rows.length === 0 ? (
        <p className="text-text-muted">{empty}</p>
      ) : (
        <table className="w-full">
          <thead>
            <tr className="text-text-muted">
              {head.map((h, i) => (
                <th
                  key={h}
                  scope="col"
                  className={cn("py-0.5 font-medium", i === 0 ? "text-start" : "text-end")}
                >
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((r, ri) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: rows of one render, no identity
              <tr key={ri} className="border-t border-surface-border/60">
                {r.map((cell, i) => (
                  <td
                    // biome-ignore lint/suspicious/noArrayIndexKey: fixed column order
                    key={i}
                    className={cn(
                      "py-0.5",
                      i === 0 ? "text-start" : "text-end font-mono tabular-nums",
                    )}
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
