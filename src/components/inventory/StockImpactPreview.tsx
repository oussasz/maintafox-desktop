/**
 * Projects stock position after a hypothetical quantity change.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { formatAssetLabel, formatOrDash } from "@/lib/display";
import { cn } from "@/lib/utils";
import { projectInventoryStockImpact } from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { StockImpactProjection } from "@shared/ipc-types";

export interface StockImpactPreviewProps {
  articleId: number;
  warehouseId?: number | null;
  deltaQty: number;
  includeOpenPoQty?: boolean;
  className?: string;
}

function fmtQty(n: number): string {
  return Number.isFinite(n) ? n.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "—";
}

export function StockImpactPreview({
  articleId,
  warehouseId = null,
  deltaQty,
  includeOpenPoQty = true,
  className,
}: StockImpactPreviewProps) {
  const { t } = useTranslation("inventory");
  const [projection, setProjection] = useState<StockImpactProjection | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (articleId <= 0 || !Number.isFinite(deltaQty) || deltaQty === 0) {
      setProjection(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    void projectInventoryStockImpact({
      articleId,
      warehouseId,
      deltaQty,
      includeOpenPoQty,
    })
      .then((row) => {
        if (!cancelled) setProjection(row);
      })
      .catch((err) => {
        if (!cancelled) {
          setProjection(null);
          setError(toErrorMessage(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [articleId, warehouseId, deltaQty, includeOpenPoQty]);

  if (articleId <= 0 || deltaQty === 0) return null;

  return (
    <div className={cn("rounded-md border border-surface-border bg-surface-muted/30 p-3 text-sm", className)}>
      {loading ? (
        <p className="text-xs text-text-muted">{t("stockImpact.loading", { defaultValue: "Projecting…" })}</p>
      ) : error ? (
        <p className="text-xs text-destructive">{error}</p>
      ) : projection ? (
        <div className="space-y-1.5">
          <p className="text-xs font-medium">
            {formatAssetLabel(projection.article_code, projection.article_name)}
            {projection.warehouse_code
              ? ` · ${formatOrDash(projection.warehouse_code)}`
              : ""}
          </p>
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs sm:grid-cols-3">
            <span>
              {t("stockImpact.currentOnHand", { defaultValue: "On hand" })}:{" "}
              <strong className="tabular-nums">{fmtQty(projection.current_on_hand)}</strong>
            </span>
            <span>
              {t("stockImpact.available", { defaultValue: "Available" })}:{" "}
              <strong className="tabular-nums">{fmtQty(projection.available_qty)}</strong>
            </span>
            <span>
              {t("stockImpact.incoming", { defaultValue: "Incoming PO" })}:{" "}
              <strong className="tabular-nums">{fmtQty(projection.incoming_open_po_qty)}</strong>
            </span>
            <span>
              {t("stockImpact.delta", { defaultValue: "Delta" })}:{" "}
              <strong className="tabular-nums">{fmtQty(projection.delta_qty)}</strong>
            </span>
            <span className="col-span-2 sm:col-span-1">
              {t("stockImpact.projected", { defaultValue: "Projected" })}:{" "}
              <strong
                className={cn(
                  "tabular-nums",
                  projection.projected_on_hand < 0 ? "text-status-danger" : "",
                )}
              >
                {fmtQty(projection.projected_on_hand)}
              </strong>
            </span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
