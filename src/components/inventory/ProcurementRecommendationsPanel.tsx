/**
 * Replenishment recommendations — transfer vs create PO actions.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { formatAssetLabel, formatOrDash } from "@/lib/display";
import {
  createInventoryProcurementRequisition,
  evaluateInventoryReplenishment,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { InventoryReplenishmentRecommendation } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export interface ProcurementRecommendationsPanelProps {
  warehouseId?: number | null;
  onCreateRequisitionPrefill?: (rec: InventoryReplenishmentRecommendation) => void;
  onCreated?: () => void | Promise<void>;
}

function fmtMoney(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function fmtDate(v: string | null | undefined): string {
  if (!v) return "—";
  try {
    return new Date(v).toLocaleDateString();
  } catch {
    return v;
  }
}

export function ProcurementRecommendationsPanel({
  warehouseId = null,
  onCreateRequisitionPrefill,
  onCreated,
}: ProcurementRecommendationsPanelProps) {
  const { t } = useTranslation("inventory");
  const [rows, setRows] = useState<InventoryReplenishmentRecommendation[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creatingId, setCreatingId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await evaluateInventoryReplenishment(warehouseId);
      setRows(data);
    } catch (err) {
      setError(toErrorMessage(err));
      setRows([]);
    } finally {
      setLoading(false);
    }
  }, [warehouseId]);

  useEffect(() => {
    void load();
  }, [load]);

  const createRequisitionFromRec = async (rec: InventoryReplenishmentRecommendation) => {
    const key = `${rec.article_id}-${rec.warehouse_id}`;
    setCreatingId(key);
    setError(null);
    try {
      if (onCreateRequisitionPrefill) {
        onCreateRequisitionPrefill(rec);
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
          reason: rec.reason,
          actor_id: null,
        });
        await onCreated?.();
        await load();
      }
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setCreatingId(null);
    }
  };

  if (!loading && rows.length === 0 && !error) return null;

  return (
    <div className="mb-4 rounded-md border p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">
          {t("procurement.recommendations.title", {
            defaultValue: "Replenishment recommendations",
          })}
        </h3>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={loading}
          onClick={() => void load()}
        >
          {t("procurement.recommendations.refresh", { defaultValue: "Refresh" })}
        </Button>
      </div>
      {error ? <p className="mb-2 text-sm text-destructive">{error}</p> : null}
      {loading ? (
        <p className="text-sm text-text-muted">
          {t("procurement.recommendations.loading", { defaultValue: "Loading…" })}
        </p>
      ) : (
        <div className="space-y-2">
          {rows.slice(0, 12).map((rec) => {
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
                        ? t("procurement.recommendations.transfer", { defaultValue: "Transfer" })
                        : t("procurement.recommendations.purchase", { defaultValue: "Purchase" })}
                    </Badge>
                    <span className="text-xs text-text-muted">{rec.warehouse_code}</span>
                  </div>
                  <p className="text-xs text-text-muted">
                    {formatOrDash(rec.reason)} ·{" "}
                    {t("procurement.recommendations.suggestedQty", {
                      defaultValue: "Suggested qty",
                    })}
                    :{" "}
                    <strong className="text-foreground tabular-nums">
                      {rec.suggested_reorder_qty}
                    </strong>
                    {" · "}
                    {t("procurement.recommendations.available", {
                      defaultValue: "Available",
                    })}
                    : <strong className="text-foreground tabular-nums">{rec.available_qty}</strong>
                  </p>
                  <p className="text-xs text-text-muted">
                    {t("procurement.recommendations.supplier", {
                      defaultValue: "Preferred supplier",
                    })}
                    : {formatOrDash(rec.suggested_supplier_name)} ·{" "}
                    {t("procurement.recommendations.lead", { defaultValue: "ETA" })}:{" "}
                    {fmtDate(rec.expected_arrival)} ·{" "}
                    {t("procurement.recommendations.cost", { defaultValue: "Est. cost" })}:{" "}
                    {fmtMoney(rec.estimated_cost)}
                  </p>
                  {isTransfer && rec.transfer_options.length > 0 ? (
                    <p className="text-xs text-text-muted">
                      {t("procurement.recommendations.surplusFrom", {
                        defaultValue: "Surplus from",
                      })}
                      :{" "}
                      {rec.transfer_options
                        .slice(0, 3)
                        .map((opt) => `${opt.warehouse_code} (${opt.available_qty})`)
                        .join(", ")}
                    </p>
                  ) : null}
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  {isTransfer ? (
                    <Badge variant="outline" className="h-7 px-2 text-xs font-normal">
                      {t("procurement.recommendations.transferHint", {
                        defaultValue: "Prefer internal transfer",
                      })}
                    </Badge>
                  ) : null}
                  <PermissionGate permission={P.INV_PROCURE}>
                    <Button
                      type="button"
                      size="sm"
                      variant={isTransfer ? "outline" : "default"}
                      disabled={creatingId === key}
                      onClick={() => void createRequisitionFromRec(rec)}
                    >
                      {creatingId === key
                        ? t("procurement.recommendations.creating", { defaultValue: "Creating…" })
                        : t("procurement.recommendations.createRequisition", {
                            defaultValue: "Create requisition",
                          })}
                    </Button>
                  </PermissionGate>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
