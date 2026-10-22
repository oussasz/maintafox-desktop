/**
 * Budget cockpit: budget vs committed vs actual vs forecast (estimate at completion) by cost
 * centre and category for one fiscal year, with the monthly burn and the documents behind every
 * figure. Figures come from `finance::cockpit` — actuals are OT settlements and manual postings,
 * commitments are live purchase orders/requisitions raised for OTs.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { AlertTriangle, Download, Scale, TrendingUp } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { CockpitDrilldownSheet } from "@/components/finance/CockpitDrilldownSheet";
import { forecastTone } from "@/components/finance/finance-tone";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { type Tone, mfTone } from "@/design-system/tone";
import { saveBlobWithDialog } from "@/export/download";
import { ExportCancelledError } from "@/export/types";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type BudgetCockpit,
  type BudgetCockpitRow,
  getBudgetCockpit,
} from "@/services/finance-cost-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

export type CockpitTarget = "planning" | "settlements";

export interface BudgetCockpitViewProps {
  fiscalYear: number;
  budgetVersionId: number | null;
  reloadToken?: number;
  /** Opens another tab of the Budget page (setup, pending settlements). */
  onNavigate: (target: CockpitTarget) => void;
  /** Opens a variance review on a row (cost centre × category). */
  onAnalyseVariance: (row: BudgetCockpitRow, cockpit: BudgetCockpit) => void;
}

/**
 * CSV of the cockpit rows (semicolon separated). Figures use the app language's decimal
 * separator without grouping (a French spreadsheet reads "200,00" as a number, "200.00" as text).
 */
export function cockpitCsv(
  cockpit: BudgetCockpit,
  header: string[],
  bucketLabel: (b: string) => string,
  formatAmount: (value: number) => string,
): string {
  const esc = (v: string) => (/[;"\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  const lines = [header.map(esc).join(";")];
  for (const r of cockpit.rows) {
    lines.push(
      [
        r.cost_center_code,
        r.cost_center_name,
        bucketLabel(r.bucket),
        formatAmount(r.budget),
        formatAmount(r.committed),
        formatAmount(r.actual),
        formatAmount(r.forecast),
        formatAmount(r.variance),
      ]
        .map(esc)
        .join(";"),
    );
  }
  return lines.join("\n");
}

/** A cockpit line with its category name in the interface language. */
type CockpitTableRow = BudgetCockpitRow & { bucket_label: string };

export function BudgetCockpitView({
  fiscalYear,
  budgetVersionId,
  reloadToken = 0,
  onNavigate,
  onAnalyseVariance,
}: BudgetCockpitViewProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatPercent, formatDecimalInput } = useFormatters();
  const { can } = usePermissions();
  const [cockpit, setCockpit] = useState<BudgetCockpit | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [drillRow, setDrillRow] = useState<BudgetCockpitRow | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setCockpit(
        await getBudgetCockpit({ fiscal_year: fiscalYear, budget_version_id: budgetVersionId }),
      );
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [fiscalYear, budgetVersionId]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken forces a refresh
  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const currency = cockpit?.currency_code ?? undefined;
  const money = useCallback((v: number) => formatCurrency(v, currency), [formatCurrency, currency]);
  const { categoryLabel: bucketLabel, reasonLabel } = useFinanceCatalogs();

  // The parent passes a new callback on every render; the columns call the latest one without
  // being rebuilt for it (the ref is updated after render, never during it).
  const analyseRef = useRef(onAnalyseVariance);
  useEffect(() => {
    analyseRef.current = onAnalyseVariance;
  }, [onAnalyseVariance]);

  // The table caches each row's sort and search values until its data changes, so the category
  // name travels with the rows: when the cost-category catalogue arrives the rows are rebuilt and
  // sorting and search use the names instead of the raw codes. Rows stay mounted (the table keys
  // them by position), so a click landing meanwhile is not lost.
  const tableRows = useMemo<CockpitTableRow[]>(
    () => (cockpit?.rows ?? []).map((r) => ({ ...r, bucket_label: bucketLabel(r.bucket) })),
    [cockpit, bucketLabel],
  );

  const columns = useMemo<ColumnDef<CockpitTableRow>[]>(
    () => [
      {
        id: "cc",
        header: t("cockpit.col.costCenter"),
        accessorFn: (r) => `${r.cost_center_code} ${r.cost_center_name}`,
        cell: ({ row }) => (
          <div className="min-w-0">
            <div className="font-medium text-text-primary">{row.original.cost_center_code}</div>
            <div className="truncate text-xs text-text-muted">{row.original.cost_center_name}</div>
          </div>
        ),
      },
      {
        id: "bucket",
        header: t("cockpit.col.category"),
        accessorFn: (r) => r.bucket_label,
        cell: ({ row }) => row.original.bucket_label,
      },
      ...(["budget", "committed", "actual", "forecast"] as const).map(
        (key): ColumnDef<CockpitTableRow> => ({
          id: key,
          header: () => <span className="block text-end">{t(`cockpit.col.${key}`)}</span>,
          accessorFn: (r) => r[key],
          cell: ({ row }) => {
            const tone =
              key === "forecast"
                ? forecastTone(row.original.budget, row.original.forecast)
                : undefined;
            return (
              <span
                className={cn("block text-end font-mono tabular-nums", tone && mfTone[tone].text)}
              >
                {money(row.original[key])}
              </span>
            );
          },
        }),
      ),
      {
        id: "variance",
        header: () => <span className="block text-end">{t("cockpit.col.variance")}</span>,
        accessorFn: (r) => r.variance,
        cell: ({ row }) => {
          const v = row.original.variance;
          return (
            <span
              className={cn(
                "block text-end font-mono tabular-nums",
                v < -0.005 ? mfTone.danger.text : "text-text-secondary",
              )}
            >
              {money(v)}
            </span>
          );
        },
      },
      {
        id: "consumed",
        header: () => <span className="block text-end">{t("cockpit.col.consumed")}</span>,
        accessorFn: (r) => r.consumed_pct ?? -1,
        cell: ({ row }) => (
          <ConsumptionBar
            pct={row.original.consumed_pct}
            label={t("cockpit.consumedAria")}
            formatPercent={formatPercent}
          />
        ),
      },
      {
        id: "reviews",
        header: t("cockpit.col.reviews"),
        enableSorting: false,
        cell: ({ row }) => {
          const r = row.original;
          const canReview = can(P.FIN_BUDGET) || can(P.FIN_MANAGE);
          return (
            <div className="flex items-center gap-2">
              {r.open_reviews > 0 ? (
                <span className="text-xs text-text-secondary">
                  {t("cockpit.openReviews", { count: r.open_reviews })}
                </span>
              ) : r.latest_driver_code ? (
                <span className="text-xs text-text-muted">{reasonLabel(r.latest_driver_code)}</span>
              ) : null}
              {canReview && r.variance < -0.005 && r.open_reviews === 0 && cockpit ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  onClick={(e) => {
                    e.stopPropagation();
                    analyseRef.current(r, cockpit);
                  }}
                >
                  {t("cockpit.analyse")}
                </Button>
              ) : null}
            </div>
          );
        },
      },
    ],
    // Rebuilding the columns is safe: the table calls cell functions directly and keeps its rows
    // mounted, so a click landing while the catalogues or permissions load is not lost.
    [t, reasonLabel, money, formatPercent, can, cockpit],
  );

  const exportCsv = async () => {
    if (!cockpit) return;
    const csv = cockpitCsv(
      cockpit,
      [
        t("cockpit.col.costCenterCode"),
        t("cockpit.col.costCenterName"),
        t("cockpit.col.category"),
        t("cockpit.col.budget"),
        t("cockpit.col.committed"),
        t("cockpit.col.actual"),
        t("cockpit.col.forecast"),
        t("cockpit.col.variance"),
      ],
      bucketLabel,
      (n) => formatDecimalInput(n, { decimals: 2, fixed: true }),
    );
    try {
      const path = await saveBlobWithDialog(
        new Blob([`﻿${csv}`], { type: "text/csv;charset=utf-8" }),
        `budget-${fiscalYear}.csv`,
        { title: t("cockpit.export"), filters: [{ name: "CSV", extensions: ["csv"] }] },
      );
      pushAppToast({ title: t("cockpit.exported", { path }), variant: "success" });
    } catch (err) {
      if (err instanceof ExportCancelledError) return;
      pushAppToast({
        title: t("cockpit.exportError"),
        description: describeAppError(err).title,
        variant: "destructive",
      });
    }
  };

  if (loading && !cockpit) return <LoadingState label={t("cockpit.loading")} />;
  if (error && !cockpit) {
    return <ErrorState error={error} title={t("cockpit.loadError")} onRetry={() => void load()} />;
  }
  if (!cockpit) return null;

  const setup = cockpit.setup;
  if (setup.cost_centers === 0 || setup.versions_for_year === 0) {
    return (
      <EmptyState
        title={
          setup.cost_centers === 0
            ? t("cockpit.empty.noCostCenterTitle")
            : t("cockpit.empty.noBudgetTitle", { year: fiscalYear })
        }
        description={
          setup.cost_centers === 0
            ? t("cockpit.empty.noCostCenterBody")
            : t("cockpit.empty.noBudgetBody", { year: fiscalYear })
        }
        icon={<Scale className="h-8 w-8 text-text-muted" aria-hidden />}
        {...(can(P.FIN_BUDGET) || can(P.FIN_MANAGE)
          ? { actionLabel: t("cockpit.empty.openPlanning"), onAction: () => onNavigate("planning") }
          : {})}
      />
    );
  }

  const totals = cockpit.totals;
  const consumed = totals.budget > 0 ? (totals.actual / totals.budget) * 100 : null;
  const forecastToneValue = forecastTone(totals.budget, totals.forecast);
  const notApproved =
    cockpit.version != null && !["approved", "frozen"].includes(cockpit.version.status);

  return (
    <div className="space-y-4">
      {notApproved ? (
        <Notice tone="attention">
          {t("cockpit.notice.notApproved", {
            status: t(`versionStatus.${cockpit.version?.status ?? "draft"}`),
          })}
        </Notice>
      ) : null}
      {cockpit.pending_settlements > 0 ? (
        <Notice
          tone="attention"
          action={
            <Button
              size="sm"
              variant="outline"
              className="h-7 text-xs"
              onClick={() => onNavigate("settlements")}
            >
              {t("cockpit.notice.openPending")}
            </Button>
          }
        >
          {t("cockpit.notice.pending", {
            count: cockpit.pending_settlements,
            amount: money(cockpit.pending_settlement_amount),
          })}
        </Notice>
      ) : null}
      {cockpit.unassigned_commitments > 0 ? (
        <Notice tone="warning">
          {t("cockpit.notice.unassignedCommitments", {
            amount: money(cockpit.unassigned_commitments),
          })}
        </Notice>
      ) : null}

      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
        <KpiTile
          label={t("kpi.budget")}
          value={money(totals.budget)}
          hint={t("kpi.budgetHint", { year: fiscalYear })}
        />
        <KpiTile
          label={t("kpi.committed")}
          value={money(totals.committed)}
          hint={t("kpi.committedHint")}
        />
        <KpiTile label={t("kpi.actual")} value={money(totals.actual)} hint={t("kpi.actualHint")} />
        <KpiTile
          label={t("kpi.forecast")}
          value={money(totals.forecast)}
          hint={t("kpi.forecastHint")}
          icon={TrendingUp}
          {...(forecastToneValue ? { tone: forecastToneValue } : {})}
        />
        <KpiTile
          label={t("kpi.variance")}
          value={money(totals.variance)}
          hint={totals.variance < 0 ? t("kpi.overrun") : t("kpi.underrun")}
          {...(totals.variance < -0.005 ? { tone: "danger" as const } : {})}
        />
        <KpiTile
          label={t("kpi.consumed")}
          value={consumed == null ? "—" : formatPercent(consumed)}
          hint={
            cockpit.as_of_month
              ? t("kpi.consumedHintMonth", {
                  month: t(
                    `month.${cockpit.months[cockpit.as_of_month - 1]?.calendar_month ?? cockpit.as_of_month}`,
                  ),
                })
              : t("kpi.consumedHint")
          }
        />
      </div>

      <MonthlyBurn cockpit={cockpit} money={money} />

      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("cockpit.tableTitle")}</h3>
        {/* Exporting budget figures is a reporting right (fin.report). */}
        {can(P.FIN_REPORT) ? (
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => void exportCsv()}
            disabled={cockpit.rows.length === 0}
          >
            <Download className="h-3.5 w-3.5" aria-hidden />
            {t("cockpit.export")}
          </Button>
        ) : null}
      </div>
      {cockpit.rows.length === 0 ? (
        <EmptyState
          title={t("cockpit.empty.noRowsTitle")}
          description={t("cockpit.empty.noRowsBody")}
        />
      ) : (
        <DataTable
          columns={columns}
          data={tableRows}
          pageSize={50}
          onRowClick={(r) => setDrillRow(r)}
        />
      )}
      <p className="text-xs text-text-muted">{t("cockpit.method")}</p>

      <CockpitDrilldownSheet
        row={drillRow}
        fiscalYear={fiscalYear}
        budgetVersionId={cockpit.version?.id ?? null}
        currency={cockpit.currency_code}
        onClose={() => setDrillRow(null)}
      />
    </div>
  );
}

function Notice({
  tone,
  children,
  action,
}: {
  tone: Tone;
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex flex-wrap items-center justify-between gap-2 rounded-md border px-3 py-2 text-sm",
        mfTone[tone].panel,
      )}
      role="status"
    >
      <span className="flex items-start gap-2">
        <AlertTriangle className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone[tone].text)} aria-hidden />
        {children}
      </span>
      {action}
    </div>
  );
}

function ConsumptionBar({
  pct,
  label,
  formatPercent,
}: {
  pct: number | null;
  label: string;
  formatPercent: (v: number) => string;
}) {
  if (pct == null) return <span className="block text-end text-text-muted">—</span>;
  const tone: Tone | undefined = pct > 100 ? "danger" : pct >= 90 ? "warning" : undefined;
  return (
    <div className="flex items-center justify-end gap-2">
      <div
        className="h-1.5 w-16 overflow-hidden rounded-full bg-surface-3"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(Math.min(pct, 100))}
      >
        <div
          className={cn("h-full", tone ? mfTone[tone].bar : "bg-text-secondary")}
          style={{ width: `${Math.min(pct, 100)}%` }}
        />
      </div>
      <span
        className={cn("w-12 text-end font-mono text-xs tabular-nums", tone && mfTone[tone].text)}
      >
        {formatPercent(pct)}
      </span>
    </div>
  );
}

/** Budget vs actual per month (bars) with the cumulative position. */
function MonthlyBurn({ cockpit, money }: { cockpit: BudgetCockpit; money: (v: number) => string }) {
  const { t } = useTranslation("budget");
  const max = Math.max(1, ...cockpit.months.map((m) => Math.max(m.budget, m.actual)));
  // Shown on hover and read by screen readers: the amounts behind the two bars of a month.
  const monthText = (m: BudgetCockpit["months"][number]) =>
    t("burn.monthAria", {
      month: t(`month.${m.calendar_month}`),
      budget: money(m.budget),
      actual: money(m.actual),
    });
  return (
    <section
      aria-labelledby="burn-title"
      className="rounded-lg border border-surface-border bg-surface-1 p-3"
    >
      <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
        <h3 id="burn-title" className="text-sm font-semibold text-text-primary">
          {t("burn.title")}
        </h3>
        <div className="flex items-center gap-3 text-xs text-text-muted">
          <span className="flex items-center gap-1">
            <span className="h-2 w-3 rounded-sm bg-surface-3" aria-hidden /> {t("burn.budget")}
          </span>
          <span className="flex items-center gap-1">
            <span className={cn("h-2 w-3 rounded-sm", mfTone.progress.bar)} aria-hidden />{" "}
            {t("burn.actual")}
          </span>
        </div>
      </div>
      <ol className="grid grid-cols-12 items-end gap-1" style={{ height: 120 }}>
        {cockpit.months.map((m) => {
          const over = m.actual > m.budget + 0.005 && m.budget > 0;
          return (
            <li
              key={m.period_month}
              className="flex h-full flex-col justify-end"
              title={monthText(m)}
              aria-label={monthText(m)}
            >
              <div className="flex flex-1 items-end justify-center gap-0.5">
                <div
                  className="w-2 rounded-t bg-surface-3"
                  style={{ height: `${(m.budget / max) * 100}%` }}
                />
                <div
                  className={cn("w-2 rounded-t", over ? mfTone.danger.bar : mfTone.progress.bar)}
                  style={{ height: `${(Math.max(m.actual, 0) / max) * 100}%` }}
                />
              </div>
              <span
                className={cn(
                  "mt-1 text-center text-[10px] text-text-muted",
                  cockpit.as_of_month === m.period_month && "font-semibold text-text-primary",
                )}
              >
                {t(`monthShort.${m.calendar_month}`)}
              </span>
            </li>
          );
        })}
      </ol>
      {cockpit.months.length > 0 ? (
        <p className="mt-2 text-xs text-text-muted">
          {t("burn.cumulative", {
            actual: money(cockpit.months[(cockpit.as_of_month ?? 12) - 1]?.cumulative_actual ?? 0),
            budget: money(cockpit.months[(cockpit.as_of_month ?? 12) - 1]?.cumulative_budget ?? 0),
          })}
        </p>
      ) : null}
    </section>
  );
}
