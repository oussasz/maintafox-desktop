/**
 * Stock to reconcile ("négatif — à rapprocher", Inventory › Controls).
 *
 * A stock movement received from another PC is applied as received; a location left below zero
 * is listed here once per article and location until a stock count is recorded there (inv.manage,
 * the variance reason and a reason of at least 8 characters) or a later entry brings it back to
 * zero or more (a goods receipt or transfer in: "régularisé par réception"). Two counts of the same
 * location recorded on different PCs without knowing each other are listed as "double comptage —
 * à vérifier", naming both, until a new count. Locations holding more reserved than on hand are
 * shown as warnings only. Nothing here can be rejected or accepted as is.
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
  type NegativeStockReview,
  type NegativeStockReviewList,
  type ReconciliationCount,
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
      await resolveNegativeStockByCount({
        article_id: counting.article_id,
        location_id: counting.location_id,
        counted_qty: counted,
        variance_reason_code: varianceReason,
        reason: reason.trim(),
      });
      setCounting(null);
      await load();
    } catch (err) {
      setSaveError(appErrorReason(err));
    } finally {
      setSaving(false);
    }
  };

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

      {open.length === 0 && list.double_counts.length === 0 ? (
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

      {includeClosed && (closed.length > 0 || list.closed_double_counts.length > 0) ? (
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
    </section>
  );
}
