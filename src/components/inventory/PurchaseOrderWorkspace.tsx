/**
 * Purchase order workspace — tabbed detail dialog for procurement maturity.
 */

import { Printer } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { printPoFiche } from "@/components/inventory/PoPrintFiche";
import { isReceivablePurchaseOrder } from "@/components/inventory/ReceiveGoodsDialog";
import { StockImpactPreview } from "@/components/inventory/StockImpactPreview";
import { poLateDays } from "@/components/inventory/po-late";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineStepState } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { POSTING_STATE_TONE, PROCUREMENT_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { formatAssetLabel, formatEntityCode, formatOrDash } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  type PurchaseOrderDetailView,
  getInventoryPurchaseOrderDetail,
  listInventoryDocumentLinks,
  setInventoryPurchaseOrderLinePrice,
  transitionInventoryPurchaseOrder,
  updateInventoryProcurementPostingState,
  upsertInventoryDocumentLink,
} from "@/services/inventory-service";
import { formatDate, intlLocaleForLanguage } from "@/utils/format-date";
import type {
  InventoryDocumentLink,
  InventoryStateEvent,
  PurchaseOrderLine,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const DOC_PURPOSES = ["INVOICE", "CERTIFICATE", "QUOTATION", "PHOTO", "OTHER"] as const;

/** Canonical display steps for the PO lifecycle timeline. */
const PO_TIMELINE_STEPS = [
  { key: "CREATED", statuses: ["DRAFT"] },
  { key: "SUBMITTED", statuses: ["SUBMITTED"] },
  { key: "APPROVED", statuses: ["APPROVED"] },
  { key: "RECEIVED", statuses: ["PARTIALLY_RECEIVED", "RECEIVED_CLOSED"] },
  { key: "CLOSED", statuses: ["RECEIVED_CLOSED"] },
] as const;

/** ERP posting states seeded in `inventory.erp_posting_state`. */
const POSTING_STATES = ["PENDING_POSTING", "POSTED", "POSTING_FAILED", "RECONCILED"] as const;

export interface PurchaseOrderWorkspaceProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  poId: number | null;
  onTransitioned?: () => void;
  onReload?: () => void | Promise<void>;
  /** Open the multi-line goods receipt dialog for this PO. */
  onReceiveGoods?: (poId: number) => void;
  /** Navigate back to the requisition that produced this PO. */
  onOpenRequisition?: (requisitionId: number) => void;
  /** Open a goods receipt from the receipts tab. */
  onOpenGoodsReceipt?: (goodsReceiptId: number) => void;
  /** Incrementing token forces a detail refresh (e.g. after posting a receipt). */
  reloadToken?: number;
  /**
   * Statuses that can be late (server `late_po_statuses`); without them no "late by" is shown
   * rather than a rule of the screen's own.
   */
  latePoStatuses?: readonly string[] | null;
}

function eventForStep(
  events: InventoryStateEvent[],
  statuses: readonly string[],
): InventoryStateEvent | undefined {
  return events.find((e) => statuses.includes(e.to_status));
}

function isStepComplete(currentStatus: string, stepIndex: number): boolean {
  const statusRank: Record<string, number> = {
    DRAFT: 0,
    SUBMITTED: 1,
    APPROVED: 2,
    PARTIALLY_RECEIVED: 3,
    RECEIVED_CLOSED: 4,
  };
  const rank = statusRank[currentStatus];
  if (rank == null) return false;
  if (stepIndex === 4) return currentStatus === "RECEIVED_CLOSED";
  if (stepIndex === 3) return rank >= 3;
  return rank >= stepIndex;
}

function purchaseOrderCancelTarget(status: string): "CANCELLED" | null {
  if (["DRAFT", "SUBMITTED", "APPROVED", "PARTIALLY_RECEIVED"].includes(status)) return "CANCELLED";
  return null;
}

export function PurchaseOrderWorkspace({
  open,
  onOpenChange,
  poId,
  onTransitioned,
  onReload,
  onReceiveGoods,
  onOpenRequisition,
  onOpenGoodsReceipt,
  reloadToken = 0,
  latePoStatuses = null,
}: PurchaseOrderWorkspaceProps) {
  const { t, i18n } = useTranslation("inventory");
  const locale = intlLocaleForLanguage(i18n.language);
  const { formatCurrency, currencyCode } = useFormatters();

  const [detail, setDetail] = useState<PurchaseOrderDetailView | null>(null);
  /** Currency the PO is priced in (supplier currency); the tenant currency when none. */
  const poCurrency = detail?.order.currency_code ?? currencyCode;
  /** Money in the PO currency, "—" when unknown. */
  const fmtMoney = (n: number | null | undefined) =>
    n == null || !Number.isFinite(n) ? "—" : formatCurrency(n, poCurrency);
  const [priceDraft, setPriceDraft] = useState("");
  const [priceSaving, setPriceSaving] = useState(false);
  const [priceError, setPriceError] = useState<unknown>(null);
  const [documents, setDocuments] = useState<InventoryDocumentLink[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  /** Failure to load the purchase order (action failures are reported by the global toast). */
  const [loadError, setLoadError] = useState<unknown>(null);
  const [cancelConfirmOpen, setCancelConfirmOpen] = useState(false);
  const [selectedLineId, setSelectedLineId] = useState<number | null>(null);
  const [docRef, setDocRef] = useState("");
  const [docPurpose, setDocPurpose] = useState<string>("INVOICE");
  const [postingState, setPostingState] = useState<string>("PENDING_POSTING");
  const [postingErrorDraft, setPostingErrorDraft] = useState("");

  const load = useCallback(async () => {
    if (!poId) {
      setDetail(null);
      setDocuments([]);
      return;
    }
    setLoading(true);
    setLoadError(null);
    try {
      const [poDetail, docs] = await Promise.all([
        getInventoryPurchaseOrderDetail(poId),
        listInventoryDocumentLinks("PURCHASE_ORDER", poId),
      ]);
      setDetail(poDetail);
      setDocuments(docs.length > 0 ? docs : poDetail.document_links);
      setPostingState(poDetail.order.posting_state);
      setPostingErrorDraft(poDetail.order.posting_error ?? "");
      setSelectedLineId((prev) => {
        if (prev && poDetail.lines.some((l) => l.id === prev)) return prev;
        return poDetail.lines[0]?.id ?? null;
      });
    } catch (err) {
      setLoadError(err);
      setDetail(null);
    } finally {
      setLoading(false);
    }
  }, [poId]);

  useEffect(() => {
    if (!open || !poId) return;
    void load();
  }, [open, poId, load, reloadToken]);

  const selectedLine: PurchaseOrderLine | null = useMemo(
    () => detail?.lines.find((l) => l.id === selectedLineId) ?? null,
    [detail, selectedLineId],
  );

  useEffect(() => {
    setPriceDraft(selectedLine?.unit_price != null ? String(selectedLine.unit_price) : "");
    setPriceError(null);
  }, [selectedLine]);

  const parsedPrice = priceDraft.trim() === "" ? null : Number(priceDraft.replace(",", "."));
  const priceInvalid = parsedPrice !== null && !(parsedPrice >= 0);

  const savePrice = async () => {
    if (!selectedLine || priceInvalid || priceSaving) return;
    setPriceSaving(true);
    setPriceError(null);
    try {
      await setInventoryPurchaseOrderLinePrice(selectedLine.id, parsedPrice);
      await load();
    } catch (err) {
      setPriceError(err);
    } finally {
      setPriceSaving(false);
    }
  };

  const lateDays = detail
    ? poLateDays(detail.order.expected_delivery_date, detail.order.status, latePoStatuses ?? [])
    : null;

  /** Resolves to false when the transition failed (reported by the global IPC toast). */
  const runTransition = async (nextStatus: string): Promise<boolean> => {
    if (!detail) return false;
    setSaving(true);
    try {
      await transitionInventoryPurchaseOrder({
        purchase_order_id: detail.order.id,
        expected_row_version: detail.order.row_version,
        next_status: nextStatus,
      });
      await load();
      onTransitioned?.();
      await onReload?.();
      return true;
    } catch {
      return false;
    } finally {
      setSaving(false);
    }
  };

  const applyPostingState = async () => {
    if (!detail) return;
    setSaving(true);
    try {
      await updateInventoryProcurementPostingState({
        entity_type: "purchase_order",
        entity_id: detail.order.id,
        posting_state: postingState,
        posting_error: postingErrorDraft.trim() || null,
      });
      await load();
      await onReload?.();
    } catch {
      // reported by the global IPC toast
    } finally {
      setSaving(false);
    }
  };

  const addDocument = async () => {
    if (!detail || !docRef.trim()) return;
    setSaving(true);
    try {
      await upsertInventoryDocumentLink({
        entity_type: "PURCHASE_ORDER",
        entity_id: detail.order.id,
        document_ref: docRef.trim(),
        link_purpose: docPurpose,
      });
      setDocRef("");
      const docs = await listInventoryDocumentLinks("PURCHASE_ORDER", detail.order.id);
      setDocuments(docs);
    } catch {
      // reported by the global IPC toast
    } finally {
      setSaving(false);
    }
  };

  const handlePrint = () => {
    if (!detail) return;
    void printPoFiche(
      detail,
      (key) =>
        (t as (k: string, o?: { defaultValue?: string }) => string)(key, {
          defaultValue: key,
        }),
      locale,
      currencyCode,
    );
  };

  const order = detail?.order;
  const cancelTarget = order ? purchaseOrderCancelTarget(order.status) : null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] max-w-3xl flex-col gap-0 overflow-hidden p-0">
        <DialogHeader className="shrink-0 border-b px-6 py-4">
          <DialogTitle className="flex flex-wrap items-center gap-2">
            {order ? (
              <>
                <span className="font-mono">{formatEntityCode(order.po_number)}</span>
                <StatusChip
                  label={t(`procurement.statuses.${order.status}`, { defaultValue: order.status })}
                  tone={toneOf(PROCUREMENT_STATUS_TONE, order.status)}
                  dot
                />
                {lateDays != null ? (
                  <Badge variant="destructive">
                    {t("procurement.poWorkspace.lateByDays", { count: lateDays })}
                  </Badge>
                ) : null}
              </>
            ) : (
              t("procurement.poWorkspace.title")
            )}
          </DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          {loading ? (
            <p className="text-sm text-text-muted">{t("procurement.poWorkspace.loading")}</p>
          ) : loadError && !detail ? (
            <ErrorState error={loadError} onRetry={() => void load()} />
          ) : !detail || !order ? (
            <p className="text-sm text-text-muted">{t("procurement.poWorkspace.noSelection")}</p>
          ) : (
            <>
              <Tabs defaultValue="general">
                <TabsList className="mb-3 flex h-auto flex-wrap gap-1">
                  <TabsTrigger value="general">
                    {t("procurement.poWorkspace.tabs.general")}
                  </TabsTrigger>
                  <TabsTrigger value="lines">{t("procurement.poWorkspace.tabs.lines")}</TabsTrigger>
                  <TabsTrigger value="timeline">
                    {t("procurement.poWorkspace.tabs.timeline")}
                  </TabsTrigger>
                  <TabsTrigger value="receipts">
                    {t("procurement.poWorkspace.tabs.receipts")}
                  </TabsTrigger>
                  <TabsTrigger value="documents">
                    {t("procurement.poWorkspace.tabs.documents")}
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="general" className="space-y-3 text-sm">
                  <div className="grid gap-2 sm:grid-cols-2">
                    <Field
                      label={t("procurement.poWorkspace.fields.supplier")}
                      value={formatOrDash(
                        order.supplier_name?.trim() || order.supplier_company_name,
                      )}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.status")}
                      value={t(`procurement.statuses.${order.status}`, {
                        defaultValue: order.status,
                      })}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.posting")}
                      value={t(`procurement.postingStates.${order.posting_state}`, {
                        defaultValue: order.posting_state,
                      })}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.orderedBy")}
                      value={formatOrDash(order.ordered_by_name)}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.currency")}
                      value={
                        order.currency_code && order.currency_code !== currencyCode
                          ? t("procurement.poWorkspace.currencyForeign", {
                              code: order.currency_code,
                              tenant: currencyCode,
                            })
                          : t("procurement.poForm.currencyTenant", { code: currencyCode })
                      }
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.createdAt")}
                      value={formatDate(order.created_at, locale)}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.orderedAt")}
                      value={formatDate(order.ordered_at, locale)}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.approvedAt")}
                      value={formatDate(order.approved_at, locale)}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.expectedDelivery")}
                      value={formatDate(order.expected_delivery_date, locale)}
                    />
                    <Field
                      label={t("procurement.poWorkspace.fields.updatedAt")}
                      value={formatDate(order.updated_at, locale)}
                    />
                    {order.requisition_id != null && onOpenRequisition ? (
                      <div>
                        <div className="text-xs text-text-muted">
                          {t("procurement.poWorkspace.fields.requisition")}
                        </div>
                        <Button
                          type="button"
                          variant="link"
                          className="h-auto p-0 font-medium"
                          onClick={() => {
                            onOpenChange(false);
                            onOpenRequisition(order.requisition_id as number);
                          }}
                        >
                          {t("procurement.requisitions.actions.open")}
                        </Button>
                      </div>
                    ) : null}
                  </div>

                  {order.posting_error ? (
                    <p className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-xs">
                      <span className="font-medium">{t("procurement.posting.error")}: </span>
                      {order.posting_error}
                    </p>
                  ) : null}

                  <PermissionGate permission={P.ERP_RECONCILE}>
                    <div className="space-y-2 rounded-md border p-3">
                      <div className="text-sm font-semibold">{t("procurement.posting.title")}</div>
                      <p className="text-xs text-text-muted">{t("procurement.posting.hint")}</p>
                      <div className="grid gap-2 sm:grid-cols-[180px_1fr_auto]">
                        <div className="space-y-1">
                          <Label className="text-xs">{t("procurement.posting.state")}</Label>
                          <Select value={postingState} onValueChange={setPostingState}>
                            <SelectTrigger>
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {POSTING_STATES.map((state) => (
                                <SelectItem key={state} value={state}>
                                  {t(`procurement.postingStates.${state}`, { defaultValue: state })}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="space-y-1">
                          <Label className="text-xs">{t("procurement.posting.error")}</Label>
                          <Input
                            value={postingErrorDraft}
                            placeholder={t("procurement.posting.errorPlaceholder")}
                            onChange={(e) => setPostingErrorDraft(e.target.value)}
                          />
                        </div>
                        <div className="flex items-end">
                          <Button
                            type="button"
                            size="sm"
                            disabled={
                              saving ||
                              (postingState === order.posting_state &&
                                postingErrorDraft === (order.posting_error ?? ""))
                            }
                            onClick={() => void applyPostingState()}
                          >
                            {t("procurement.posting.apply")}
                          </Button>
                        </div>
                      </div>
                    </div>
                  </PermissionGate>
                </TabsContent>

                <TabsContent value="lines" className="space-y-3">
                  {detail.lines.length === 0 ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.poWorkspace.lines.empty")}
                    </p>
                  ) : (
                    <div className="space-y-2">
                      {detail.lines.map((line) => {
                        const selected = line.id === selectedLineId;
                        return (
                          <button
                            key={line.id}
                            type="button"
                            className={cn(
                              "w-full rounded-md border p-3 text-left text-sm hover:bg-accent/40",
                              selected && "ring-2 ring-primary",
                            )}
                            onClick={() => setSelectedLineId(line.id)}
                          >
                            <div className="flex flex-wrap items-start justify-between gap-2">
                              <div className="font-medium">
                                {formatAssetLabel(line.article_code, line.article_name)}
                              </div>
                              {line.work_order_code ? (
                                <LinkedEntityBadge
                                  entity="work_order"
                                  code={line.work_order_code}
                                  entityId={line.work_order_id}
                                />
                              ) : null}
                            </div>
                            <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-text-muted sm:grid-cols-4">
                              <span>
                                {t("procurement.poWorkspace.lines.ordered")}:{" "}
                                <strong className="text-foreground">{line.ordered_qty}</strong>
                              </span>
                              <span>
                                {t("procurement.poWorkspace.lines.received")}:{" "}
                                <strong className="text-foreground">{line.received_qty}</strong>
                              </span>
                              <span>
                                {t("procurement.poWorkspace.lines.remaining")}:{" "}
                                <strong className="text-foreground">{line.remaining_qty}</strong>
                              </span>
                              <span>
                                {t("procurement.poWorkspace.lines.unitPrice")}:{" "}
                                <strong className="text-foreground">
                                  {fmtMoney(line.unit_price)}
                                </strong>
                              </span>
                            </div>
                            <div className="mt-1 text-xs">
                              {t("procurement.poWorkspace.lines.lineTotal")}:{" "}
                              <strong>{fmtMoney(line.line_total)}</strong>
                            </div>
                          </button>
                        );
                      })}
                      {selectedLine && order.status === "DRAFT" ? (
                        <PermissionGate permission={P.INV_PROCURE}>
                          <div className="space-y-1 rounded-md border border-dashed p-3">
                            <Label htmlFor="po-line-price">
                              {t("procurement.poWorkspace.price.label", {
                                article: selectedLine.article_code,
                                currency: poCurrency,
                              })}
                            </Label>
                            <div className="flex gap-2">
                              <Input
                                id="po-line-price"
                                inputMode="decimal"
                                className="h-11 tabular-nums"
                                value={priceDraft}
                                onChange={(e) => setPriceDraft(e.target.value)}
                                aria-describedby="po-line-price-hint"
                              />
                              <Button
                                type="button"
                                className="h-11"
                                disabled={priceSaving || priceInvalid}
                                onClick={() => void savePrice()}
                              >
                                {t("procurement.poWorkspace.price.save")}
                              </Button>
                            </div>
                            <p id="po-line-price-hint" className="text-xs text-text-muted">
                              {priceInvalid
                                ? t("procurement.poWorkspace.price.invalid")
                                : t("procurement.poWorkspace.price.hint")}
                            </p>
                            {priceError ? (
                              <ErrorState
                                error={priceError}
                                title={t("procurement.poWorkspace.price.error")}
                              />
                            ) : null}
                          </div>
                        </PermissionGate>
                      ) : null}
                      {selectedLine && selectedLine.remaining_qty > 0 ? (
                        <div>
                          <p className="mb-1 text-xs font-medium text-text-muted">
                            {t("stockImpact.title")}
                          </p>
                          <StockImpactPreview
                            articleId={selectedLine.article_id}
                            deltaQty={selectedLine.remaining_qty}
                            includeOpenPoQty
                          />
                        </div>
                      ) : null}
                    </div>
                  )}
                </TabsContent>

                <TabsContent value="timeline">
                  <Timeline
                    items={PO_TIMELINE_STEPS.map((step, index) => ({ step, index }))}
                    locale={locale}
                    getEntry={({ step, index }): TimelineEntry => {
                      const complete = isStepComplete(order.status, index);
                      const event = eventForStep(detail.state_events, step.statuses);
                      const isCurrent =
                        (step.statuses as readonly string[]).includes(order.status) ||
                        (step.key === "RECEIVED" && order.status === "PARTIALLY_RECEIVED") ||
                        (step.key === "CLOSED" && order.status === "RECEIVED_CLOSED");
                      let stepState: TimelineStepState = "pending";
                      if (complete && isCurrent) stepState = "current";
                      else if (complete) stepState = "complete";
                      return {
                        id: step.key,
                        title: t(`procurement.poWorkspace.timeline.${step.key}`),
                        ...(event?.changed_at ? { timestamp: event.changed_at } : {}),
                        subtitle: event
                          ? `${formatDate(event.changed_at, locale)}${event.note ? ` — ${event.note}` : ""}`
                          : complete
                            ? t("procurement.poWorkspace.timeline.reached")
                            : t("procurement.poWorkspace.timeline.pending"),
                        stepState,
                      };
                    }}
                  />
                  {order.status === "CANCELLED" ? (
                    <Badge variant="destructive" className="mt-2">
                      {t("procurement.poWorkspace.timeline.CANCELLED")}
                    </Badge>
                  ) : null}
                </TabsContent>

                <TabsContent value="receipts" className="space-y-2">
                  {detail.goods_receipts.length === 0 ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.poWorkspace.receipts.empty")}
                    </p>
                  ) : (
                    detail.goods_receipts.map((gr) => {
                      const body = (
                        <>
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-mono font-medium">
                              {formatEntityCode(gr.gr_number)}
                            </span>
                            <StatusChip
                              label={t(`procurement.statuses.${gr.status}`, {
                                defaultValue: gr.status,
                              })}
                              tone={toneOf(PROCUREMENT_STATUS_TONE, gr.status)}
                              dot
                            />
                            <StatusChip
                              label={t(`procurement.postingStates.${gr.posting_state}`, {
                                defaultValue: gr.posting_state,
                              })}
                              tone={toneOf(POSTING_STATE_TONE, gr.posting_state)}
                            />
                          </div>
                          <div className="mt-1 text-xs text-text-muted">
                            {t("procurement.poWorkspace.receipts.receivedAt")}:{" "}
                            {formatDate(gr.received_at, locale)}
                          </div>
                        </>
                      );
                      if (!onOpenGoodsReceipt) {
                        return (
                          <div key={gr.id} className="rounded-md border p-3 text-sm">
                            {body}
                          </div>
                        );
                      }
                      return (
                        <button
                          key={gr.id}
                          type="button"
                          title={t("procurement.poWorkspace.receipts.open")}
                          className="w-full rounded-md border p-3 text-left text-sm hover:bg-accent/40"
                          onClick={() => {
                            onOpenChange(false);
                            onOpenGoodsReceipt(gr.id);
                          }}
                        >
                          {body}
                        </button>
                      );
                    })
                  )}
                </TabsContent>

                <TabsContent value="documents" className="space-y-3">
                  {documents.length === 0 ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.poWorkspace.documents.empty")}
                    </p>
                  ) : (
                    <ul className="space-y-2">
                      {documents.map((doc) => (
                        <li
                          key={doc.id}
                          className="flex flex-wrap items-center justify-between gap-2 rounded border px-3 py-2 text-sm"
                        >
                          <span className="truncate font-medium">{doc.document_ref}</span>
                          <Badge variant="outline">
                            {t(
                              `documents.purposes.${doc.link_purpose}` as "documents.purposes.OTHER",
                              {
                                defaultValue: doc.link_purpose,
                              },
                            )}
                          </Badge>
                        </li>
                      ))}
                    </ul>
                  )}
                  <PermissionGate permission={P.INV_PROCURE}>
                    <div className="grid gap-2 rounded-md border p-3 sm:grid-cols-[1fr_160px_auto]">
                      <div className="space-y-1">
                        <Label>{t("procurement.poWorkspace.documents.fields.ref")}</Label>
                        <Input
                          value={docRef}
                          onChange={(e) => setDocRef(e.target.value)}
                          placeholder={t("procurement.poWorkspace.documents.refPlaceholder")}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label>{t("procurement.poWorkspace.documents.fields.purpose")}</Label>
                        <Select value={docPurpose} onValueChange={setDocPurpose}>
                          <SelectTrigger>
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            {DOC_PURPOSES.map((p) => (
                              <SelectItem key={p} value={p}>
                                {t(`documents.purposes.${p}` as "documents.purposes.OTHER")}
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                      <div className="flex items-end">
                        <Button
                          size="sm"
                          disabled={saving || !docRef.trim()}
                          onClick={() => void addDocument()}
                        >
                          {t("procurement.poWorkspace.documents.add")}
                        </Button>
                      </div>
                    </div>
                  </PermissionGate>
                </TabsContent>
              </Tabs>
            </>
          )}
        </div>

        <DialogFooter className="shrink-0 flex-wrap gap-2 border-t px-6 py-3 sm:justify-between">
          <div className="flex flex-wrap items-center gap-3 text-sm">
            {detail ? (
              <span>
                {t("procurement.poWorkspace.grandTotal")}:{" "}
                <strong>{fmtMoney(detail.grand_total)}</strong>
                {detail.grand_total_partial ? (
                  <span className="ml-1 text-xs text-text-muted">
                    ({t("procurement.poWorkspace.grandTotalPartial")})
                  </span>
                ) : null}
              </span>
            ) : null}
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!detail}
              onClick={handlePrint}
            >
              <Printer className="mr-1.5 h-3.5 w-3.5" aria-hidden />
              {t("procurement.poWorkspace.actions.print")}
            </Button>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => onOpenChange(false)}
            >
              {t("procurement.poWorkspace.actions.close")}
            </Button>
            {order ? (
              <PermissionGate permission={P.INV_PROCURE}>
                <>
                  {/* Only the transitions the current status allows are offered. */}
                  {order.status === "DRAFT" ? (
                    <Button
                      variant="outline"
                      className="h-11"
                      disabled={saving}
                      onClick={() => void runTransition("SUBMITTED")}
                    >
                      {t("procurement.poWorkspace.actions.submit")}
                    </Button>
                  ) : null}
                  {order.status === "SUBMITTED" ? (
                    <Button
                      className="h-11"
                      disabled={saving}
                      onClick={() => void runTransition("APPROVED")}
                    >
                      {t("procurement.poWorkspace.actions.approve")}
                    </Button>
                  ) : null}
                  {onReceiveGoods && isReceivablePurchaseOrder(order.status) ? (
                    <Button
                      variant="outline"
                      className="h-11"
                      disabled={saving}
                      onClick={() => onReceiveGoods(order.id)}
                    >
                      {t("procurement.poWorkspace.actions.receive")}
                    </Button>
                  ) : null}
                  {cancelTarget ? (
                    <Button
                      variant="destructive"
                      className="h-11"
                      disabled={saving}
                      onClick={() => setCancelConfirmOpen(true)}
                    >
                      {t("procurement.poWorkspace.actions.cancel")}
                    </Button>
                  ) : null}
                </>
              </PermissionGate>
            ) : null}
          </div>
        </DialogFooter>
      </DialogContent>
      <ConfirmDialog
        open={cancelConfirmOpen}
        onOpenChange={setCancelConfirmOpen}
        title={t("procurement.purchaseOrders.confirmCancel.title")}
        description={t("procurement.purchaseOrders.confirmCancel.description", {
          number: order?.po_number ?? "",
        })}
        confirmLabel={t("procurement.purchaseOrders.confirmCancel.confirm")}
        destructive
        onConfirm={async () => {
          if (!cancelTarget) return;
          const ok = await runTransition(cancelTarget);
          if (!ok) throw new Error("cancel failed");
        }}
      />
    </Dialog>
  );
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-xs text-text-muted">{label}</div>
      <div className="font-medium">{value}</div>
    </div>
  );
}
