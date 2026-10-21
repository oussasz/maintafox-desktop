/**
 * Replenishment recommendations — transfer vs create PO actions.
 *
 * Rows come from the backend's `evaluate_reorder` (the one low-stock definition, shared with the
 * dashboard tiles and the article list). The reason is the coded trigger, worded here; numbers,
 * money and dates go through the shared formatters; every row is listed (no silent cap).
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel, formatOrDash } from "@/lib/display";
import {
  createInventoryProcurementRequisition,
  evaluateInventoryReplenishment,
} from "@/services/inventory-service";
import type { InventoryReplenishmentRecommendation } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export interface ProcurementRecommendationsPanelProps {
  warehouseId?: number | null;
  onCreateRequisitionPrefill?: (rec: InventoryReplenishmentRecommendation, reason: string) => void;
  onCreated?: () => void | Promise<void>;
  /** Element id, so a dashboard tile can bring the panel into view. */
  id?: string;
}

/** Rows shown before "show all" (all of them stay reachable). */
const FIRST_ROWS = 12;

export function ProcurementRecommendationsPanel({
  warehouseId = null,
  onCreateRequisitionPrefill,
  onCreated,
  id,
}: ProcurementRecommendationsPanelProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatCurrency, formatDate } = useFormatters();
  const [rows, setRows] = useState<InventoryReplenishmentRecommendation[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [showAll, setShowAll] = useState(false);
  const [creatingId, setCreatingId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await evaluateInventoryReplenishment(warehouseId));
    } catch (err) {
      setError(err);
      setRows([]);
    } finally {
      setLoading(false);
    }
  }, [warehouseId]);

  useEffect(() => {
    void load();
  }, [load]);

  /** Reason in the UI language, from the coded trigger (`reorder_point` / `min_stock`). */
  const reasonOf = (rec: InventoryReplenishmentRecommendation) =>
    t(`procurement.recommendations.trigger.${rec.trigger_type}`, {
      defaultValue: t("procurement.recommendations.trigger.other"),
      warehouse: rec.warehouse_code,
    });

  const createRequisitionFromRec = async (rec: InventoryReplenishmentRecommendation) => {
    const key = `${rec.article_id}-${rec.warehouse_id}`;
    setCreatingId(key);
    try {
      if (onCreateRequisitionPrefill) {
        onCreateRequisitionPrefill(rec, reasonOf(rec));
      } else {
        await createInventoryProcurementRequisition({
          article_id: rec.article_id,
          preferred_location_id: null,
          requested_qty: rec.suggested_reorder_qty,
          demand_source_type: "REORDER",
          demand_source_ref: rec.warehouse_code,
          source_reservation_id: null,
          source_reorder_trigger: rec.trigger_type,
          purchase_priority: "NORMAL",
          reason: reasonOf(rec),
          actor_id: null,
        });
        await onCreated?.();
        await load();
      }
    } catch {
      // The global IPC toast explains the refusal.
    } finally {
      setCreatingId(null);
    }
  };

  if (!loading && rows.length === 0 && error == null) return null;

  const visible = showAll ? rows : rows.slice(0, FIRST_ROWS);

  return (
    <section
      id={id}
      className="mb-4 rounded-md border border-surface-border bg-surface-1 p-3"
      aria-label={t("procurement.recommendations.title")}
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">
          {t("procurement.recommendations.title")}
          {rows.length > 0 ? (
            <span className="ml-2 text-xs font-normal text-text-muted">
              {formatNumber(rows.length)}
            </span>
          ) : null}
        </h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-9"
          disabled={loading}
          onClick={() => void load()}
        >
          {t("procurement.recommendations.refresh")}
        </Button>
      </div>
      {error != null ? (
        <ErrorState
          error={error}
          title={t("procurement.recommendations.loadError")}
          onRetry={() => void load()}
          retrying={loading}
          className="py-4"
        />
      ) : loading ? (
        <p className="text-sm text-text-muted">{t("procurement.recommendations.loading")}</p>
      ) : (
        <div className="space-y-2">
          {visible.map((rec) => {
            const key = `${rec.article_id}-${rec.warehouse_id}`;
            const isTransfer = rec.suggestion_type === "TRANSFER";
            return (
              <div
                key={key}
                className="flex flex-col gap-2 rounded border border-surface-border p-2.5 text-sm sm:flex-row sm:items-start sm:justify-between"
              >
                <div className="min-w-0 space-y-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-medium">
                      {formatAssetLabel(rec.article_code, rec.article_name)}
                    </span>
                    <Badge
                      variant={isTransfer ? "secondary" : "outline"}
                      className="h-5 text-[10px]"
                    >
                      {isTransfer
                        ? t("procurement.recommendations.transfer")
                        : t("procurement.recommendations.purchase")}
                    </Badge>
                    <span className="text-xs text-text-muted">{rec.warehouse_code}</span>
                  </div>
                  <p className="text-xs text-text-muted">
                    {reasonOf(rec)} · {t("procurement.recommendations.suggestedQty")}:{" "}
                    <strong className="tabular-nums text-text-primary">
                      {formatNumber(rec.suggested_reorder_qty)}
                    </strong>
                    {" · "}
                    {t("procurement.recommendations.available")}:{" "}
                    <strong className="tabular-nums text-text-primary">
                      {formatNumber(rec.available_qty)}
                    </strong>
                  </p>
                  <p className="text-xs text-text-muted">
                    {t("procurement.recommendations.supplier")}:{" "}
                    {formatOrDash(rec.suggested_supplier_name)} ·{" "}
                    {t("procurement.recommendations.lead")}:{" "}
                    {rec.expected_arrival ? formatDate(rec.expected_arrival) : "—"} ·{" "}
                    {t("procurement.recommendations.cost")}:{" "}
                    {rec.estimated_cost != null ? formatCurrency(rec.estimated_cost) : "—"}
                  </p>
                  {isTransfer && rec.transfer_options.length > 0 ? (
                    <p className="text-xs text-text-muted">
                      {t("procurement.recommendations.surplusFrom")}:{" "}
                      {rec.transfer_options
                        .map((opt) => `${opt.warehouse_code} (${formatNumber(opt.available_qty)})`)
                        .join(", ")}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  {isTransfer ? (
                    <Badge variant="outline" className="h-7 px-2 text-xs font-normal">
                      {t("procurement.recommendations.transferHint")}
                    </Badge>
                  ) : null}
                  <PermissionGate permission={P.INV_PROCURE}>
                    <Button
                      type="button"
                      size="sm"
                      className="h-9"
                      variant={isTransfer ? "outline" : "default"}
                      disabled={creatingId === key}
                      onClick={() => void createRequisitionFromRec(rec)}
                    >
                      {creatingId === key
                        ? t("procurement.recommendations.creating")
                        : t("procurement.recommendations.createRequisition")}
                    </Button>
                  </PermissionGate>
                </div>
              </div>
            );
          })}
          {rows.length > FIRST_ROWS ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-9"
              onClick={() => setShowAll((v) => !v)}
            >
              {showAll
                ? t("procurement.recommendations.showFewer")
                : t("procurement.recommendations.showAll", { count: rows.length })}
            </Button>
          ) : null}
        </div>
      )}
    </section>
  );
}
