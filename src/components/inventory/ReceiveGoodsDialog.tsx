/**
 * Goods receipt dialog — multi-line receiving for one purchase order.
 *
 * The whole receipt is posted with a single `receive_inventory_purchase_order_goods`
 * call so the backend creates one GR document with all its lines.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { formatAssetLabel, formatEntityCode } from "@/lib/display";
import {
  listInventoryPurchaseOrderLines,
  receiveInventoryPurchaseOrderGoods,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  PurchaseOrder,
  PurchaseOrderLine,
  ReceivePurchaseOrderLineInput,
  StockLocation,
} from "@shared/ipc-types";

const FULFILLMENT_ACTIONS = ["STOCK_ONLY", "RESERVE_FOR_DEMAND", "ISSUE_TO_DEMAND"] as const;

/** PO statuses the backend accepts for goods receipt posting. */
export const RECEIVABLE_PO_STATUSES = ["APPROVED", "PARTIALLY_RECEIVED"] as const;

export function isReceivablePurchaseOrder(status: string): boolean {
  return (RECEIVABLE_PO_STATUSES as readonly string[]).includes(status);
}

type LineDraft = {
  locationId: number;
  receivedQty: string;
  rejectedQty: string;
  rejectionReason: string;
};

const EMPTY_DRAFT: LineDraft = {
  locationId: 0,
  receivedQty: "",
  rejectedQty: "",
  rejectionReason: "",
};

export interface ReceiveGoodsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Full PO list; only receivable orders are selectable. */
  purchaseOrders: PurchaseOrder[];
  locations: StockLocation[];
  /** Preselected PO (e.g. when opened from the PO workspace). */
  initialPoId?: number | null;
  onPosted?: () => void | Promise<void>;
}

function toQty(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function ReceiveGoodsDialog({
  open,
  onOpenChange,
  purchaseOrders,
  locations,
  initialPoId = null,
  onPosted,
}: ReceiveGoodsDialogProps) {
  const { t } = useTranslation("inventory");

  const [poId, setPoId] = useState<number>(0);
  const [lines, setLines] = useState<PurchaseOrderLine[]>([]);
  const [drafts, setDrafts] = useState<Record<number, LineDraft>>({});
  const [fulfillmentAction, setFulfillmentAction] = useState<string>("STOCK_ONLY");
  const [loadingLines, setLoadingLines] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const receivablePos = useMemo(
    () => purchaseOrders.filter((po) => isReceivablePurchaseOrder(po.status)),
    [purchaseOrders],
  );

  // Reset to a clean draft each time the dialog opens.
  useEffect(() => {
    if (!open) return;
    setPoId(initialPoId ?? 0);
    setLines([]);
    setDrafts({});
    setFulfillmentAction("STOCK_ONLY");
    setError(null);
  }, [open, initialPoId]);

  useEffect(() => {
    if (!open || poId <= 0) {
      setLines([]);
      setDrafts({});
      return;
    }
    let cancelled = false;
    setLoadingLines(true);
    listInventoryPurchaseOrderLines(poId)
      .then((rows) => {
        if (cancelled) return;
        const openLines = rows.filter((line) => line.remaining_qty > 0);
        setLines(openLines);
        setDrafts(Object.fromEntries(openLines.map((line) => [line.id, { ...EMPTY_DRAFT }])));
      })
      .catch((err) => {
        if (cancelled) return;
        setLines([]);
        setDrafts({});
        setError(toErrorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setLoadingLines(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, poId]);

  const patchDraft = useCallback((lineId: number, patch: Partial<LineDraft>) => {
    setDrafts((current) => ({
      ...current,
      [lineId]: { ...(current[lineId] ?? EMPTY_DRAFT), ...patch },
    }));
  }, []);

  const activeLines = useMemo(
    () => lines.filter((line) => toQty(drafts[line.id]?.receivedQty ?? "") > 0),
    [lines, drafts],
  );

  const hasDemandLine = useMemo(
    () => activeLines.some((line) => Boolean(line.demand_source_ref)),
    [activeLines],
  );

  const validationError = useMemo<string | null>(() => {
    if (activeLines.length === 0) return t("procurement.receive.errors.noLines");
    for (const line of activeLines) {
      const draft = drafts[line.id] ?? EMPTY_DRAFT;
      const received = toQty(draft.receivedQty);
      const rejected = toQty(draft.rejectedQty);
      if (draft.locationId <= 0) return t("procurement.receive.errors.location");
      if (received > line.remaining_qty) return t("procurement.receive.errors.overRemaining");
      if (rejected < 0 || rejected > received)
        return t("procurement.receive.errors.rejectedExceeds");
    }
    return null;
  }, [activeLines, drafts, t]);

  const post = async () => {
    if (poId <= 0 || validationError) return;
    setSaving(true);
    setError(null);
    try {
      const payload: ReceivePurchaseOrderLineInput[] = activeLines.map((line) => {
        const draft = drafts[line.id] ?? EMPTY_DRAFT;
        const received = toQty(draft.receivedQty);
        const rejected = toQty(draft.rejectedQty);
        return {
          po_line_id: line.id,
          article_id: line.article_id,
          location_id: draft.locationId,
          received_qty: received,
          accepted_qty: received - rejected,
          rejected_qty: rejected,
          rejection_reason: draft.rejectionReason.trim() || null,
        };
      });
      await receiveInventoryPurchaseOrderGoods({
        purchase_order_id: poId,
        lines: payload,
        fulfillment_action: hasDemandLine ? fulfillmentAction : null,
      });
      onOpenChange(false);
      await onPosted?.();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex max-h-[90vh] max-w-3xl flex-col gap-0 overflow-hidden p-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className="shrink-0 border-b px-6 py-4">
          <DialogTitle>{t("procurement.receive.title")}</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-4">
          <p className="text-xs text-text-muted">{t("procurement.receive.hint")}</p>
          {error ? <p className="text-sm text-destructive">{error}</p> : null}

          <div className="space-y-1">
            <Label>{t("procurement.receive.purchaseOrder")}</Label>
            <Select value={String(poId)} onValueChange={(v) => setPoId(Number(v))}>
              <SelectTrigger>
                <SelectValue placeholder={t("procurement.receive.selectPo")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="0">{t("procurement.receive.selectPo")}</SelectItem>
                {receivablePos.map((po) => (
                  <SelectItem key={po.id} value={String(po.id)}>
                    {formatEntityCode(po.po_number)} —{" "}
                    {t(`procurement.statuses.${po.status}`, { defaultValue: po.status })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {poId <= 0 ? (
            <p className="text-sm text-text-muted">{t("procurement.receive.noPo")}</p>
          ) : loadingLines ? (
            <p className="text-sm text-text-muted">{t("procurement.receive.loadingLines")}</p>
          ) : lines.length === 0 ? (
            <p className="text-sm text-text-muted">{t("procurement.receive.linesEmpty")}</p>
          ) : (
            <div className="space-y-3">
              {lines.map((line) => {
                const draft = drafts[line.id] ?? EMPTY_DRAFT;
                const received = toQty(draft.receivedQty);
                const rejected = toQty(draft.rejectedQty);
                const accepted = Math.max(0, received - rejected);
                return (
                  <div key={line.id} className="space-y-2 rounded-md border p-3">
                    <div className="flex flex-wrap items-baseline justify-between gap-2">
                      <span className="text-sm font-medium">
                        {formatAssetLabel(line.article_code, line.article_name)}
                      </span>
                      <span className="text-xs text-text-muted">
                        {t("procurement.receive.columns.remaining")}:{" "}
                        <strong className="text-foreground tabular-nums">
                          {line.remaining_qty}
                        </strong>
                      </span>
                    </div>
                    <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
                      <div className="space-y-1">
                        <Label htmlFor={`gr-loc-${line.id}`} className="text-xs">
                          {t("procurement.receive.columns.location")}
                        </Label>
                        <Select
                          value={String(draft.locationId)}
                          onValueChange={(v) => patchDraft(line.id, { locationId: Number(v) })}
                        >
                          <SelectTrigger id={`gr-loc-${line.id}`}>
                            <SelectValue placeholder={t("procurement.receive.selectLocation")} />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="0">
                              {t("procurement.receive.selectLocation")}
                            </SelectItem>
                            {locations.map((location) => (
                              <SelectItem key={location.id} value={String(location.id)}>
                                {location.warehouse_code}/{location.code}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor={`gr-received-${line.id}`} className="text-xs">
                          {t("procurement.receive.columns.receivedQty")}
                        </Label>
                        <Input
                          id={`gr-received-${line.id}`}
                          type="number"
                          min={0}
                          max={line.remaining_qty}
                          step="0.01"
                          value={draft.receivedQty}
                          onChange={(e) => patchDraft(line.id, { receivedQty: e.target.value })}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor={`gr-rejected-${line.id}`} className="text-xs">
                          {t("procurement.receive.columns.rejectedQty")}
                        </Label>
                        <Input
                          id={`gr-rejected-${line.id}`}
                          type="number"
                          min={0}
                          step="0.01"
                          value={draft.rejectedQty}
                          onChange={(e) => patchDraft(line.id, { rejectedQty: e.target.value })}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor={`gr-reason-${line.id}`} className="text-xs">
                          {t("procurement.receive.columns.rejectionReason")}
                        </Label>
                        <Input
                          id={`gr-reason-${line.id}`}
                          value={draft.rejectionReason}
                          disabled={rejected <= 0}
                          onChange={(e) => patchDraft(line.id, { rejectionReason: e.target.value })}
                        />
                      </div>
                    </div>
                    <p className="text-xs text-text-muted">
                      {t("procurement.receive.columns.accepted")}:{" "}
                      <strong className="text-foreground tabular-nums">{accepted}</strong>
                    </p>
                  </div>
                );
              })}

              {hasDemandLine ? (
                <div className="space-y-1 rounded-md border border-dashed p-3">
                  <Label>{t("procurement.receive.fulfillmentAction")}</Label>
                  <Select value={fulfillmentAction} onValueChange={setFulfillmentAction}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {FULFILLMENT_ACTIONS.map((action) => (
                        <SelectItem key={action} value={action}>
                          {t(`procurement.receive.fulfillmentValues.${action}`, {
                            defaultValue: action,
                          })}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              ) : null}
            </div>
          )}
        </div>

        <DialogFooter className="shrink-0 flex-wrap gap-2 border-t px-6 py-3">
          {validationError && lines.length > 0 ? (
            <span className="mr-auto text-xs text-text-muted">{validationError}</span>
          ) : null}
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t("procurement.receive.cancel")}
          </Button>
          <Button
            type="button"
            disabled={saving || poId <= 0 || validationError != null}
            onClick={() => void post()}
          >
            {t("procurement.receive.post")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
