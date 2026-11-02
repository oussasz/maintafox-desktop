/**
 * Side-by-side comparison of two budget versions of the year (L7-11): per cost centre and
 * category, the amount of each version and the gap between them.
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { compareBudgetLines } from "@/components/finance/budget-compare";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
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
import { listBudgetLines } from "@/services/budget-service";
import type { BudgetLine, BudgetVersion } from "@shared/ipc-types";

const TH = "px-3 py-2 text-start font-semibold";

function versionName(v: BudgetVersion, untitled: string): string {
  return `${v.title ?? untitled} · v${v.version_no}`;
}

export function BudgetVersionCompare({
  versions,
  referenceVersionId,
}: {
  versions: BudgetVersion[];
  referenceVersionId: number | null;
}) {
  const { t } = useTranslation("budget");
  const { categoryLabel } = useFinanceCatalogs();
  const { formatCurrency } = useFormatters();
  const [leftId, setLeftId] = useState<number | null>(referenceVersionId);
  const [rightId, setRightId] = useState<number | null>(null);
  const [leftLines, setLeftLines] = useState<BudgetLine[]>([]);
  const [rightLines, setRightLines] = useState<BudgetLine[]>([]);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    setLeftId((cur) => cur ?? referenceVersionId);
  }, [referenceVersionId]);

  useEffect(() => {
    if (leftId == null || rightId == null) return;
    let cancelled = false;
    setError(null);
    void Promise.all([
      listBudgetLines({ budget_version_id: leftId }),
      listBudgetLines({ budget_version_id: rightId }),
    ])
      .then(([l, r]) => {
        if (cancelled) return;
        setLeftLines(l);
        setRightLines(r);
      })
      .catch((err) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [leftId, rightId]);

  const rows = useMemo(
    () => (leftId != null && rightId != null ? compareBudgetLines(leftLines, rightLines) : []),
    [leftId, rightId, leftLines, rightLines],
  );
  const left = versions.find((v) => v.id === leftId) ?? null;
  const right = versions.find((v) => v.id === rightId) ?? null;
  const currency = left?.currency_code ?? right?.currency_code ?? "";
  const totalLeft = rows.reduce((s, r) => s + r.left, 0);
  const totalRight = rows.reduce((s, r) => s + r.right, 0);
  const untitled = t("planning.untitled");

  const picker = (label: string, value: number | null, onChange: (id: number) => void) => (
    <div className="space-y-1">
      <Label>{label}</Label>
      <Select value={value == null ? "" : String(value)} onValueChange={(v) => onChange(Number(v))}>
        <SelectTrigger className="h-10 w-64" aria-label={label}>
          <SelectValue placeholder={t("compare.choose")} />
        </SelectTrigger>
        <SelectContent>
          {versions.map((v) => (
            <SelectItem key={v.id} value={String(v.id)}>
              {versionName(v, untitled)} · {t(`versionStatus.${v.status}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );

  return (
    <section aria-labelledby="compare-title" className="space-y-2">
      <div>
        <h3 id="compare-title" className="text-sm font-semibold text-text-primary">
          {t("compare.title")}
        </h3>
        <p className="text-xs text-text-muted">{t("compare.hint")}</p>
      </div>
      <div className="flex flex-wrap gap-4">
        {picker(t("compare.versionA"), leftId, setLeftId)}
        {picker(t("compare.versionB"), rightId, setRightId)}
      </div>
      {error ? <ErrorState error={error} title={t("planning.loadError")} /> : null}
      {leftId != null && rightId != null && !error ? (
        <div className="overflow-x-auto rounded-lg border border-surface-border">
          <table className="w-full text-sm">
            <thead className="bg-surface-2 text-xs uppercase tracking-wide text-text-secondary">
              <tr>
                <th scope="col" className={TH}>
                  {t("planning.col.costCenter")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.category")}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {left ? versionName(left, untitled) : t("compare.versionA")}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {right ? versionName(right, untitled) : t("compare.versionB")}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {t("compare.gap")}
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr
                  key={`${r.costCenterCode}|${r.bucket}`}
                  className="border-t border-surface-border"
                >
                  <td className="px-3 py-2">{r.costCenterCode}</td>
                  <td className="px-3 py-2">{categoryLabel(r.bucket)}</td>
                  <td className="px-3 py-2 text-end font-mono tabular-nums">
                    {formatCurrency(r.left, currency)}
                  </td>
                  <td className="px-3 py-2 text-end font-mono tabular-nums">
                    {formatCurrency(r.right, currency)}
                  </td>
                  <td className="px-3 py-2 text-end font-mono tabular-nums">
                    {r.delta === 0
                      ? "—"
                      : `${r.delta > 0 ? "+" : ""}${formatCurrency(r.delta, currency)}`}
                  </td>
                </tr>
              ))}
              <tr className="border-t border-surface-border bg-surface-2 font-semibold">
                <td className="px-3 py-2" colSpan={2}>
                  {t("compare.total")}
                </td>
                <td className="px-3 py-2 text-end font-mono tabular-nums">
                  {formatCurrency(totalLeft, currency)}
                </td>
                <td className="px-3 py-2 text-end font-mono tabular-nums">
                  {formatCurrency(totalRight, currency)}
                </td>
                <td className="px-3 py-2 text-end font-mono tabular-nums">
                  {totalRight - totalLeft === 0
                    ? "—"
                    : `${totalRight > totalLeft ? "+" : ""}${formatCurrency(totalRight - totalLeft, currency)}`}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}
