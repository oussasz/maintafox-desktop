/**
 * Maintenance cost history per asset against its replacement asset value (RAV): lifetime and
 * 12-month cost, split by category, corrective vs preventive, the mean cost of a corrective OT
 * (failure cost used by RAMS cost-based intervals) and the SMRP cost/RAV ratio. An asset whose
 * lifetime maintenance cost has reached its replacement value is flagged for a repair-or-replace
 * review. Figures are the settled costs of closed OTs (`finance::asset_costs`).
 */
import type { ColumnDef } from "@tanstack/react-table";
import { Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { KpiTile } from "@/components/metrics/KpiTile";
import { ExportCsvButton } from "@/components/ui/ExportCsvButton";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { cn } from "@/lib/utils";
import {
  type AssetCostHistory,
  type AssetCostRow,
  listAssetCostHistory,
} from "@/services/finance-cost-service";
import { P } from "@shared/rbac/permissions.generated";

export interface AssetCostPanelProps {
  reloadToken?: number;
}

export function AssetCostPanel({ reloadToken = 0 }: AssetCostPanelProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, formatPercent: percent, formatDate } = useFormatters();
  // A ratio rounded to one decimal would read "0 %" for a real cost: say it is under 0,1 %.
  const formatPercent = useCallback(
    (v: number) => (v > 0 && v < 0.05 ? `< ${percent(0.1)}` : percent(v)),
    [percent],
  );
  const { categoryLabel } = useFinanceCatalogs();
  const [search, setSearch] = useState("");
  const debounced = useDebouncedValue(search, 300);
  const [centre, setCentre] = useState("");
  const [data, setData] = useState<AssetCostHistory | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await listAssetCostHistory({ search: debounced.trim() || null, limit: 500 }));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [debounced]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reloadToken forces a refresh
  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const currency = data?.currency_code ?? undefined;
  const money = useCallback(
    (v: number | null) => (v == null ? "—" : formatCurrency(v, currency)),
    [formatCurrency, currency],
  );

  const centres = useMemo(
    () =>
      [
        ...new Set(
          (data?.rows ?? []).map((r) => r.cost_center_code).filter((c): c is string => !!c),
        ),
      ].sort(),
    [data],
  );
  // A centre that a new search no longer returns must not leave an empty, unexplained table.
  const activeCentre = centres.includes(centre) ? centre : "";
  const rows = useMemo(
    () => (data?.rows ?? []).filter((r) => !activeCentre || r.cost_center_code === activeCentre),
    [data, activeCentre],
  );
  // Cost per production line (cost centre of the equipment) and per work type, from the same rows.
  const byCentre = useMemo(() => {
    const acc = new Map<string, { life: number; m12: number; corr: number; prev: number }>();
    for (const r of rows) {
      const key = r.cost_center_code ?? "";
      const e = acc.get(key) ?? { life: 0, m12: 0, corr: 0, prev: 0 };
      e.life += r.lifetime_cost;
      e.m12 += r.cost_12m;
      e.corr += r.corrective_cost;
      e.prev += r.preventive_cost;
      acc.set(key, e);
    }
    return [...acc.entries()].sort((a, b) => b[1].life - a[1].life);
  }, [rows]);
  const totals = useMemo(
    () =>
      rows.reduce(
        (a, r) => ({
          life: a.life + r.lifetime_cost,
          m12: a.m12 + r.cost_12m,
          corr: a.corr + r.corrective_cost,
          prev: a.prev + r.preventive_cost,
        }),
        { life: 0, m12: 0, corr: 0, prev: 0 },
      ),
    [rows],
  );
  const exportHeader = useMemo(
    () => [
      t("assets.col.asset"),
      t("assets.export.name"),
      t("assets.export.line"),
      t("assets.col.rav"),
      t("assets.col.lifetime"),
      t("assets.col.cost12m"),
      t("assets.col.corrective"),
      t("assets.export.preventive"),
      t("assets.export.other"),
      t("assets.col.lastClosed"),
    ],
    [t],
  );
  const getExportRows = useCallback(
    () =>
      rows.map((r) => [
        r.equipment_code,
        r.equipment_name,
        r.cost_center_code ?? "",
        r.replacement_value,
        r.lifetime_cost,
        r.cost_12m,
        r.corrective_cost,
        r.preventive_cost,
        Math.max(0, r.lifetime_cost - r.corrective_cost - r.preventive_cost),
        r.last_closed_at ? formatDate(r.last_closed_at) : "",
      ]),
    [rows, formatDate],
  );

  const columns = useMemo<ColumnDef<AssetCostRow>[]>(
    () => [
      {
        id: "asset",
        header: t("assets.col.asset"),
        accessorFn: (r) => `${r.equipment_code} ${r.equipment_name}`,
        cell: ({ row }) => (
          <div className="min-w-0">
            <LinkedEntityBadge
              entity="asset"
              code={row.original.equipment_code}
              entityId={row.original.equipment_id}
              title={row.original.equipment_name}
            />
            <div className="truncate text-xs text-text-muted">{row.original.equipment_name}</div>
          </div>
        ),
      },
      {
        id: "rav",
        header: () => <span className="block text-end">{t("assets.col.rav")}</span>,
        accessorFn: (r) => r.replacement_value ?? -1,
        cell: ({ row }) => (
          <span className="block text-end font-mono tabular-nums">
            {money(row.original.replacement_value)}
          </span>
        ),
      },
      {
        id: "lifetime",
        header: () => <span className="block text-end">{t("assets.col.lifetime")}</span>,
        accessorFn: (r) => r.lifetime_cost,
        cell: ({ row }) => {
          const r = row.original;
          return (
            <div className="text-end">
              <div className="font-mono tabular-nums">{money(r.lifetime_cost)}</div>
              <div className="text-[11px] text-text-muted">
                {(
                  [
                    ["labor", r.lifetime_labor],
                    ["parts", r.lifetime_parts],
                    ["services", r.lifetime_services],
                  ] as const
                )
                  .map(([code, amount]) => `${categoryLabel(code)} ${money(amount)}`)
                  .join(" · ")}
              </div>
            </div>
          );
        },
      },
      {
        id: "lifetimeRav",
        header: () => <span className="block text-end">{t("assets.col.lifetimeRav")}</span>,
        accessorFn: (r) => r.lifetime_to_rav_pct ?? -1,
        cell: ({ row }) => {
          const v = row.original.lifetime_to_rav_pct;
          if (v == null) return <span className="block text-end text-text-muted">—</span>;
          return (
            <div className="flex flex-col items-end gap-0.5">
              <span className={cn("font-mono tabular-nums", v >= 100 && mfTone.danger.text)}>
                {formatPercent(v)}
              </span>
              {v >= 100 ? <StatusChip tone="danger" label={t("assets.replaceReview")} /> : null}
            </div>
          );
        },
      },
      {
        id: "cost12m",
        header: () => <span className="block text-end">{t("assets.col.cost12m")}</span>,
        accessorFn: (r) => r.cost_12m,
        cell: ({ row }) => (
          <div className="text-end">
            <div className="font-mono tabular-nums">{money(row.original.cost_12m)}</div>
            <div className="text-[11px] text-text-muted">
              {row.original.cost_12m_to_rav_pct == null
                ? t("assets.noRav")
                : t("assets.ofRav", { pct: formatPercent(row.original.cost_12m_to_rav_pct) })}
            </div>
          </div>
        ),
      },
      {
        id: "corrective",
        header: () => <span className="block text-end">{t("assets.col.corrective")}</span>,
        accessorFn: (r) => r.corrective_cost,
        cell: ({ row }) => {
          const r = row.original;
          return (
            <div className="text-end">
              <div className="font-mono tabular-nums">{money(r.corrective_cost)}</div>
              <div className="text-[11px] text-text-muted">
                {t("assets.correctiveCount", {
                  count: r.corrective_work_orders,
                  avg: money(r.avg_corrective_cost),
                })}
              </div>
            </div>
          );
        },
      },
      {
        id: "years",
        header: t("assets.col.years"),
        enableSorting: false,
        cell: ({ row }) => <YearBars years={row.original.by_year} money={money} />,
      },
      {
        id: "last",
        header: t("assets.col.lastClosed"),
        accessorFn: (r) => r.last_closed_at ?? "",
        cell: ({ row }) => formatDate(row.original.last_closed_at),
      },
    ],
    [t, money, formatPercent, formatDate, categoryLabel],
  );

  return (
    <div className="space-y-4">
      <p className="max-w-3xl text-sm text-text-secondary">{t("assets.intro")}</p>
      {data ? (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
          <KpiTile label={t("assets.kpi.rav")} value={money(data.fleet_replacement_value)} />
          <KpiTile
            label={t("assets.kpi.cost12m")}
            value={money(data.fleet_cost_12m)}
            hint={t("assets.kpi.cost12mHint")}
          />
          <KpiTile
            label={t("assets.kpi.costRav")}
            value={
              data.fleet_cost_to_rav_pct == null ? "—" : formatPercent(data.fleet_cost_to_rav_pct)
            }
            hint={t("assets.kpi.costRavHint")}
          />
          <KpiTile
            label={t("assets.kpi.withoutRav")}
            value={String(data.assets_without_rav)}
            hint={t("assets.kpi.withoutRavHint")}
            {...(data.assets_without_rav > 0 ? { tone: "attention" as const } : {})}
          />
        </div>
      ) : null}
      <div className="relative max-w-sm">
        <Search className="absolute left-2 top-2.5 h-4 w-4 text-text-muted" aria-hidden />
        <Input
          aria-label={t("assets.search")}
          placeholder={t("assets.search")}
          className="pl-8"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      {data && data.rows.length > 0 ? (
        <div className="flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2 text-sm text-text-secondary">
            {t("assets.lineFilter")}
            <select
              className="h-10 rounded-md border border-surface-border bg-surface-1 px-2 text-sm"
              value={activeCentre}
              onChange={(e) => setCentre(e.target.value)}
            >
              <option value="">{t("assets.allLines")}</option>
              {centres.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <ExportCsvButton
            fileName="cout-par-equipement.csv"
            header={exportHeader}
            getRows={getExportRows}
            permission={P.FIN_VIEW}
          />
        </div>
      ) : null}
      {data && rows.length > 0 ? (
        <section className="space-y-2" aria-label={t("assets.breakdown.title")}>
          <h3 className="text-sm font-medium text-text-primary">{t("assets.breakdown.title")}</h3>
          <p className="text-xs text-text-muted">{t("assets.breakdown.hint")}</p>
          <div className="overflow-x-auto rounded-md border border-surface-border">
            <table className="w-full text-sm">
              <thead className="bg-surface-2 text-xs text-text-muted">
                <tr>
                  <th className="px-3 py-1.5 text-start">{t("assets.breakdown.line")}</th>
                  <th className="px-3 py-1.5 text-end">{t("assets.col.lifetime")}</th>
                  <th className="px-3 py-1.5 text-end">{t("assets.col.cost12m")}</th>
                  <th className="px-3 py-1.5 text-end">{t("assets.col.corrective")}</th>
                  <th className="px-3 py-1.5 text-end">{t("assets.export.preventive")}</th>
                  <th className="px-3 py-1.5 text-end">{t("assets.export.other")}</th>
                </tr>
              </thead>
              <tbody>
                {byCentre.map(([code, v]) => (
                  <tr key={code || "none"} className="border-t border-surface-border">
                    <td className="px-3 py-1.5">{code || t("assets.breakdown.noLine")}</td>
                    <td className="px-3 py-1.5 text-end font-mono tabular-nums">{money(v.life)}</td>
                    <td className="px-3 py-1.5 text-end font-mono tabular-nums">{money(v.m12)}</td>
                    <td className="px-3 py-1.5 text-end font-mono tabular-nums">{money(v.corr)}</td>
                    <td className="px-3 py-1.5 text-end font-mono tabular-nums">{money(v.prev)}</td>
                    <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                      {money(Math.max(0, v.life - v.corr - v.prev))}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot className="border-t border-surface-border bg-surface-2 text-xs font-medium">
                <tr>
                  <td className="px-3 py-1.5">{t("assets.breakdown.total")}</td>
                  <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                    {money(totals.life)}
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                    {money(totals.m12)}
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                    {money(totals.corr)}
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                    {money(totals.prev)}
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono tabular-nums">
                    {money(Math.max(0, totals.life - totals.corr - totals.prev))}
                  </td>
                </tr>
              </tfoot>
            </table>
          </div>
        </section>
      ) : null}
      {loading && !data ? <LoadingState /> : null}
      {error ? (
        <ErrorState error={error} title={t("assets.loadError")} onRetry={() => void load()} />
      ) : null}
      {data && data.rows.length === 0 ? (
        <EmptyState title={t("assets.emptyTitle")} description={t("assets.emptyBody")} />
      ) : null}
      {data && rows.length > 0 ? <DataTable columns={columns} data={rows} pageSize={50} /> : null}
    </div>
  );
}

function YearBars({
  years,
  money,
}: {
  years: { year: number; amount: number }[];
  money: (v: number) => string;
}) {
  const max = Math.max(1, ...years.map((y) => y.amount));
  return (
    <ol className="flex h-8 items-end gap-0.5">
      {years.map((y) => (
        <li
          key={y.year}
          className="flex h-full w-3 items-end"
          title={`${y.year} : ${money(y.amount)}`}
          aria-label={`${y.year} : ${money(y.amount)}`}
        >
          <span
            className={cn("block w-full rounded-t", mfTone.progress.bar)}
            style={{ height: `${Math.max((y.amount / max) * 100, y.amount > 0 ? 6 : 0)}%` }}
          />
        </li>
      ))}
    </ol>
  );
}
