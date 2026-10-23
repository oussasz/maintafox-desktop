import { ArrowDown, Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { StockImpactPreview } from "@/components/inventory/StockImpactPreview";
import { WorkOrderReferencePicker } from "@/components/inventory/WorkOrderReferencePicker";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { type Tone, mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { formatSiteLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  type InventoryArticleRow,
  type LotStock,
  adjustInventoryStock,
  isLotTracked,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryLotStock,
  listInventoryStockBalances,
} from "@/services/inventory-service";
import { getLookupValues } from "@/services/lookup-service";
import { NEGATIVE_STOCK_REASON_MIN_CHARS } from "@/services/negative-stock-service";
import { useInventoryStore } from "@/stores/inventory-store";
import type { LookupValueOption, StockLocation } from "@shared/ipc-types";

/** Lot choice of a tracked movement: a new lot (entries), FEFO (exits) or an existing lot id. */
const NEW_LOT = "new";
const AUTO_LOT = "auto";

function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export type InventoryAdjustmentType = "entry" | "exit" | "adjustment";

const MOTIF_CODES_BY_TYPE: Record<InventoryAdjustmentType, ReadonlySet<string>> = {
  entry: new Set(["ENTREE_ACHAT", "RETOUR_OT"]),
  exit: new Set(["SORTIE_OT"]),
  adjustment: new Set(["AJUSTEMENT", "INVENTAIRE"]),
};

/** Movement reasons that move parts to / from a work order: the OT must be named. */
export const WORK_ORDER_MOTIFS: ReadonlySet<string> = new Set(["SORTIE_OT", "RETOUR_OT"]);

/** Purchase entries outside a PO must carry their unit cost (feeds the weighted average). */
export const COST_REQUIRED_MOTIFS: ReadonlySet<string> = new Set(["ENTREE_ACHAT"]);

const TYPE_TONE: Record<InventoryAdjustmentType, Tone> = {
  entry: "success",
  exit: "attention",
  adjustment: "info",
};

export interface InventoryAdjustmentDialogProps {
  articleId: number;
  defaultWarehouseId?: number | null;
  defaultLocationId?: number | null;
  /** Optional business document reference (PO, WO, …). */
  sourceRef?: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}

function resolveDelta(type: InventoryAdjustmentType, qty: number): number {
  if (type === "entry") return Math.abs(qty);
  if (type === "exit") return -Math.abs(qty);
  return qty;
}

export function InventoryAdjustmentDialog({
  articleId,
  defaultWarehouseId = null,
  defaultLocationId = null,
  sourceRef = null,
  open,
  onOpenChange,
  onSuccess,
}: InventoryAdjustmentDialogProps) {
  const { t, i18n } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatNumber, formatCurrency, currencyCode, formatDate } = useFormatters();
  const warehouses = useInventoryStore((s) => s.warehouses);
  const formatQty = (n: number) => formatNumber(n, { maximumFractionDigits: 3 });
  const formatDelta = (n: number) =>
    n > 0 ? `+${formatQty(n)}` : n < 0 ? `−${formatQty(Math.abs(n))}` : formatQty(0);

  const [article, setArticle] = useState<InventoryArticleRow | null>(null);
  const [lots, setLots] = useState<LotStock[]>([]);
  const [lotChoice, setLotChoice] = useState<string>(NEW_LOT);
  const [lotNumber, setLotNumber] = useState("");
  const [lotExpiry, setLotExpiry] = useState("");
  const [postingDate, setPostingDate] = useState(todayIso());
  const [warehouseId, setWarehouseId] = useState<number>(0);
  const [locationId, setLocationId] = useState<number>(0);
  const [adjustType, setAdjustType] = useState<InventoryAdjustmentType>("entry");
  const [quantity, setQuantity] = useState<string>("");
  const [motifCode, setMotifCode] = useState<string>("");
  const [comment, setComment] = useState("");
  const [workOrderCode, setWorkOrderCode] = useState<string | null>(null);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [motifs, setMotifs] = useState<LookupValueOption[]>([]);
  const [currentStock, setCurrentStock] = useState<number | null>(null);
  /** Weighted-average unit cost at the selected location (null when never costed). */
  const [currentAvgCost, setCurrentAvgCost] = useState<number | null>(null);
  const [unitCost, setUnitCost] = useState<string>("");
  const [saving, setSaving] = useState(false);
  /** Failure to load the article, reasons, locations or balance (not save failures). */
  const [error, setError] = useState<unknown>(null);
  const [loadVersion, setLoadVersion] = useState(0);
  const [loadingLocations, setLoadingLocations] = useState(false);
  const [loadingArticle, setLoadingArticle] = useState(false);

  const activeWarehouses = useMemo(() => warehouses.filter((w) => w.is_active === 1), [warehouses]);

  const filteredMotifs = useMemo(() => {
    const allowed = MOTIF_CODES_BY_TYPE[adjustType];
    return motifs.filter((m) => m.is_active !== 0 && allowed.has(m.code));
  }, [motifs, adjustType]);

  const motifLabel = useCallback(
    (m: LookupValueOption) => {
      if (i18n.language.startsWith("fr") && m.fr_label) return m.fr_label;
      if (i18n.language.startsWith("en") && m.en_label) return m.en_label;
      return m.label || m.code;
    },
    [i18n.language],
  );

  const resetForm = useCallback(() => {
    const wh =
      defaultWarehouseId != null && activeWarehouses.some((w) => w.id === defaultWarehouseId)
        ? defaultWarehouseId
        : 0;
    setWarehouseId(wh);
    setLocationId(wh > 0 && defaultLocationId != null ? defaultLocationId : 0);
    setAdjustType("entry");
    setQuantity("");
    setMotifCode("");
    setComment("");
    setWorkOrderCode(null);
    setUnitCost("");
    setCurrentStock(null);
    setCurrentAvgCost(null);
    setError(null);
    setLotChoice(NEW_LOT);
    setLotNumber("");
    setLotExpiry("");
    setPostingDate(todayIso());
  }, [activeWarehouses, defaultLocationId, defaultWarehouseId]);

  useEffect(() => {
    if (!open) return;
    resetForm();
    setLoadingArticle(true);
    void Promise.all([listInventoryArticles({}), getLookupValues("inventory.movement_type")])
      .then(([articles, movementTypes]) => {
        setArticle(articles.find((a) => a.id === articleId) ?? null);
        setMotifs(movementTypes);
      })
      .catch((err: unknown) => setError(err))
      .finally(() => setLoadingArticle(false));
  }, [open, articleId, resetForm, loadVersion]);

  useEffect(() => {
    if (!open || warehouseId <= 0) {
      setLocations([]);
      return;
    }
    let cancelled = false;
    setLoadingLocations(true);
    void listInventoryLocations(warehouseId)
      .then((rows) => {
        if (cancelled) return;
        const active = rows.filter((l) => l.is_active === 1);
        setLocations(active);
        setLocationId((prev) => (active.some((l) => l.id === prev) ? prev : 0));
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoadingLocations(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, warehouseId]);

  useEffect(() => {
    if (!open || articleId <= 0 || locationId <= 0) {
      setCurrentStock(null);
      setCurrentAvgCost(null);
      return;
    }
    let cancelled = false;
    void listInventoryStockBalances({ article_id: articleId, warehouse_id: null })
      .then((rows) => {
        if (cancelled) return;
        const balance = rows.find((b) => b.location_id === locationId);
        setCurrentStock(balance?.on_hand_qty ?? 0);
        setCurrentAvgCost(balance?.moving_avg_unit_cost ?? null);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [open, articleId, locationId]);

  useEffect(() => {
    if (!filteredMotifs.some((m) => m.code === motifCode)) {
      setMotifCode("");
    }
  }, [filteredMotifs, motifCode]);

  const tracked = article ? isLotTracked(article) : false;
  const serialTracked = article?.requires_serial_tracking === 1;
  const expiryTracked = article?.requires_expiration === 1;

  useEffect(() => {
    if (!open || !tracked || locationId <= 0) {
      setLots([]);
      return;
    }
    let cancelled = false;
    void listInventoryLotStock(articleId, locationId)
      .then((rows) => {
        if (!cancelled) setLots(rows.filter((l) => l.quantity > 0));
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [open, tracked, articleId, locationId]);

  const qtyNumber = Number(quantity);
  const delta = Number.isFinite(qtyNumber) ? resolveDelta(adjustType, qtyNumber) : 0;
  const afterStock =
    currentStock != null && Number.isFinite(qtyNumber) && delta !== 0 ? currentStock + delta : null;

  const needsWorkOrder = WORK_ORDER_MOTIFS.has(motifCode);
  // Inbound movements other than returns from a job may carry their own unit cost; exits and
  // returns are valued at the location's weighted-average cost.
  const takesUnitCost = delta > 0 && !needsWorkOrder;
  const costRequired = takesUnitCost && COST_REQUIRED_MOTIFS.has(motifCode);
  const unitCostNumber = unitCost.trim() === "" ? null : Number(unitCost);
  const unitCostValid =
    unitCostNumber === null
      ? !costRequired
      : Number.isFinite(unitCostNumber) && unitCostNumber >= 0;

  const isEntry = delta > 0;
  // An entry at a location below zero reconciles it (CMMS Advisor): the comment is its reason,
  // required and audited (`inventory::negative_stock::check_entry_reason`).
  const reconcilesNegative = isEntry && currentStock != null && currentStock < 0;
  const entryReasonMissing =
    reconcilesNegative && comment.trim().length < NEGATIVE_STOCK_REASON_MIN_CHARS;
  // Entries default to a new lot, exits to first-expired-first-out.
  const effectiveLotChoice = isEntry
    ? lotChoice === AUTO_LOT
      ? NEW_LOT
      : lotChoice
    : lotChoice === NEW_LOT
      ? AUTO_LOT
      : lotChoice;
  const lotBlocker = !tracked
    ? null
    : isEntry && serialTracked && Math.abs(delta) !== 1
      ? t("adjustStock.lots.blockers.serialQty")
      : isEntry && effectiveLotChoice === NEW_LOT && !lotNumber.trim()
        ? serialTracked
          ? t("adjustStock.lots.blockers.serial")
          : t("adjustStock.lots.blockers.lot")
        : isEntry && effectiveLotChoice === NEW_LOT && expiryTracked && !lotExpiry
          ? t("adjustStock.lots.blockers.expiry")
          : null;
  const dateBlocker = !postingDate
    ? t("adjustStock.postingDate.blocker")
    : postingDate > todayIso()
      ? t("adjustStock.postingDate.future")
      : null;

  const canSubmit =
    lotBlocker === null &&
    dateBlocker === null &&
    !saving &&
    !loadingArticle &&
    article != null &&
    warehouseId > 0 &&
    locationId > 0 &&
    motifCode.length > 0 &&
    Number.isFinite(qtyNumber) &&
    qtyNumber !== 0 &&
    (adjustType === "adjustment" || qtyNumber > 0) &&
    delta !== 0 &&
    (!needsWorkOrder || workOrderCode !== null) &&
    (!takesUnitCost || unitCostValid) &&
    !entryReasonMissing;

  /** Why "Validate" is disabled (shown under the form). */
  const blocker =
    saving || loadingArticle
      ? null
      : warehouseId <= 0 || locationId <= 0
        ? t("adjustStock.blockers.location")
        : !Number.isFinite(qtyNumber) || qtyNumber === 0 || delta === 0
          ? t("adjustStock.blockers.quantity")
          : adjustType !== "adjustment" && qtyNumber < 0
            ? t("adjustStock.blockers.positive")
            : motifCode.length === 0
              ? t("adjustStock.blockers.motif")
              : needsWorkOrder && workOrderCode === null
                ? t("adjustStock.blockers.workOrder")
                : takesUnitCost && !unitCostValid
                  ? t("adjustStock.blockers.unitCost")
                  : entryReasonMissing
                    ? t("adjustStock.negativeLocation.blocker", {
                        count: NEGATIVE_STOCK_REASON_MIN_CHARS,
                      })
                    : (lotBlocker ?? dateBlocker);

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSaving(true);
    try {
      await adjustInventoryStock(
        {
          article_id: articleId,
          location_id: locationId,
          delta_qty: delta,
          reason_code: motifCode,
          notes: comment.trim() || null,
          // Exits to / returns from a work order carry the OT code as their source reference.
          source_ref: needsWorkOrder ? workOrderCode : sourceRef,
        },
        takesUnitCost ? unitCostNumber : null,
        {
          lotId:
            tracked && effectiveLotChoice !== NEW_LOT && effectiveLotChoice !== AUTO_LOT
              ? Number(effectiveLotChoice)
              : null,
          newLot:
            tracked && isEntry && effectiveLotChoice === NEW_LOT
              ? {
                  lot_number: serialTracked ? null : lotNumber.trim(),
                  serial_number: serialTracked ? lotNumber.trim() : null,
                  expiry_date: lotExpiry || null,
                }
              : null,
          postingDate: postingDate === todayIso() ? null : postingDate,
        },
      );
      onOpenChange(false);
      onSuccess();
    } catch {
      // Reported by the global IPC toast; the dialog stays open with the user's input.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="max-h-[90vh] max-w-md overflow-y-auto"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{t("adjustStock.title")}</DialogTitle>
          <DialogDescription>{t("adjustStock.description")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-3 py-1">
          <div className="space-y-1">
            <label className="text-xs text-text-muted">{t("adjustStock.fields.article")}</label>
            {loadingArticle ? (
              <p className="text-sm text-text-muted">{tc("app.loading")}</p>
            ) : article ? (
              <p className="text-sm font-medium">
                {article.article_name} ({article.article_code})
              </p>
            ) : (
              <p className="text-sm text-destructive">{t("adjustStock.errors.articleNotFound")}</p>
            )}
          </div>

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-warehouse">
              {t("adjustStock.fields.warehouse")}
            </label>
            <Select
              value={warehouseId > 0 ? String(warehouseId) : "__none__"}
              onValueChange={(v) => {
                setWarehouseId(v === "__none__" ? 0 : Number(v));
                setLocationId(0);
              }}
            >
              <SelectTrigger id="adjust-warehouse">
                <SelectValue placeholder={t("adjustStock.placeholders.warehouse")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__" disabled>
                  {t("adjustStock.placeholders.warehouse")}
                </SelectItem>
                {activeWarehouses.map((w) => (
                  <SelectItem key={w.id} value={String(w.id)}>
                    {formatSiteLabel(w.code, w.name)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-location">
              {t("adjustStock.fields.location")}
            </label>
            <Select
              value={locationId > 0 ? String(locationId) : "__none__"}
              onValueChange={(v) => setLocationId(v === "__none__" ? 0 : Number(v))}
              disabled={warehouseId <= 0 || loadingLocations}
            >
              <SelectTrigger id="adjust-location">
                <SelectValue
                  placeholder={
                    loadingLocations
                      ? t("adjustStock.placeholders.loadingLocations")
                      : t("adjustStock.placeholders.location")
                  }
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__" disabled>
                  {t("adjustStock.placeholders.location")}
                </SelectItem>
                {locations.map((l) => (
                  <SelectItem key={l.id} value={String(l.id)}>
                    {formatSiteLabel(l.code, l.name)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <fieldset className="space-y-2">
            <legend className="text-xs text-text-muted">{t("adjustStock.fields.type")}</legend>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              {(["entry", "exit", "adjustment"] as const).map((type) => {
                const selected = adjustType === type;
                const tone = mfTone[TYPE_TONE[type]];
                return (
                  <button
                    key={type}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => setAdjustType(type)}
                    className={cn(
                      "flex min-h-11 items-center gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      selected
                        ? cn(tone.chip, "font-medium")
                        : "border-surface-border bg-surface-1 text-text-muted hover:bg-surface-2",
                    )}
                  >
                    <span
                      className={cn("h-2.5 w-2.5 shrink-0 rounded-full", tone.dot)}
                      aria-hidden
                    />
                    {t(`adjustStock.types.${type}`)}
                  </button>
                );
              })}
            </div>
          </fieldset>

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-qty">
              {t("adjustStock.fields.quantity")}
            </label>
            <Input
              id="adjust-qty"
              type="number"
              step="0.01"
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
              placeholder={
                adjustType === "adjustment"
                  ? t("adjustStock.placeholders.quantitySigned")
                  : t("adjustStock.placeholders.quantity")
              }
            />
          </div>

          {currentStock != null && delta !== 0 && Number.isFinite(qtyNumber) ? (
            <div className="space-y-2">
              <div className="rounded-md border border-surface-border bg-surface-2/40 p-3 text-sm">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-text-muted">{t("adjustStock.preview.current")}</span>
                  <span className="font-medium tabular-nums">{formatQty(currentStock)}</span>
                </div>
                <div className="mt-1 flex items-center justify-between gap-3">
                  <span className="text-text-muted">{t("adjustStock.preview.adjustment")}</span>
                  <span
                    className={cn(
                      "font-medium tabular-nums",
                      delta > 0 ? mfTone.success.text : delta < 0 ? mfTone.attention.text : "",
                    )}
                  >
                    {formatDelta(delta)}
                  </span>
                </div>
                <div className="my-2 flex justify-center text-text-muted">
                  <ArrowDown className="h-4 w-4" aria-hidden />
                </div>
                <div className="flex items-center justify-between gap-3 border-t border-surface-border pt-2">
                  <span className="font-medium">{t("adjustStock.preview.after")}</span>
                  <span
                    className={cn(
                      "text-base font-semibold tabular-nums",
                      afterStock != null && afterStock < 0 ? mfTone.danger.text : "",
                    )}
                  >
                    {afterStock != null ? formatQty(afterStock) : "—"}
                  </span>
                </div>
              </div>
              <StockImpactPreview
                articleId={articleId}
                warehouseId={warehouseId > 0 ? warehouseId : null}
                deltaQty={delta}
                includeOpenPoQty
              />
            </div>
          ) : null}

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-motif">
              {t("adjustStock.fields.motif")}
            </label>
            <Select
              value={motifCode || "__none__"}
              onValueChange={(v) => setMotifCode(v === "__none__" ? "" : v)}
            >
              <SelectTrigger id="adjust-motif">
                <SelectValue placeholder={t("adjustStock.placeholders.motif")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none__" disabled>
                  {t("adjustStock.placeholders.motif")}
                </SelectItem>
                {filteredMotifs.map((m) => (
                  <SelectItem key={m.id} value={m.code}>
                    {motifLabel(m)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {takesUnitCost ? (
            <div className="space-y-1">
              <label className="text-xs text-text-muted" htmlFor="adjust-unit-cost">
                {costRequired
                  ? t("adjustStock.fields.unitCostRequired", { currency: currencyCode })
                  : t("adjustStock.fields.unitCost", { currency: currencyCode })}
              </label>
              <Input
                id="adjust-unit-cost"
                type="number"
                min={0}
                step="0.01"
                value={unitCost}
                onChange={(e) => setUnitCost(e.target.value)}
                aria-describedby="adjust-unit-cost-hint"
              />
              <p id="adjust-unit-cost-hint" className="text-xs text-text-muted">
                {currentAvgCost != null
                  ? t("adjustStock.cost.entryHint", { average: formatCurrency(currentAvgCost) })
                  : t("adjustStock.cost.entryHintNoAverage")}
              </p>
            </div>
          ) : delta !== 0 && locationId > 0 ? (
            <p className="text-xs text-text-muted">
              {currentAvgCost != null
                ? t("adjustStock.cost.valuedAtAverage", {
                    average: formatCurrency(currentAvgCost),
                    value: formatCurrency(Math.abs(delta) * currentAvgCost),
                  })
                : t("adjustStock.cost.notValued")}
            </p>
          ) : null}

          {tracked && delta !== 0 && locationId > 0 ? (
            <fieldset className="space-y-2 rounded-md border border-dashed p-3">
              <legend className="px-1 text-xs font-medium">
                {serialTracked ? t("adjustStock.lots.serialTitle") : t("adjustStock.lots.title")}
              </legend>
              <div className="space-y-1">
                <label className="text-xs text-text-muted" htmlFor="adjust-lot">
                  {t("adjustStock.lots.choice")}
                </label>
                <Select value={effectiveLotChoice} onValueChange={(v) => v && setLotChoice(v)}>
                  <SelectTrigger id="adjust-lot" className="h-11">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {isEntry ? (
                      <SelectItem value={NEW_LOT}>
                        {serialTracked
                          ? t("adjustStock.lots.newSerial")
                          : t("adjustStock.lots.newLot")}
                      </SelectItem>
                    ) : (
                      <SelectItem value={AUTO_LOT}>{t("adjustStock.lots.fefo")}</SelectItem>
                    )}
                    {lots.map((l) => (
                      <SelectItem key={l.lot_id} value={String(l.lot_id)}>
                        {(l.serial_number ?? l.lot_number ?? "") +
                          ` · ${formatQty(l.quantity)}` +
                          (l.expiry_date ? ` · ${formatDate(l.expiry_date)}` : "")}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              {isEntry && effectiveLotChoice === NEW_LOT ? (
                <div className="grid gap-2 sm:grid-cols-2">
                  <div className="space-y-1">
                    <label className="text-xs text-text-muted" htmlFor="adjust-lot-number">
                      {serialTracked
                        ? t("adjustStock.lots.serialNumber")
                        : t("adjustStock.lots.lotNumber")}
                    </label>
                    <Input
                      id="adjust-lot-number"
                      className="h-11 font-mono"
                      value={lotNumber}
                      onChange={(e) => setLotNumber(e.target.value)}
                    />
                  </div>
                  <div className="space-y-1">
                    <label className="text-xs text-text-muted" htmlFor="adjust-lot-expiry">
                      {expiryTracked
                        ? t("adjustStock.lots.expiry")
                        : t("adjustStock.lots.expiryOptional")}
                    </label>
                    <DateField
                      id="adjust-lot-expiry"
                      className="h-11"
                      value={lotExpiry}
                      onChange={(v) => setLotExpiry(v)}
                    />
                  </div>
                </div>
              ) : null}
            </fieldset>
          ) : null}

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-posting-date">
              {t("adjustStock.postingDate.label")}
            </label>
            <DateField
              id="adjust-posting-date"
              className="h-11"
              max={todayIso()}
              value={postingDate}
              onChange={(v) => setPostingDate(v)}
              aria-describedby="adjust-posting-date-hint"
            />
            <p id="adjust-posting-date-hint" className="text-xs text-text-muted">
              {t("adjustStock.postingDate.hint")}
            </p>
          </div>

          {needsWorkOrder ? (
            <div className="space-y-1">
              <label className="text-xs text-text-muted" htmlFor="adjust-work-order">
                {t("adjustStock.fields.workOrder")}
              </label>
              <WorkOrderReferencePicker
                id="adjust-work-order"
                value={workOrderCode}
                onChange={setWorkOrderCode}
                disabled={saving}
              />
            </div>
          ) : null}

          <div className="space-y-1">
            <label className="text-xs text-text-muted" htmlFor="adjust-comment">
              {t("adjustStock.fields.comment")}
            </label>
            <Textarea
              id="adjust-comment"
              rows={3}
              value={comment}
              onChange={(e) => setComment(e.target.value)}
              placeholder={t("adjustStock.placeholders.comment")}
              aria-describedby={reconcilesNegative ? "adjust-negative-hint" : undefined}
            />
            {reconcilesNegative ? (
              <p id="adjust-negative-hint" className={cn("text-xs", mfTone.warning.text)}>
                {t("adjustStock.negativeLocation.hint", { count: NEGATIVE_STOCK_REASON_MIN_CHARS })}
              </p>
            ) : null}
          </div>

          {error ? (
            <ErrorState
              error={error}
              title={t("adjustStock.errors.loadFailed")}
              onRetry={() => setLoadVersion((v) => v + 1)}
            />
          ) : null}
          {blocker ? (
            <p id="adjust-blocker" role="status" className="text-xs text-text-muted">
              {blocker}
            </p>
          ) : null}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {tc("action.cancel")}
          </Button>
          <Button
            type="button"
            className="h-11"
            aria-describedby={blocker ? "adjust-blocker" : undefined}
            onClick={() => void handleSubmit()}
            disabled={!canSubmit}
          >
            {saving ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("adjustStock.actions.validate")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
