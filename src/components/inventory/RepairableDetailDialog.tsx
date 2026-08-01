/**
 * Repairable order detail dialog — summary, timeline, repair-vs-replace, documents.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import {
  RepairableActionDialog,
  type RepairableActionKind,
  type RepairableActionPayload,
} from "@/components/inventory/RepairableActionDialog";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineStepState } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
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
import { formatAssetLabel, formatEntityCode, formatOrDash } from "@/lib/display";
import {
  getInventoryRepairableOrderDetail,
  listInventoryDocumentLinks,
  transitionInventoryRepairableOrder,
  upsertInventoryDocumentLink,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import { formatDate, intlLocaleForLanguage } from "@/utils/format-date";
import type {
  InventoryDocumentLink,
  InventoryStateEvent,
  InventorySupplier,
  RepairableOrderDetail,
  RepairVsReplaceResult,
  StockLocation,
  TransitionRepairableOrderInput,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const DOC_PURPOSES = ["INVOICE", "CERTIFICATE", "QUOTATION", "PHOTO", "OTHER"] as const;

const REPAIR_TIMELINE_STEPS = [
  { key: "REQUESTED", status: "REQUESTED" },
  { key: "RELEASED", status: "RELEASED" },
  { key: "SENT", status: "SENT_FOR_REPAIR" },
  { key: "RETURNED", status: "RETURNED_FROM_REPAIR" },
  { key: "CLOSED", status: "CLOSED" },
] as const;

export interface RepairableDetailDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  orderId: number | null;
  onTransitioned?: () => void;
  /** Active non-blocked vendors for dispatch capture (owned by the parent panel). */
  suppliers?: InventorySupplier[];
  /** Active stock locations for return / scrap capture. */
  locations?: StockLocation[];
  /** Navigate to the article master record. */
  onOpenArticle?: (articleId: number) => void;
}

function fmtMoney(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function fmtDays(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return n.toLocaleString(undefined, { maximumFractionDigits: 1 });
}

function eventForStatus(
  events: InventoryStateEvent[],
  status: string,
): InventoryStateEvent | undefined {
  return events.find((e) => e.to_status === status);
}

function stepRank(status: string): number {
  const idx = REPAIR_TIMELINE_STEPS.findIndex((s) => s.status === status);
  return idx;
}

function RepairVsReplaceCard({ result }: { result: RepairVsReplaceResult }) {
  const { t } = useTranslation("inventory");

  if (result.recommendation === "INSUFFICIENT_DATA") {
    return (
      <div className="rounded-md border border-dashed p-3 text-sm">
        <div className="font-medium">{t("procurement.repairableDetail.repairVsReplace.title")}</div>
        <p className="mt-1 text-text-muted">
          {t("procurement.repairableDetail.repairVsReplace.insufficientData")}
        </p>
        {result.reason ? (
          <p className="mt-1 text-xs text-text-muted">
            {t(`procurement.repairableDetail.repairVsReplace.reasons.${result.reason}`, {
              defaultValue: result.reason,
            })}
          </p>
        ) : null}
      </div>
    );
  }

  return (
    <div className="rounded-md border p-3 text-sm">
      <div className="mb-2 font-medium">
        {t("procurement.repairableDetail.repairVsReplace.title")}
      </div>
      <div className="grid gap-2 sm:grid-cols-3">
        <div>
          <div className="text-xs text-text-muted">
            {t("procurement.repairableDetail.repairVsReplace.repairCost")}
          </div>
          <div className="font-medium">{fmtMoney(result.repair_cost)}</div>
        </div>
        <div>
          <div className="text-xs text-text-muted">
            {t("procurement.repairableDetail.repairVsReplace.replacementCost")}
          </div>
          <div className="font-medium">{fmtMoney(result.replacement_cost)}</div>
        </div>
        <div>
          <div className="text-xs text-text-muted">
            {t("procurement.repairableDetail.repairVsReplace.recommendation")}
          </div>
          <Badge
            variant={
              result.recommendation === "REPAIR"
                ? "default"
                : result.recommendation === "REPLACE"
                  ? "destructive"
                  : "secondary"
            }
          >
            {t(`procurement.repairableDetail.repairVsReplace.${result.recommendation}`)}
          </Badge>
        </div>
      </div>
      {result.reason ? (
        <p className="mt-2 text-xs text-text-muted">
          {t(`procurement.repairableDetail.repairVsReplace.reasons.${result.reason}`, {
            defaultValue: result.reason,
          })}
        </p>
      ) : null}
    </div>
  );
}

export function RepairableDetailDialog({
  open,
  onOpenChange,
  orderId,
  onTransitioned,
  suppliers = [],
  locations = [],
  onOpenArticle,
}: RepairableDetailDialogProps) {
  const { t, i18n } = useTranslation("inventory");
  const locale = intlLocaleForLanguage(i18n.language);

  const [detail, setDetail] = useState<RepairableOrderDetail | null>(null);
  const [documents, setDocuments] = useState<InventoryDocumentLink[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [docRef, setDocRef] = useState("");
  const [docPurpose, setDocPurpose] = useState<string>("OTHER");
  const [closeRepairCost, setCloseRepairCost] = useState("");
  const [closeWarrantyUntil, setCloseWarrantyUntil] = useState("");
  const [closeWarrantyActive, setCloseWarrantyActive] = useState(false);
  const [actionKind, setActionKind] = useState<RepairableActionKind | null>(null);

  const load = useCallback(async () => {
    if (!orderId) {
      setDetail(null);
      setDocuments([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const [orderDetail, docs] = await Promise.all([
        getInventoryRepairableOrderDetail(orderId),
        listInventoryDocumentLinks("REPAIRABLE_ORDER", orderId),
      ]);
      setDetail(orderDetail);
      setDocuments(docs.length > 0 ? docs : orderDetail.document_links);
      setCloseRepairCost(
        orderDetail.order.repair_cost != null ? String(orderDetail.order.repair_cost) : "",
      );
      setCloseWarrantyUntil(orderDetail.order.warranty_until?.slice(0, 10) ?? "");
      setCloseWarrantyActive(orderDetail.order.warranty_active === 1);
    } catch (err) {
      setError(toErrorMessage(err));
      setDetail(null);
    } finally {
      setLoading(false);
    }
  }, [orderId]);

  useEffect(() => {
    if (!open || !orderId) return;
    void load();
  }, [open, orderId, load]);

  const order = detail?.order;

  const runTransition = async (
    nextStatus: string,
    extras?: Omit<
      TransitionRepairableOrderInput,
      "order_id" | "expected_row_version" | "next_status"
    >,
  ) => {
    if (!detail) return;
    setSaving(true);
    setError(null);
    try {
      await transitionInventoryRepairableOrder({
        order_id: detail.order.id,
        expected_row_version: detail.order.row_version,
        next_status: nextStatus,
        ...extras,
      });
      await load();
      onTransitioned?.();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const ACTION_TARGET: Record<RepairableActionKind, string> = {
    dispatch: "SENT_FOR_REPAIR",
    scrap: "SCRAPPED",
    cancel: "CANCELLED",
    receive: "RETURNED_FROM_REPAIR",
  };

  const confirmAction = async (payload: RepairableActionPayload) => {
    if (!actionKind) return;
    const target = ACTION_TARGET[actionKind];
    const extras: Omit<
      TransitionRepairableOrderInput,
      "order_id" | "expected_row_version" | "next_status"
    > = { ...payload };
    if (actionKind === "receive" && payload.return_location_id == null) {
      extras.return_location_id = detail?.order.return_location_id ?? null;
    }
    if (actionKind === "scrap" && payload.return_location_id == null) {
      extras.return_location_id = detail?.order.return_location_id ?? null;
    }
    setActionKind(null);
    await runTransition(target, extras);
  };

  const startSend = () => {
    if (!order) return;
    if (!order.serial_number?.trim() || order.vendor_supplier_id == null) {
      setActionKind("dispatch");
      return;
    }
    void runTransition("SENT_FOR_REPAIR");
  };

  const startReceiveBack = () => {
    if (!order) return;
    if (order.return_location_id == null) {
      setActionKind("receive");
      return;
    }
    void runTransition("RETURNED_FROM_REPAIR", {
      return_location_id: order.return_location_id,
    });
  };

  const addDocument = async () => {
    if (!detail || !docRef.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await upsertInventoryDocumentLink({
        entity_type: "REPAIRABLE_ORDER",
        entity_id: detail.order.id,
        document_ref: docRef.trim(),
        link_purpose: docPurpose,
      });
      setDocRef("");
      const docs = await listInventoryDocumentLinks("REPAIRABLE_ORDER", detail.order.id);
      setDocuments(docs);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const handleClose = async () => {
    const cost = closeRepairCost.trim() ? Number(closeRepairCost) : null;
    await runTransition("CLOSED", {
      repair_cost: cost != null && Number.isFinite(cost) ? cost : null,
      warranty_until: closeWarrantyUntil.trim() || null,
      warranty_active: closeWarrantyActive,
    });
  };

  const currentRank = order ? stepRank(order.status) : -1;

  return (
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="flex max-h-[90vh] max-w-2xl flex-col gap-0 overflow-hidden p-0">
          <DialogHeader className="shrink-0 border-b px-6 py-4">
            <DialogTitle className="flex flex-wrap items-center gap-2">
              {order ? (
                <>
                  <span className="font-mono">{formatEntityCode(order.order_code)}</span>
                  <Badge variant={order.status === "SCRAPPED" ? "destructive" : "outline"}>
                    {t(`procurement.repairableStatuses.${order.status}`, {
                      defaultValue: order.status,
                    })}
                  </Badge>
                  {order.warranty_active === 1 ? (
                    <Badge variant="secondary">
                      {t("procurement.repairableDetail.warrantyActive")}
                    </Badge>
                  ) : null}
                </>
              ) : (
                t("procurement.repairableDetail.title")
              )}
            </DialogTitle>
          </DialogHeader>

          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-4">
            {loading ? (
              <p className="text-sm text-text-muted">{t("procurement.repairableDetail.loading")}</p>
            ) : error && !detail ? (
              <p className="text-sm text-destructive">{error}</p>
            ) : !detail || !order ? (
              <p className="text-sm text-text-muted">
                {t("procurement.repairableDetail.noSelection")}
              </p>
            ) : (
              <>
                {error ? <p className="text-sm text-destructive">{error}</p> : null}

                <section className="space-y-2">
                  <h3 className="text-sm font-semibold">
                    {t("procurement.repairableDetail.sections.summary")}
                  </h3>
                  <div className="grid gap-2 rounded-md border p-3 text-sm sm:grid-cols-2">
                    {onOpenArticle ? (
                      <div>
                        <div className="text-xs text-text-muted">
                          {t("procurement.repairableDetail.fields.article")}
                        </div>
                        <button
                          type="button"
                          className="font-medium text-primary hover:underline"
                          onClick={() => {
                            onOpenChange(false);
                            onOpenArticle(order.article_id);
                          }}
                        >
                          {formatAssetLabel(order.article_code, order.article_name)}
                        </button>
                      </div>
                    ) : (
                      <Field
                        label={t("procurement.repairableDetail.fields.article")}
                        value={formatAssetLabel(order.article_code, order.article_name)}
                      />
                    )}
                    <Field
                      label={t("procurement.repairableDetail.fields.serial")}
                      value={formatOrDash(order.serial_number)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.reason")}
                      value={formatOrDash(order.reason)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.vendor")}
                      value={formatOrDash(
                        order.vendor_supplier_name?.trim() || order.vendor_supplier_code,
                      )}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.sentAt")}
                      value={formatDate(order.sent_at, locale)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.returnedAt")}
                      value={formatDate(order.returned_at, locale)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.repairCost")}
                      value={fmtMoney(order.repair_cost)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.fields.warrantyUntil")}
                      value={formatDate(order.warranty_until, locale)}
                    />
                    {order.work_order_code ? (
                      <div className="sm:col-span-2">
                        <div className="mb-1 text-xs text-text-muted">
                          {t("procurement.repairableDetail.fields.workOrder")}
                        </div>
                        <LinkedEntityBadge
                          entity="work_order"
                          code={order.work_order_code}
                          entityId={order.work_order_id}
                        />
                      </div>
                    ) : null}
                  </div>
                </section>

                <section>
                  <h3 className="mb-2 text-sm font-semibold">
                    {t("procurement.repairableDetail.sections.timeline")}
                  </h3>
                  <Timeline
                    items={REPAIR_TIMELINE_STEPS.map((step, index) => ({ step, index }))}
                    locale={locale}
                    getEntry={({ step, index }): TimelineEntry => {
                      const complete = currentRank >= 0 && index <= currentRank;
                      const event = eventForStatus(detail.state_events, step.status);
                      const isCurrent = order.status === step.status;
                      let stepState: TimelineStepState = "pending";
                      if (complete && isCurrent) stepState = "current";
                      else if (complete) stepState = "complete";
                      return {
                        id: step.key,
                        title: t(`procurement.repairableDetail.timeline.${step.key}`),
                        ...(event?.changed_at ? { timestamp: event.changed_at } : {}),
                        subtitle: event
                          ? `${formatDate(event.changed_at, locale)}${event.note ? ` — ${event.note}` : ""}`
                          : complete
                            ? t("procurement.repairableDetail.timeline.reached")
                            : t("procurement.repairableDetail.timeline.pending"),
                        stepState,
                      };
                    }}
                  />
                </section>

                <section>
                  <RepairVsReplaceCard result={detail.repair_vs_replace} />
                </section>

                <section>
                  <h3 className="mb-2 text-sm font-semibold">
                    {t("procurement.repairableDetail.sections.history")}
                  </h3>
                  <div className="grid gap-2 rounded-md border p-3 text-sm sm:grid-cols-3">
                    <Field
                      label={t("procurement.repairableDetail.history.repairCount")}
                      value={String(detail.history_stats.repair_count)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.history.avgCost")}
                      value={fmtMoney(detail.history_stats.avg_cost)}
                    />
                    <Field
                      label={t("procurement.repairableDetail.history.avgTurnaround")}
                      value={
                        detail.history_stats.avg_turnaround_days != null
                          ? t("procurement.repairableDetail.history.days", {
                              count: detail.history_stats.avg_turnaround_days,
                              days: fmtDays(detail.history_stats.avg_turnaround_days),
                            })
                          : "—"
                      }
                    />
                  </div>
                </section>

                <section className="space-y-2">
                  <h3 className="text-sm font-semibold">
                    {t("procurement.repairableDetail.sections.documents")}
                  </h3>
                  {documents.length === 0 ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.repairableDetail.documents.empty")}
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
                        <Label>{t("procurement.repairableDetail.documents.fields.ref")}</Label>
                        <Input value={docRef} onChange={(e) => setDocRef(e.target.value)} />
                      </div>
                      <div className="space-y-1">
                        <Label>{t("procurement.repairableDetail.documents.fields.purpose")}</Label>
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
                          {t("procurement.repairableDetail.documents.add")}
                        </Button>
                      </div>
                    </div>
                  </PermissionGate>
                </section>

                {order.status === "RETURNED_FROM_REPAIR" ? (
                  <section className="space-y-2 rounded-md border p-3">
                    <h3 className="text-sm font-semibold">
                      {t("procurement.repairableDetail.sections.closeFields")}
                    </h3>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <div className="space-y-1">
                        <Label>{t("procurement.repairableDetail.fields.repairCost")}</Label>
                        <Input
                          type="number"
                          min={0}
                          step="0.01"
                          value={closeRepairCost}
                          onChange={(e) => setCloseRepairCost(e.target.value)}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label>{t("procurement.repairableDetail.fields.warrantyUntil")}</Label>
                        <Input
                          type="date"
                          value={closeWarrantyUntil}
                          onChange={(e) => setCloseWarrantyUntil(e.target.value)}
                        />
                      </div>
                      <div className="flex items-center gap-2 sm:col-span-2">
                        <input
                          type="checkbox"
                          id="repair-warranty-active"
                          checked={closeWarrantyActive}
                          onChange={(e) => setCloseWarrantyActive(e.target.checked)}
                        />
                        <Label htmlFor="repair-warranty-active">
                          {t("procurement.repairableDetail.fields.warrantyActive")}
                        </Label>
                      </div>
                    </div>
                  </section>
                ) : null}
              </>
            )}
          </div>

          <DialogFooter className="shrink-0 flex-wrap gap-2 border-t px-6 py-3">
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("procurement.repairableDetail.actions.close")}
            </Button>
            {order ? (
              <PermissionGate permission={P.INV_PROCURE}>
                <>
                  <Button
                    variant="outline"
                    disabled={saving || order.status !== "REQUESTED"}
                    onClick={() => void runTransition("RELEASED")}
                  >
                    {t("procurement.repairableDetail.actions.release")}
                  </Button>
                  <Button
                    variant="outline"
                    disabled={saving || order.status !== "RELEASED"}
                    onClick={startSend}
                  >
                    {t("procurement.repairableDetail.actions.send")}
                  </Button>
                  <Button
                    variant="outline"
                    disabled={saving || order.status !== "SENT_FOR_REPAIR"}
                    onClick={startReceiveBack}
                  >
                    {t("procurement.repairableDetail.actions.receiveBack")}
                  </Button>
                  <Button
                    disabled={saving || order.status !== "RETURNED_FROM_REPAIR"}
                    onClick={() => void handleClose()}
                  >
                    {t("procurement.repairableDetail.actions.closeOrder")}
                  </Button>
                  {order.status === "REQUESTED" || order.status === "RELEASED" ? (
                    <Button
                      variant="destructive"
                      disabled={saving}
                      onClick={() => setActionKind("cancel")}
                    >
                      {t("procurement.repairableDetail.actions.cancel")}
                    </Button>
                  ) : null}
                  {order.status === "SENT_FOR_REPAIR" || order.status === "RETURNED_FROM_REPAIR" ? (
                    <Button
                      variant="destructive"
                      disabled={saving}
                      onClick={() => setActionKind("scrap")}
                    >
                      {t("procurement.repairableDetail.actions.scrap")}
                    </Button>
                  ) : null}
                </>
              </PermissionGate>
            ) : null}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <RepairableActionDialog
        open={actionKind != null}
        onOpenChange={(next) => {
          if (!next) setActionKind(null);
        }}
        kind={actionKind}
        order={order ?? null}
        suppliers={suppliers}
        locations={locations}
        saving={saving}
        onConfirm={confirmAction}
      />
    </>
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
