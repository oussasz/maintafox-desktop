/**
 * Side-by-side comparison of two budget versions of the year (L7-11): per cost centre and
 * category, the amount of each version and the gap between them.
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { compareBudgetLines, roundCents } from "@/components/finance/budget-compare";
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
const NUM = "px-3 py-2 text-end font-mono tabular-nums";

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
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    setLeftId((cur) => cur ?? referenceVersionId);
  }, [referenceVersionId]);

  useEffect(() => {
    if (leftId == null || rightId == null || leftId === rightId) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setError(null);
    setLoading(true);
    // Never show the previous pair's amounts under the new version names.
    setLeftLines([]);
    setRightLines([]);
    void Promise.all([
      listBudgetLines({ budget_version_id: leftId }),
      listBudgetLines({ budget_version_id: rightId }),
    ])
      .then(([l, r]) => {
        if (cancelled) return;
        setLeftLines(l);
        setRightLines(r);
        setLoading(false);
      })
      .catch((err) => {
        if (cancelled) return;
        setLeftLines([]);
        setRightLines([]);
        setError(err);
        setLoading(false);
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
  const currencyLeft = left?.currency_code ?? "";
  const currencyRight = right?.currency_code ?? "";
  // Amounts in two currencies can sit side by side but are never added or subtracted.
  const sameCurrency = currencyLeft === currencyRight;
  const sameVersion = leftId != null && leftId === rightId;
  const totalLeft = roundCents(rows.reduce((s, r) => s + r.left, 0));
  const totalRight = roundCents(rows.reduce((s, r) => s + r.right, 0));
  const totalGap = roundCents(totalRight - totalLeft);
  const untitled = t("planning.untitled");

  const gap = (value: number): string =>
    !sameCurrency || value === 0
      ? "—"
      : `${value > 0 ? "+" : ""}${formatCurrency(value, currencyLeft)}`;

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
      {sameVersion ? (
        <p role="status" className="text-xs text-text-muted">
          {t("compare.sameVersion")}
        </p>
      ) : null}
      {loading ? (
        <p role="status" className="text-xs text-text-muted">
          {t("compare.loading")}
        </p>
      ) : null}
      {left && right && !sameVersion && !sameCurrency ? (
        <p role="status" className="text-xs text-text-muted">
          {t("compare.differentCurrencies")}
        </p>
      ) : null}
      {error ? <ErrorState error={error} title={t("planning.loadError")} /> : null}
      {left && right && !sameVersion && !error && !loading ? (
        <div className="overflow-x-auto rounded-lg border border-surface-border">
          <table className="w-full text-sm">
            <caption className="sr-only">{t("compare.title")}</caption>
            <thead className="bg-surface-2 text-xs uppercase tracking-wide text-text-secondary">
              <tr>
                <th scope="col" className={TH}>
                  {t("planning.col.costCenter")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.category")}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {versionName(left, untitled)}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {versionName(right, untitled)}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {t("compare.gap")}
                </th>
              </tr>
            </thead>
            <tbody>
              {rows.length === 0 ? (
                <tr>
                  <td className="px-3 py-3 text-text-muted" colSpan={5}>
                    {t("compare.noLines")}
                  </td>
                </tr>
              ) : null}
              {rows.map((r) => (
                <tr key={r.key} className="border-t border-surface-border">
                  <td className="px-3 py-2">
                    {r.costCenterCode}
                    <span className="ms-2 text-xs text-text-muted">{r.costCenterName}</span>
                  </td>
                  <td className="px-3 py-2">{categoryLabel(r.bucket)}</td>
                  <td className={NUM}>{formatCurrency(r.left, currencyLeft)}</td>
                  <td className={NUM}>{formatCurrency(r.right, currencyRight)}</td>
                  <td className={NUM}>{gap(r.delta)}</td>
                </tr>
              ))}
              <tr className="border-t border-surface-border bg-surface-2 font-semibold">
                <td className="px-3 py-2" colSpan={2}>
                  {t("compare.total")}
                </td>
                <td className={NUM}>{formatCurrency(totalLeft, currencyLeft)}</td>
                <td className={NUM}>{formatCurrency(totalRight, currencyRight)}</td>
                <td className={NUM}>{gap(totalGap)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}
