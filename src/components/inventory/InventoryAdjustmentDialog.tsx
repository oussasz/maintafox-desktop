import { ArrowDown, Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { StockImpactPreview } from "@/components/inventory/StockImpactPreview";
import { Button } from "@/components/ui/button";
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
import { formatSiteLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  adjustInventoryStock,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryStockBalances,
} from "@/services/inventory-service";
import { getLookupValues } from "@/services/lookup-service";
import { useInventoryStore } from "@/stores/inventory-store";
import { toErrorMessage } from "@/utils/errors";
import type { InventoryArticle, LookupValueOption, StockLocation } from "@shared/ipc-types";

export type InventoryAdjustmentType = "entry" | "exit" | "adjustment";

const MOTIF_CODES_BY_TYPE: Record<InventoryAdjustmentType, ReadonlySet<string>> = {
  entry: new Set(["ENTREE_ACHAT", "RETOUR_OT"]),
  exit: new Set(["SORTIE_OT"]),
  adjustment: new Set(["AJUSTEMENT", "INVENTAIRE"]),
};

const TYPE_STYLES: Record<
  InventoryAdjustmentType,
  { active: string; dot: string }
> = {
  entry: {
    active: "border-green-500 bg-green-50 text-green-800 ring-1 ring-green-500/30",
    dot: "bg-green-500",
  },
  exit: {
    active: "border-red-500 bg-red-50 text-red-800 ring-1 ring-red-500/30",
    dot: "bg-red-500",
  },
  adjustment: {
    active: "border-blue-500 bg-blue-50 text-blue-800 ring-1 ring-blue-500/30",
    dot: "bg-blue-500",
  },
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

function formatQty(n: number): string {
  return Number.isFinite(n) ? String(n) : "—";
}

function formatDelta(n: number): string {
  if (!Number.isFinite(n) || n === 0) return "0";
  return n > 0 ? `+${n}` : String(n);
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
  const warehouses = useInventoryStore((s) => s.warehouses);

  const [article, setArticle] = useState<InventoryArticle | null>(null);
  const [warehouseId, setWarehouseId] = useState<number>(0);
  const [locationId, setLocationId] = useState<number>(0);
  const [adjustType, setAdjustType] = useState<InventoryAdjustmentType>("entry");
  const [quantity, setQuantity] = useState<string>("");
  const [motifCode, setMotifCode] = useState<string>("");
  const [comment, setComment] = useState("");
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [motifs, setMotifs] = useState<LookupValueOption[]>([]);
  const [currentStock, setCurrentStock] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadingLocations, setLoadingLocations] = useState(false);
  const [loadingArticle, setLoadingArticle] = useState(false);

  const activeWarehouses = useMemo(
    () => warehouses.filter((w) => w.is_active === 1),
    [warehouses],
  );

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
    setCurrentStock(null);
    setError(null);
  }, [activeWarehouses, defaultLocationId, defaultWarehouseId]);

  useEffect(() => {
    if (!open) return;
    resetForm();
    setLoadingArticle(true);
    void Promise.all([
      listInventoryArticles({}),
      getLookupValues("inventory.movement_type"),
    ])
      .then(([articles, movementTypes]) => {
        setArticle(articles.find((a) => a.id === articleId) ?? null);
        setMotifs(movementTypes);
      })
      .catch((err) => setError(toErrorMessage(err)))
      .finally(() => setLoadingArticle(false));
  }, [open, articleId, resetForm]);

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
      .catch((err) => {
        if (!cancelled) setError(toErrorMessage(err));
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
      return;
    }
    let cancelled = false;
    void listInventoryStockBalances({ article_id: articleId, warehouse_id: null })
      .then((rows) => {
        if (cancelled) return;
        const balance = rows.find((b) => b.location_id === locationId);
        setCurrentStock(balance?.on_hand_qty ?? 0);
      })
      .catch((err) => {
        if (!cancelled) setError(toErrorMessage(err));
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

  const qtyNumber = Number(quantity);
  const delta = Number.isFinite(qtyNumber) ? resolveDelta(adjustType, qtyNumber) : 0;
  const afterStock =
    currentStock != null && Number.isFinite(qtyNumber) && delta !== 0
      ? currentStock + delta
      : null;

  const canSubmit =
    !saving &&
    !loadingArticle &&
    article != null &&
    warehouseId > 0 &&
    locationId > 0 &&
    motifCode.length > 0 &&
    Number.isFinite(qtyNumber) &&
    qtyNumber !== 0 &&
    (adjustType === "adjustment" || qtyNumber > 0) &&
    delta !== 0;

  const handleSubmit = async () => {
    if (!canSubmit) return;
    setSaving(true);
    setError(null);
    try {
      await adjustInventoryStock({
        article_id: articleId,
        location_id: locationId,
        delta_qty: delta,
        reason_code: motifCode,
        notes: comment.trim() || null,
        source_ref: sourceRef,
      });
      onOpenChange(false);
      onSuccess();
    } catch (err) {
      setError(toErrorMessage(err));
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
                const styles = TYPE_STYLES[type];
                return (
                  <button
                    key={type}
                    type="button"
                    onClick={() => setAdjustType(type)}
                    className={cn(
                      "flex items-center gap-2 rounded-md border px-3 py-2 text-left text-sm transition-colors",
                      selected
                        ? styles.active
                        : "border-surface-border bg-surface text-text-muted hover:bg-surface-muted",
                    )}
                  >
                    <span
                      className={cn("h-2.5 w-2.5 shrink-0 rounded-full", styles.dot)}
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
              <div className="rounded-md border border-surface-border bg-surface-muted/40 p-3 text-sm">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-text-muted">{t("adjustStock.preview.current")}</span>
                  <span className="font-medium tabular-nums">{formatQty(currentStock)}</span>
                </div>
                <div className="mt-1 flex items-center justify-between gap-3">
                  <span className="text-text-muted">{t("adjustStock.preview.adjustment")}</span>
                  <span
                    className={cn(
                      "font-medium tabular-nums",
                      delta > 0 ? "text-green-700" : delta < 0 ? "text-red-700" : "",
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
                      afterStock != null && afterStock < 0 ? "text-red-700" : "",
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
            />
          </div>

          {error ? <p className="text-sm text-destructive">{error}</p> : null}
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {tc("action.cancel")}
          </Button>
          <Button type="button" onClick={() => void handleSubmit()} disabled={!canSubmit}>
            {saving ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("adjustStock.actions.validate")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
