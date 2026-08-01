/**
 * Monthly article consumption chart (issue / adjust-out).
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { BarChart } from "@/components/charts/BarChart";
import { getArticleConsumptionMonthly } from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { ArticleConsumptionMonth } from "@shared/ipc-types";

export interface ArticleConsumptionChartProps {
  articleId: number;
  months?: number;
}

export function ArticleConsumptionChart({ articleId, months = 12 }: ArticleConsumptionChartProps) {
  const { t } = useTranslation("inventory");
  const [rows, setRows] = useState<ArticleConsumptionMonth[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (articleId <= 0) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    void getArticleConsumptionMonthly(articleId, months)
      .then((data) => {
        if (!cancelled) setRows(data);
      })
      .catch((err) => {
        if (!cancelled) {
          setRows([]);
          setError(toErrorMessage(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [articleId, months]);

  const chartData = useMemo(
    () =>
      rows.map((row) => ({
        label: row.year_month.slice(5),
        value: row.issued_qty,
      })),
    [rows],
  );

  return (
    <div className="rounded border border-surface-border p-3">
      <h4 className="mb-2 text-sm font-medium">
        {t("article.consumption.title", { defaultValue: "Monthly consumption" })}
      </h4>
      {loading ? (
        <p className="text-sm text-text-muted">{t("article.consumption.loading", { defaultValue: "Loading…" })}</p>
      ) : error ? (
        <p className="text-sm text-destructive">{error}</p>
      ) : chartData.length === 0 ? (
        <p className="text-sm text-text-muted">{t("article.consumption.empty", { defaultValue: "No consumption in period." })}</p>
      ) : (
        <div className="h-48 w-full">
          <BarChart data={chartData} height={180} />
        </div>
      )}
    </div>
  );
}
