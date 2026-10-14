/**
 * Goods receipt dialog — multi-line receiving for one purchase order.
 *
 * The whole receipt is posted with a single `receive_inventory_purchase_order_goods`
 * call so the backend creates one GR document with all its lines. Partial receipts are line by
 * line (the rest stays open on the PO). For a PO in a supplier currency the receipt captures the
 * exchange rate (entered, else the stored rate valid on the receipt date) and stock is valued in
 * the tenant currency. Lot / serial-tracked articles record the lots received.
 */

import { Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
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
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel, formatEntityCode } from "@/lib/display";
import {
  type InventoryArticleRow,
  type PurchaseOrderRow,
  type ReceiptLotInput,
  type ReceiveGoodsLineInput,
  type ResolvedExchangeRate,
  isLotTracked,
  listInventoryArticles,
  listInventoryPurchaseOrderLines,
  receiveInventoryPurchaseOrderGoods,
  resolveCurrencyExchangeRate,
} from "@/services/inventory-service";
import type { PurchaseOrder, PurchaseOrderLine, StockLocation } from "@shared/ipc-types";

const FULFILLMENT_ACTIONS = ["STOCK_ONLY", "RESERVE_FOR_DEMAND", "ISSUE_TO_DEMAND"] as const;

/** PO statuses the backend accepts for goods receipt posting. */
export const RECEIVABLE_PO_STATUSES = ["APPROVED", "PARTIALLY_RECEIVED"] as const;

export function isReceivablePurchaseOrder(status: string): boolean {
  return (RECEIVABLE_PO_STATUSES as readonly string[]).includes(status);
}

type LotDraft = {
  key: string;
  lotNumber: string;
  serialNumber: string;
  expiryDate: string;
  quantity: string;
};

type LineDraft = {
  locationId: number;
  receivedQty: string;
  rejectedQty: string;
  rejectionReason: string;
  lots: LotDraft[];
};

const EMPTY_DRAFT: LineDraft = {
  locationId: 0,
  receivedQty: "",
  rejectedQty: "",
  rejectionReason: "",
  lots: [],
};

let lotSeq = 0;
function newLot(quantity = ""): LotDraft {
  lotSeq += 1;
  return { key: `lot-${lotSeq}`, lotNumber: "", serialNumber: "", expiryDate: "", quantity };
}

export interface ReceiveGoodsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Full PO list; only receivable orders are selectable. */
  purchaseOrders: Array<PurchaseOrder & Partial<Pick<PurchaseOrderRow, "currency_code">>>;
  locations: StockLocation[];
  /** Article master (tracking rules); loaded by the dialog when not given. */
  articles?: InventoryArticleRow[];
  /** Preselected PO (e.g. when opened from the PO workspace). */
  initialPoId?: number | null;
  onPosted?: () => void | Promise<void>;
}

function toQty(value: string): number {
  const parsed = Number(value.replace(",", "."));
  return Number.isFinite(parsed) ? parsed : 0;
}

function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

type LotProblem = "missing" | "expiry" | "serialQty" | "sum" | null;

/** Checks the lots of a tracked line against the accepted quantity. */
export function lotProblem(
  lots: LotDraft[],
  accepted: number,
  rules: { serial: boolean; expiry: boolean },
): LotProblem {
  if (accepted <= 0) return null;
  if (lots.length === 0) return "missing";
  for (const lot of lots) {
    const id = rules.serial ? lot.serialNumber.trim() : lot.lotNumber.trim();
    if (!id) return "missing";
    if (rules.expiry && !lot.expiryDate) return "expiry";
    if (rules.serial && toQty(lot.quantity) !== 1) return "serialQty";
  }
  const sum = lots.reduce((s, l) => s + toQty(l.quantity), 0);
  if (Math.abs(sum - accepted) > 1e-6) return "sum";
  return null;
}

export function ReceiveGoodsDialog({
  open,
  onOpenChange,
  purchaseOrders,
  locations,
  articles: articlesProp,
  initialPoId = null,
  onPosted,
}: ReceiveGoodsDialogProps) {
  const { t } = useTranslation("inventory");
  const { currencyCode, formatCurrency, formatDate, formatNumber } = useFormatters();

  const [poId, setPoId] = useState<number>(0);
  const [lines, setLines] = useState<PurchaseOrderLine[]>([]);
  const [drafts, setDrafts] = useState<Record<number, LineDraft>>({});
  const [fulfillmentAction, setFulfillmentAction] = useState<string>("STOCK_ONLY");
  const [loadingLines, setLoadingLines] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [postingDate, setPostingDate] = useState(todayIso());
  const [rateInput, setRateInput] = useState("");
  const [storedRate, setStoredRate] = useState<ResolvedExchangeRate | null>(null);
  const [loadedArticles, setLoadedArticles] = useState<InventoryArticleRow[]>([]);
  const articles = articlesProp ?? loadedArticles;

  const receivablePos = useMemo(
    () => purchaseOrders.filter((po) => isReceivablePurchaseOrder(po.status)),
    [purchaseOrders],
  );
  const selectedPo = receivablePos.find((po) => po.id === poId) ?? null;
  const poCurrency =
    selectedPo?.currency_code && selectedPo.currency_code !== currencyCode
      ? selectedPo.currency_code
      : null;

  // Reset to a clean draft each time the dialog opens.
  useEffect(() => {
    if (!open) return;
    setPoId(initialPoId ?? 0);
    setLines([]);
    setDrafts({});
    setFulfillmentAction("STOCK_ONLY");
    setError(null);
    setPostingDate(todayIso());
    setRateInput("");
    setStoredRate(null);
  }, [open, initialPoId]);

  useEffect(() => {
    if (!open || articlesProp) return;
    listInventoryArticles({ search: null })
      .then(setLoadedArticles)
      .catch(() => setLoadedArticles([]));
  }, [open, articlesProp]);

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
      .catch((err: unknown) => {
        if (cancelled) return;
        setLines([]);
        setDrafts({});
        setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoadingLines(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, poId]);

  // Stored rate valid on the receipt date (foreign-currency PO only).
  useEffect(() => {
    if (!open || !poCurrency || !postingDate) {
      setStoredRate(null);
      return;
    }
    let cancelled = false;
    resolveCurrencyExchangeRate(poCurrency, postingDate)
      .then((r) => {
        if (!cancelled) setStoredRate(r);
      })
      .catch(() => {
        if (!cancelled) setStoredRate(null);
      });
    return () => {
      cancelled = true;
    };
  }, [open, poCurrency, postingDate]);

  const patchDraft = useCallback((lineId: number, patch: Partial<LineDraft>) => {
    setDrafts((current) => ({
      ...current,
      [lineId]: { ...(current[lineId] ?? EMPTY_DRAFT), ...patch },
    }));
  }, []);

  const trackingOf = useCallback(
    (articleId: number) => {
      const article = articles.find((a) => a.id === articleId);
      return {
        tracked: article ? isLotTracked(article) : false,
        serial: article?.requires_serial_tracking === 1,
        expiry: article?.requires_expiration === 1,
      };
    },
    [articles],
  );

  const activeLines = useMemo(
    () => lines.filter((line) => toQty(drafts[line.id]?.receivedQty ?? "") > 0),
    [lines, drafts],
  );

  const hasDemandLine = useMemo(
    () => activeLines.some((line) => Boolean(line.demand_source_ref)),
    [activeLines],
  );

  const enteredRate = rateInput.trim() === "" ? null : toQty(rateInput);
  const effectiveRate = poCurrency ? (enteredRate ?? storedRate?.rate ?? null) : 1;

  const validationError = useMemo<string | null>(() => {
    if (activeLines.length === 0) return t("procurement.receive.errors.noLines");
    if (!postingDate) return t("procurement.receive.errors.postingDate");
    if (postingDate > todayIso()) return t("procurement.receive.errors.futureDate");
    if (poCurrency) {
      if (enteredRate !== null && !(enteredRate > 0)) return t("procurement.receive.errors.rate");
      if (enteredRate === null && storedRate?.rate == null)
        return t("procurement.receive.errors.rateMissing", { currency: poCurrency });
    }
    for (const line of activeLines) {
      const draft = drafts[line.id] ?? EMPTY_DRAFT;
      const received = toQty(draft.receivedQty);
      const rejected = toQty(draft.rejectedQty);
      if (draft.locationId <= 0) return t("procurement.receive.errors.location");
      if (received > line.remaining_qty) return t("procurement.receive.errors.overRemaining");
      if (rejected < 0 || rejected > received)
        return t("procurement.receive.errors.rejectedExceeds");
      const tracking = trackingOf(line.article_id);
      if (tracking.tracked) {
        const problem = lotProblem(draft.lots, received - rejected, tracking);
        if (problem)
          return t(`procurement.receive.lots.errors.${problem}`, {
            article: line.article_code,
          });
      }
    }
    return null;
  }, [activeLines, drafts, enteredRate, poCurrency, postingDate, storedRate, t, trackingOf]);

  const post = async () => {
    if (poId <= 0 || validationError) return;
    setSaving(true);
    setError(null);
    try {
      const payload: ReceiveGoodsLineInput[] = activeLines.map((line) => {
        const draft = drafts[line.id] ?? EMPTY_DRAFT;
        const received = toQty(draft.receivedQty);
        const rejected = toQty(draft.rejectedQty);
        const tracking = trackingOf(line.article_id);
        const lots: ReceiptLotInput[] | null = tracking.tracked
          ? draft.lots.map((lot) => ({
              lot_number: tracking.serial ? null : lot.lotNumber.trim(),
              serial_number: tracking.serial ? lot.serialNumber.trim() : null,
              expiry_date: lot.expiryDate || null,
              quantity: toQty(lot.quantity),
            }))
          : null;
        return {
          po_line_id: line.id,
          article_id: line.article_id,
          location_id: draft.locationId,
          received_qty: received,
          accepted_qty: received - rejected,
          rejected_qty: rejected,
          rejection_reason: draft.rejectionReason.trim() || null,
          lots,
        };
      });
      await receiveInventoryPurchaseOrderGoods({
        purchase_order_id: poId,
        lines: payload,
        fulfillment_action: hasDemandLine ? fulfillmentAction : null,
        exchange_rate: poCurrency ? enteredRate : null,
        posting_date: postingDate === todayIso() ? null : postingDate,
      });
      onOpenChange(false);
      await onPosted?.();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  };

  const qtyLabel = (v: number) => formatNumber(v, { maximumFractionDigits: 3 });

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
          {error ? <ErrorState error={error} title={t("procurement.receive.postError")} /> : null}

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="gr-po">{t("procurement.receive.purchaseOrder")}</Label>
              <Select value={String(poId)} onValueChange={(v) => v && setPoId(Number(v))}>
                <SelectTrigger id="gr-po">
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
            <div className="space-y-1">
              <Label htmlFor="gr-posting-date">{t("procurement.receive.postingDate")}</Label>
              <Input
                id="gr-posting-date"
                type="date"
                max={todayIso()}
                value={postingDate}
                onChange={(e) => setPostingDate(e.target.value)}
                aria-describedby="gr-posting-date-hint"
              />
              <p id="gr-posting-date-hint" className="text-xs text-text-muted">
                {t("procurement.receive.postingDateHint")}
              </p>
            </div>
          </div>

          {poCurrency ? (
            <div className="space-y-1 rounded-md border border-dashed p-3">
              <Label htmlFor="gr-rate">
                {t("procurement.receive.rate.label", { from: poCurrency, to: currencyCode })}
              </Label>
              <Input
                id="gr-rate"
                inputMode="decimal"
                className="tabular-nums"
                value={rateInput}
                placeholder={storedRate?.rate != null ? String(storedRate.rate) : ""}
                onChange={(e) => setRateInput(e.target.value)}
                aria-describedby="gr-rate-hint"
              />
              <p id="gr-rate-hint" className="text-xs text-text-muted">
                {storedRate?.rate != null
                  ? t("procurement.receive.rate.stored", {
                      rate: formatNumber(storedRate.rate, { maximumFractionDigits: 6 }),
                      date: storedRate.valid_from ? formatDate(storedRate.valid_from) : "",
                    })
                  : t("procurement.receive.rate.none")}
              </p>
            </div>
          ) : null}

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
                const tracking = trackingOf(line.article_id);
                const patchLot = (key: string, patch: Partial<LotDraft>) =>
                  patchDraft(line.id, {
                    lots: draft.lots.map((l) => (l.key === key ? { ...l, ...patch } : l)),
                  });
                return (
                  <div key={line.id} className="space-y-2 rounded-md border p-3">
                    <div className="flex flex-wrap items-baseline justify-between gap-2">
                      <span className="text-sm font-medium">
                        {formatAssetLabel(line.article_code, line.article_name)}
                      </span>
                      <span className="text-xs text-text-muted">
                        {t("procurement.receive.columns.remaining")}:{" "}
                        <strong className="text-foreground tabular-nums">
                          {qtyLabel(line.remaining_qty)}
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
                          onValueChange={(v) => v && patchDraft(line.id, { locationId: Number(v) })}
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
                          onChange={(e) => {
                            const value = e.target.value;
                            // A tracked line starts with one lot holding the whole quantity.
                            const lots =
                              tracking.tracked && draft.lots.length === 0 && toQty(value) > 0
                                ? [newLot(tracking.serial ? "1" : value)]
                                : draft.lots;
                            patchDraft(line.id, { receivedQty: value, lots });
                          }}
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
                      <strong className="text-foreground tabular-nums">{qtyLabel(accepted)}</strong>
                      {line.unit_price != null && effectiveRate != null ? (
                        <span className="ml-3 tabular-nums">
                          {t("procurement.receive.stockCost", {
                            cost: formatCurrency(line.unit_price * effectiveRate),
                          })}
                        </span>
                      ) : null}
                    </p>

                    {tracking.tracked && accepted > 0 ? (
                      <fieldset className="space-y-2 rounded-md border border-dashed p-2">
                        <legend className="px-1 text-xs font-medium">
                          {tracking.serial
                            ? t("procurement.receive.lots.serialTitle")
                            : t("procurement.receive.lots.title")}
                        </legend>
                        {draft.lots.map((lot, index) => (
                          <div
                            key={lot.key}
                            className="grid items-end gap-2 sm:grid-cols-[1.4fr_1fr_0.8fr_auto]"
                          >
                            <div className="space-y-1">
                              <Label htmlFor={`gr-lot-id-${lot.key}`} className="text-xs">
                                {tracking.serial
                                  ? t("procurement.receive.lots.serialNumber")
                                  : t("procurement.receive.lots.lotNumber")}
                              </Label>
                              <Input
                                id={`gr-lot-id-${lot.key}`}
                                className="font-mono"
                                value={tracking.serial ? lot.serialNumber : lot.lotNumber}
                                onChange={(e) =>
                                  patchLot(
                                    lot.key,
                                    tracking.serial
                                      ? { serialNumber: e.target.value }
                                      : { lotNumber: e.target.value },
                                  )
                                }
                              />
                            </div>
                            <div className="space-y-1">
                              <Label htmlFor={`gr-lot-exp-${lot.key}`} className="text-xs">
                                {tracking.expiry
                                  ? t("procurement.receive.lots.expiry")
                                  : t("procurement.receive.lots.expiryOptional")}
                              </Label>
                              <Input
                                id={`gr-lot-exp-${lot.key}`}
                                type="date"
                                value={lot.expiryDate}
                                onChange={(e) => patchLot(lot.key, { expiryDate: e.target.value })}
                              />
                            </div>
                            <div className="space-y-1">
                              <Label htmlFor={`gr-lot-qty-${lot.key}`} className="text-xs">
                                {t("procurement.receive.lots.quantity")}
                              </Label>
                              <Input
                                id={`gr-lot-qty-${lot.key}`}
                                type="number"
                                min={0}
                                step="0.01"
                                value={lot.quantity}
                                disabled={tracking.serial}
                                onChange={(e) => patchLot(lot.key, { quantity: e.target.value })}
                              />
                            </div>
                            <Button
                              type="button"
                              variant="ghost"
                              className="h-11 w-11 p-0"
                              aria-label={t("procurement.receive.lots.remove", { n: index + 1 })}
                              onClick={() =>
                                patchDraft(line.id, {
                                  lots: draft.lots.filter((l) => l.key !== lot.key),
                                })
                              }
                            >
                              <Trash2 className="h-4 w-4" aria-hidden />
                            </Button>
                          </div>
                        ))}
                        <div className="flex flex-wrap items-center justify-between gap-2">
                          <Button
                            type="button"
                            variant="outline"
                            className="h-11"
                            onClick={() =>
                              patchDraft(line.id, {
                                lots: [...draft.lots, newLot(tracking.serial ? "1" : "")],
                              })
                            }
                          >
                            <Plus className="mr-1 h-4 w-4" aria-hidden />
                            {tracking.serial
                              ? t("procurement.receive.lots.addSerial")
                              : t("procurement.receive.lots.add")}
                          </Button>
                          <span className="text-xs tabular-nums text-text-muted">
                            {t("procurement.receive.lots.sum", {
                              sum: qtyLabel(draft.lots.reduce((s, l) => s + toQty(l.quantity), 0)),
                              accepted: qtyLabel(accepted),
                            })}
                          </span>
                        </div>
                      </fieldset>
                    ) : null}
                  </div>
                );
              })}

              {hasDemandLine ? (
                <div className="space-y-1 rounded-md border border-dashed p-3">
                  <Label htmlFor="gr-fulfillment">
                    {t("procurement.receive.fulfillmentAction")}
                  </Label>
                  <Select value={fulfillmentAction} onValueChange={setFulfillmentAction}>
                    <SelectTrigger id="gr-fulfillment">
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
            <span role="status" className="mr-auto text-xs text-text-muted">
              {validationError}
            </span>
          ) : null}
          <Button
            type="button"
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
          >
            {t("procurement.receive.cancel")}
          </Button>
          <Button
            type="button"
            className="h-11"
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
