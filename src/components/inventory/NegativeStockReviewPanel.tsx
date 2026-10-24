/**
 * Stock to reconcile ("négatif — à rapprocher", Inventory › Controls).
 *
 * A stock movement received from another PC is applied as received; a location left below zero
 * is listed here once per article and location until a stock count is recorded there (inv.manage,
 * the variance reason and a reason of at least 8 characters) or a later entry brings it back to
 * zero or more (a goods receipt or transfer in: "régularisé par réception"). Two counts of the same
 * location recorded on different PCs without knowing each other are listed as "double comptage —
 * à vérifier", naming both, until a new count; a count whose movement is in the ledger twice (the
 * same count posted on two PCs by older versions) as "comptage passé deux fois — à vérifier",
 * naming the session and both movements, until a new count; a reservation issued, or a PO line
 * received, beyond its quantity by several PCs as "sur-sortie / sur-réception — à rapprocher",
 * naming the work order or PO and every movement, until a count made on a PC holding them all
 * (from this list, a count session or an inventory campaign) or, for an over-issue, a return of
 * the reservation back to its reserved quantity ("régularisé par retour"); closed ones name that
 * return, or the count's session, counter and approver. Locations holding more reserved than on
 * hand are shown as warnings only. Nothing here can be rejected or accepted as is.
 */
import { AlertTriangle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { useLookupOptions } from "@/components/inventory/use-lookup-options";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfAlert } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorReason } from "@/lib/app-error";
import {
  NEGATIVE_STOCK_REASON_MIN_CHARS,
  type DoublePostedMovement,
  type NegativeStockReview,
  type NegativeStockReviewList,
  type OverQuantityMovement,
  type OverQuantityReview,
  type PendingReconciliationCount,
  type ReconciliationCount,
  approvePendingNegativeStockCount,
  listNegativeStockReviews,
  resolveNegativeStockByCount,
} from "@/services/negative-stock-service";
import { P } from "@shared/rbac/permissions.generated";

/** The article and location a count is recorded for (a negative item or a double count). */
type CountTarget = Pick<
  NegativeStockReview,
  | "article_id"
  | "location_id"
  | "article_code"
  | "article_name"
  | "warehouse_code"
  | "location_code"
  | "on_hand_qty"
>;

export interface NegativeStockReviewPanelProps {
  /** Bumped by the page refresh button to reload the list. */
  reloadToken?: number;
}

export function NegativeStockReviewPanel({ reloadToken = 0 }: NegativeStockReviewPanelProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatDateTime, formatNumber } = useFormatters();
  const varianceReasons = useLookupOptions("inventory.variance_reason");
  const [includeClosed, setIncludeClosed] = useState(false);
  const [list, setList] = useState<NegativeStockReviewList | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [counting, setCounting] = useState<CountTarget | null>(null);
  const [countedQty, setCountedQty] = useState("");
  const [varianceReason, setVarianceReason] = useState("");
  const [reason, setReason] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  /** The count waiting for approval being approved, with the approver's note. */
  const [approving, setApproving] = useState<PendingReconciliationCount | null>(null);
  const [approvalNote, setApprovalNote] = useState("");
  /** A count just recorded above the tolerance: it waits for a second person. */
  const [countWaiting, setCountWaiting] = useState(false);

  const qty = useCallback(
    (value: number) => formatNumber(value, { maximumFractionDigits: 3 }),
    [formatNumber],
  );
  const movementLabel = useCallback(
    (code: string | null) =>
      code ? t(`detail.activity.types.movement.${code}`, { defaultValue: code }) : "—",
    [t],
  );

  const load = useCallback(async () => {
    try {
      setList(await listNegativeStockReviews(includeClosed));
      setLoadError(null);
    } catch (err) {
      setLoadError(err);
    }
  }, [includeClosed]);

  useEffect(() => {
    void load();
  }, [load, reloadToken]);

  const countLine = (c: ReconciliationCount) =>
    t("negativeStock.doubleCount.countLine", {
      who: c.counted_by ?? t("negativeStock.doubleCount.unknownUser"),
      path: t(`negativeStock.doubleCount.path.${c.path}`, { session: c.session_code }),
      device: c.device_code || "—",
      date: c.counted_at ? formatDateTime(c.counted_at) : "—",
      counted: qty(c.counted_qty),
      variance: qty(c.variance_qty),
    });

  const movementLine = (m: DoublePostedMovement) =>
    t(
      m.device_code
        ? "negativeStock.doublePosting.movementLineDevice"
        : "negativeStock.doublePosting.movementLine",
      {
        who: m.posted_by ?? t("negativeStock.doubleCount.unknownUser"),
        device: m.device_code ?? "",
        date: m.posted_at ? formatDateTime(m.posted_at) : "—",
        quantity: qty(m.quantity),
      },
    );

  const overMovementLine = (m: OverQuantityMovement) =>
    t(
      m.document_ref
        ? "negativeStock.overQuantity.movementLineRef"
        : "negativeStock.overQuantity.movementLine",
      {
        movement: movementLabel(m.movement_type),
        ref: m.document_ref ?? "",
        who: m.posted_by ?? t("negativeStock.doubleCount.unknownUser"),
        date: m.posted_at ? formatDateTime(m.posted_at) : "—",
        quantity: qty(m.quantity),
      },
    );

  /** The work order or reservation (over-issue), the purchase order (over-receipt). */
  const overDocument = (item: OverQuantityReview) => {
    if (item.kind === "over_receipt") return item.purchase_order_number ?? "—";
    if (item.work_order_code) {
      return t("negativeStock.overQuantity.documentWorkOrder", { code: item.work_order_code });
    }
    if (item.reservation_ref) {
      return t("negativeStock.overQuantity.documentReservation", { ref: item.reservation_ref });
    }
    return t("negativeStock.overQuantity.documentNone");
  };

  /**
   * The count that closed an item (over-issue, over-receipt, count posted twice): its session,
   * path, who counted, when, and who approved its variance when it had one.
   */
  const countClosure = (by: ReconciliationCount | null) => {
    const unknown = t("negativeStock.doubleCount.unknownUser");
    return t(
      by?.approved_by
        ? "negativeStock.overQuantity.closedByCountApproved"
        : "negativeStock.overQuantity.closedByCount",
      {
        session: by?.session_code ?? "—",
        path: by ? t(`negativeStock.overQuantity.closedPath.${by.path}`) : "",
        who: by?.counted_by ?? unknown,
        date: by?.counted_at ? formatDateTime(by.counted_at) : "—",
        approver: by?.approved_by ?? "",
      },
    );
  };

  /** How a closed over-issue or over-receipt was closed: the return, or the count. */
  const overClosure = (item: OverQuantityReview) => {
    const back = item.closed_by_return;
    if (item.closure_kind === "return" && back) {
      return t("negativeStock.overQuantity.closedByReturn", {
        quantity: qty(back.quantity),
        who: back.posted_by ?? t("negativeStock.doubleCount.unknownUser"),
        date: back.posted_at ? formatDateTime(back.posted_at) : "—",
      });
    }
    return countClosure(item.closed_by);
  };

  const openCount = (item: CountTarget) => {
    setCounting(item);
    setCountedQty("");
    setVarianceReason("");
    setReason("");
    setSaveError(null);
  };

  const counted = countedQty.trim() === "" ? Number.NaN : Number(countedQty);
  const countValid = Number.isFinite(counted) && counted >= 0;
  const reasonValid = reason.trim().length >= NEGATIVE_STOCK_REASON_MIN_CHARS;
  const canSave = countValid && reasonValid && varianceReason !== "" && !saving;

  const recordCount = async () => {
    if (!counting || !canSave) return;
    setSaving(true);
    setSaveError(null);
    try {
      const resolution = await resolveNegativeStockByCount({
        article_id: counting.article_id,
        location_id: counting.location_id,
        counted_qty: counted,
        variance_reason_code: varianceReason,
        reason: reason.trim(),
      });
      setCountWaiting(resolution.pending_approval);
      setCounting(null);
      await load();
    } catch (err) {
      setSaveError(appErrorReason(err));
    } finally {
      setSaving(false);
    }
  };

  const approvalValid = approvalNote.trim().length >= NEGATIVE_STOCK_REASON_MIN_CHARS;
  const approvePending = async () => {
    if (!approving || !approvalValid || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      await approvePendingNegativeStockCount({
        line_id: approving.line_id,
        expected_row_version: approving.row_version,
        reason: approvalNote.trim(),
      });
      setApproving(null);
      setCountWaiting(false);
      await load();
    } catch (err) {
      setSaveError(appErrorReason(err));
    } finally {
      setSaving(false);
    }
  };

  /** "Comptage en attente d'approbation" on an item whose article and location has one. */
  const pendingChip = (item: Pick<CountTarget, "article_id" | "location_id">) =>
    list?.pending_counts.some(
      (p) => p.article_id === item.article_id && p.location_id === item.location_id,
    ) ? (
      <StatusChip label={t("negativeStock.pending.flag")} tone="info" />
    ) : null;

  if (loadError && !list) {
    return (
      <ErrorState
        error={loadError}
        title={t("negativeStock.loadError")}
        onRetry={() => void load()}
      />
    );
  }
  if (!list) return <LoadingState label={t("negativeStock.loading")} />;

  const open = list.items.filter((i) => i.status === "open");
  const closed = list.items.filter((i) => i.status === "closed");
  return (
    <section
      className="rounded-lg border border-surface-border bg-surface-1 p-4"
      aria-labelledby="inv-negative-stock-title"
    >
      <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 id="inv-negative-stock-title" className="text-sm font-semibold">
            {t("negativeStock.title")}
          </h3>
          <p className="text-xs text-text-muted">{t("negativeStock.description")}</p>
        </div>
        <label className="flex items-center gap-2 text-xs text-text-muted">
          <input
            type="checkbox"
            checked={includeClosed}
            onChange={(e) => setIncludeClosed(e.target.checked)}
          />
          {t("negativeStock.showClosed")}
        </label>
      </div>

      {countWaiting ? (
        <p className={`${mfAlert.info} mb-2`} role="status">
          {t("negativeStock.pending.recorded")}
        </p>
      ) : null}

      {list.pending_counts.length > 0 ? (
        <ul className="mb-2 space-y-2" aria-label={t("negativeStock.pending.list")}>
          {list.pending_counts.map((p) => (
            <li
              key={p.line_id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-primary/20 bg-primary-bg/60 px-3 py-2"
            >
              <div className="min-w-0 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip label={t("negativeStock.pending.flag")} tone="info" />
                  <span className="font-medium">
                    {p.article_code} — {p.article_name}
                  </span>
                  <span className="text-text-muted">
                    {p.warehouse_code}/{p.location_code}
                  </span>
                </div>
                <p className="mt-1 text-xs text-text-muted">
                  {t("negativeStock.pending.line", {
                    who: p.counted_by ?? t("negativeStock.doubleCount.unknownUser"),
                    date: formatDateTime(p.counted_at),
                    session: p.session_code,
                    system: qty(p.system_qty),
                    counted: qty(p.counted_qty),
                    variance: qty(p.variance_qty),
                  })}
                </p>
                <p className="mt-1 text-xs text-text-muted">{t("negativeStock.pending.rule")}</p>
              </div>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button
                  variant="outline"
                  className="h-11"
                  onClick={() => {
                    setApproving(p);
                    setApprovalNote("");
                    setSaveError(null);
                  }}
                >
                  {t("negativeStock.pending.approve")}
                </Button>
              </PermissionGate>
            </li>
          ))}
        </ul>
      ) : null}

      {open.length === 0 &&
      list.double_counts.length === 0 &&
      list.double_postings.length === 0 &&
      list.over_quantities.length === 0 ? (
        <p className="text-sm text-text-muted">{t("negativeStock.noneOpen")}</p>
      ) : (
        <ul className="space-y-2" aria-label={t("negativeStock.openList")}>
          {open.map((item) => (
            <li
              key={item.id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-status-danger/30 bg-status-danger/5 px-3 py-2"
            >
              <div className="min-w-0 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip label={t("negativeStock.flag")} tone="danger" icon={AlertTriangle} />
                  {pendingChip(item)}
                  <span className="font-medium">
                    {item.article_code} — {item.article_name}
                  </span>
                  <span className="text-text-muted">
                    {item.warehouse_code}/{item.location_code}
                  </span>
                </div>
                <p className="mt-1 text-xs text-text-muted">
                  {t("negativeStock.balance", {
                    onHand: qty(item.on_hand_qty),
                    reserved: qty(item.reserved_qty),
                    available: qty(item.available_qty),
                  })}{" "}
                  ·{" "}
                  {t("negativeStock.openedAt", {
                    date: formatDateTime(item.opened_at),
                    movement: movementLabel(item.opening_movement_type),
                  })}
                </p>
              </div>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button className="h-11" onClick={() => openCount(item)}>
                  {t("negativeStock.recordCount")}
                </Button>
              </PermissionGate>
            </li>
          ))}
        </ul>
      )}

      {list.double_counts.length > 0 ? (
        <ul className="mt-2 space-y-2" aria-label={t("negativeStock.doubleCount.list")}>
          {list.double_counts.map((item) => (
            <li
              key={`${item.article_id}-${item.location_id}`}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-status-warning/35 bg-status-warning/10 px-3 py-2"
            >
              <div className="min-w-0 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={t("negativeStock.doubleCount.flag")}
                    tone="warning"
                    icon={AlertTriangle}
                  />
                  {pendingChip(item)}
                  <span className="font-medium">
                    {item.article_code} — {item.article_name}
                  </span>
                  <span className="text-text-muted">
                    {item.warehouse_code}/{item.location_code}
                  </span>
                </div>
                <p className="mt-1 text-xs text-text-muted">
                  {t("negativeStock.doubleCount.explain", { onHand: qty(item.on_hand_qty) })}
                </p>
                <ul className="mt-1 list-disc ps-5 text-xs text-text-secondary">
                  {item.counts.map((c) => (
                    <li key={c.count_sync_id}>{countLine(c)}</li>
                  ))}
                </ul>
              </div>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button className="h-11" onClick={() => openCount(item)}>
                  {t("negativeStock.recordCount")}
                </Button>
              </PermissionGate>
            </li>
          ))}
        </ul>
      ) : null}

      {list.double_postings.length > 0 ? (
        <ul className="mt-2 space-y-2" aria-label={t("negativeStock.doublePosting.list")}>
          {list.double_postings.map((item) => (
            <li
              key={`${item.article_id}-${item.location_id}-${item.session_code}-${item.movement_type}`}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-status-warning/35 bg-status-warning/10 px-3 py-2"
            >
              <div className="min-w-0 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={t(`negativeStock.doublePosting.flagKind.${item.kind}`)}
                    tone="warning"
                    icon={AlertTriangle}
                  />
                  {pendingChip(item)}
                  <span className="font-medium">
                    {item.article_code} — {item.article_name}
                  </span>
                  <span className="text-text-muted">
                    {item.warehouse_code}/{item.location_code}
                  </span>
                </div>
                <p className="mt-1 text-xs text-text-muted">
                  {t(`negativeStock.doublePosting.explainKind.${item.kind}.${item.movement_type}`, {
                    session: item.session_code,
                    variance: qty(item.variance_qty),
                    excess: qty(item.excess_qty),
                    onHand: qty(item.on_hand_qty),
                  })}
                </p>
                <ul className="mt-1 list-disc ps-5 text-xs text-text-secondary">
                  {item.movements.map((m, i) => (
                    <li key={`${m.movement_sync_id}-${i}`}>{movementLine(m)}</li>
                  ))}
                </ul>
              </div>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button className="h-11" onClick={() => openCount(item)}>
                  {t("negativeStock.recordCount")}
                </Button>
              </PermissionGate>
            </li>
          ))}
        </ul>
      ) : null}

      {list.over_quantities.length > 0 ? (
        <ul className="mt-2 space-y-2" aria-label={t("negativeStock.overQuantity.list")}>
          {list.over_quantities.map((item, i) => (
            <li
              key={`${item.kind}-${item.article_id}-${item.location_id}-${item.movements[0]?.movement_sync_id ?? ""}-${i}`}
              className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-status-warning/35 bg-status-warning/10 px-3 py-2"
            >
              <div className="min-w-0 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={t(`negativeStock.overQuantity.flagKind.${item.kind}`)}
                    tone="warning"
                    icon={AlertTriangle}
                  />
                  {pendingChip(item)}
                  <span className="font-medium">
                    {item.article_code} — {item.article_name}
                  </span>
                  <span className="text-text-muted">
                    {item.warehouse_code}/{item.location_code}
                  </span>
                </div>
                <p className="mt-1 text-xs text-text-muted">
                  {t(`negativeStock.overQuantity.explainKind.${item.kind}`, {
                    document: overDocument(item),
                    expected: qty(item.expected_qty),
                    actual: qty(item.actual_qty),
                    excess: qty(item.excess_qty),
                    onHand: qty(item.on_hand_qty),
                  })}
                </p>
                {item.kind === "over_receipt" ? (
                  <p className="mt-1 text-xs text-text-muted">
                    {t("negativeStock.overQuantity.receiptRejected")}
                  </p>
                ) : null}
                <ul className="mt-1 list-disc ps-5 text-xs text-text-secondary">
                  {item.movements.map((m) => (
                    <li key={m.movement_sync_id}>{overMovementLine(m)}</li>
                  ))}
                </ul>
              </div>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button className="h-11" onClick={() => openCount(item)}>
                  {t("negativeStock.recordCount")}
                </Button>
              </PermissionGate>
            </li>
          ))}
        </ul>
      ) : null}

      {list.over_reservations.length > 0 ? (
        <div className={`${mfAlert.warning} mt-3`} role="status">
          <p className="font-medium">{t("negativeStock.overReservedTitle")}</p>
          <ul className="mt-1 space-y-0.5 text-xs">
            {list.over_reservations.map((w) => (
              <li key={`${w.article_id}-${w.location_id}`}>
                {t("negativeStock.overReservedLine", {
                  article: `${w.article_code} — ${w.article_name}`,
                  location: `${w.warehouse_code}/${w.location_code}`,
                  onHand: qty(w.on_hand_qty),
                  reserved: qty(w.reserved_qty),
                })}
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      {includeClosed &&
      (closed.length > 0 ||
        list.closed_double_counts.length > 0 ||
        list.closed_double_postings.length > 0 ||
        list.closed_over_quantities.length > 0) ? (
        <ul className="mt-3 space-y-1" aria-label={t("negativeStock.closedList")}>
          {closed.map((item) => (
            <li key={item.id} className="text-xs text-text-muted">
              <span className="font-medium text-text-secondary">
                {item.article_code} — {item.warehouse_code}/{item.location_code}
              </span>{" "}
              ·{" "}
              {t(`negativeStock.closure.${item.closure_kind ?? "other"}`, {
                date: item.closed_at ? formatDateTime(item.closed_at) : "—",
                movement: movementLabel(item.closing_movement_type),
              })}
            </li>
          ))}
          {list.closed_double_counts.map((item) => (
            <li
              key={`${item.article_id}-${item.location_id}-${item.closed_by_reversal?.count_sync_id ?? ""}`}
              className="text-xs text-text-muted"
            >
              <span className="font-medium text-text-secondary">
                {item.article_code} — {item.warehouse_code}/{item.location_code}
              </span>{" "}
              · {t("negativeStock.doubleCount.flag")} ·{" "}
              {t("negativeStock.doubleCount.closedByReversal", {
                session: item.closed_by_reversal?.session_code ?? "—",
                who:
                  item.closed_by_reversal?.reversed_by ??
                  t("negativeStock.doubleCount.unknownUser"),
                date: item.closed_by_reversal?.reversed_at
                  ? formatDateTime(item.closed_by_reversal.reversed_at)
                  : "—",
              })}
            </li>
          ))}
          {list.closed_double_postings.map((item) => (
            <li
              key={`${item.article_id}-${item.location_id}-${item.session_code}-${item.movement_type}`}
              className="text-xs text-text-muted"
            >
              <span className="font-medium text-text-secondary">
                {item.article_code} — {item.warehouse_code}/{item.location_code}
              </span>{" "}
              · {t(`negativeStock.doublePosting.flagKind.${item.kind}`)} ·{" "}
              {countClosure(item.closed_by)}
            </li>
          ))}
          {list.closed_over_quantities.map((item, i) => (
            <li
              key={`${item.kind}-${item.article_id}-${item.location_id}-${item.movements[0]?.movement_sync_id ?? ""}-${i}`}
              className="text-xs text-text-muted"
            >
              <span className="font-medium text-text-secondary">
                {item.article_code} — {item.warehouse_code}/{item.location_code}
              </span>{" "}
              · {t(`negativeStock.overQuantity.flagKind.${item.kind}`)} · {overDocument(item)} ·{" "}
              {overClosure(item)}
            </li>
          ))}
        </ul>
      ) : null}

      <Dialog
        open={counting !== null}
        onOpenChange={(isOpen) => {
          if (!isOpen && !saving) setCounting(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("negativeStock.dialog.title")}</DialogTitle>
            <DialogDescription>
              {counting
                ? t("negativeStock.dialog.description", {
                    article: `${counting.article_code} — ${counting.article_name}`,
                    location: `${counting.warehouse_code}/${counting.location_code}`,
                    onHand: qty(counting.on_hand_qty),
                  })
                : ""}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="inv-neg-counted">{t("negativeStock.dialog.counted")}</Label>
              <DecimalField
                id="inv-neg-counted"
                decimals={3}
                min={0}
                value={countedQty.trim() === "" ? null : Number(countedQty)}
                onValueChange={(v) => setCountedQty(v == null ? "" : String(v))}
                aria-invalid={countedQty !== "" && !countValid}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-neg-variance">{t("negativeStock.dialog.varianceReason")}</Label>
              <Select value={varianceReason} onValueChange={setVarianceReason}>
                <SelectTrigger id="inv-neg-variance">
                  <SelectValue placeholder={t("negativeStock.dialog.varianceReasonPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {varianceReasons.options.map((option) => (
                    <SelectItem key={option.id} value={option.code}>
                      {varianceReasons.labelOf(option.code)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-neg-reason">{t("negativeStock.dialog.reason")}</Label>
              <Textarea
                id="inv-neg-reason"
                rows={3}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder={t("negativeStock.dialog.reasonPlaceholder", {
                  count: NEGATIVE_STOCK_REASON_MIN_CHARS,
                })}
              />
            </div>
            {saveError ? (
              <p className={mfAlert.danger} role="alert">
                {saveError}
              </p>
            ) : null}
          </div>
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={saving}
              onClick={() => setCounting(null)}
            >
              {tc("action.cancel")}
            </Button>
            <Button className="h-11" disabled={!canSave} onClick={() => void recordCount()}>
              {t("negativeStock.dialog.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* A count waiting for approval: a second person approves it with a note (four-eyes). */}
      <Dialog
        open={approving !== null}
        onOpenChange={(isOpen) => {
          if (!isOpen && !saving) setApproving(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("negativeStock.pending.dialogTitle")}</DialogTitle>
            <DialogDescription>
              {approving
                ? t("negativeStock.pending.dialogDescription", {
                    article: `${approving.article_code} — ${approving.article_name}`,
                    location: `${approving.warehouse_code}/${approving.location_code}`,
                    who: approving.counted_by ?? t("negativeStock.doubleCount.unknownUser"),
                    counted: qty(approving.counted_qty),
                    variance: qty(approving.variance_qty),
                  })
                : ""}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="inv-neg-approval-note">{t("negativeStock.pending.note")}</Label>
              <Textarea
                id="inv-neg-approval-note"
                rows={3}
                value={approvalNote}
                onChange={(e) => setApprovalNote(e.target.value)}
                placeholder={t("negativeStock.dialog.reasonPlaceholder", {
                  count: NEGATIVE_STOCK_REASON_MIN_CHARS,
                })}
              />
            </div>
            {saveError ? (
              <p className={mfAlert.danger} role="alert">
                {saveError}
              </p>
            ) : null}
          </div>
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={saving}
              onClick={() => setApproving(null)}
            >
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={!approvalValid || saving}
              onClick={() => void approvePending()}
            >
              {t("negativeStock.pending.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}
