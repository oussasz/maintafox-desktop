/**
 * Store-to-store transfer of one article, started from a stock line. The source article and store
 * are fixed; the quantity is capped at the available stock (on hand minus reserved: reserved stock
 * is never transferable) and a reason is required. Every refusal is a coded backend rule
 * (INV_TRANSFER_*) translated by the shared error reason.
 */
import { ArrowRightLeft } from "lucide-react";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  EntityFormDialog,
  EntityFormFooter,
  EntityFormHint,
  EntityFormSection,
} from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { appErrorReason } from "@/lib/app-error";
import { formatSiteLabel } from "@/lib/display";
import {
  type InventoryArticleRow,
  type LotStock,
  isLotTracked,
  listInventoryLocations,
  listInventoryLotStock,
  transferInventoryStock,
} from "@/services/inventory-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { InventoryStockBalance, StockLocation } from "@shared/ipc-types";

/** Same minimum as the backend rule (INV_TRANSFER_NOTE_TOO_SHORT). */
export const TRANSFER_NOTE_MIN_CHARS = 5;
const AUTO_LOT = "auto";

export interface StockTransferDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  article: InventoryArticleRow;
  /** The stock line the transfer leaves from (article + store). */
  source: InventoryStockBalance;
  /** Called after a successful transfer so the caller reloads balances and movements. */
  onTransferred: () => void;
}

export function StockTransferDialog({
  open,
  onOpenChange,
  article,
  source,
  onTransferred,
}: StockTransferDialogProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatDate } = useFormatters();
  const [stores, setStores] = useState<StockLocation[]>([]);
  const [storesFailed, setStoresFailed] = useState(false);
  const [lots, setLots] = useState<LotStock[]>([]);
  const [toId, setToId] = useState("");
  const [lotChoice, setLotChoice] = useState(AUTO_LOT);
  const [quantity, setQuantity] = useState("");
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const tracked = isLotTracked(article);
  const serial = article.requires_serial_tracking === 1;
  const unit = article.unit_symbol || article.unit_label;
  const articleId = article.id;
  const sourceLocationId = source.location_id;
  const fmt = (n: number) => formatNumber(n, { maximumFractionDigits: 3 });

  // Each opening starts blank: the person chooses the destination and writes the reason.
  useEffect(() => {
    if (!open) return;
    setToId("");
    setLotChoice(AUTO_LOT);
    setQuantity("");
    setReason("");
    setSubmitted(false);
    setError(null);
    setStoresFailed(false);
    let cancelled = false;
    void listInventoryLocations(null)
      .then((rows) => {
        if (!cancelled) setStores(rows);
      })
      .catch(() => {
        if (!cancelled) setStoresFailed(true);
      });
    setLots([]);
    if (tracked) {
      void listInventoryLotStock(articleId, sourceLocationId)
        .then((rows) => {
          if (!cancelled) setLots(rows);
        })
        .catch(() => {
          if (!cancelled) setLots([]);
        });
    }
    return () => {
      cancelled = true;
    };
  }, [open, tracked, articleId, sourceLocationId]);

  const destinations = useMemo(
    () => stores.filter((s) => s.is_active === 1 && s.id !== sourceLocationId),
    [stores, sourceLocationId],
  );
  const destination = destinations.find((s) => String(s.id) === toId) ?? null;
  const storeLabel = (s: Pick<StockLocation, "warehouse_code" | "code" | "name">) =>
    `${s.warehouse_code} / ${formatSiteLabel(s.code, s.name)}`;
  const sourceLabel = `${formatSiteLabel(source.warehouse_code, source.warehouse_name)} / ${formatSiteLabel(
    source.location_code,
    source.location_name,
  )}`;

  const chosenLot =
    lotChoice === AUTO_LOT ? null : lots.find((l) => String(l.lot_id) === lotChoice);
  const available = Math.max(source.available_qty, 0);
  const cap = chosenLot ? Math.min(available, chosenLot.quantity) : available;

  const parsed = Number(quantity.replace(",", "."));
  const quantityError = (() => {
    if (quantity.trim() === "" || !Number.isFinite(parsed) || parsed <= 0)
      return t("transfer.errors.quantityInvalid");
    if (serial && !Number.isInteger(parsed)) return t("transfer.errors.wholeUnits");
    if (parsed > cap + 1e-9) return t("transfer.errors.quantityAbove", { max: fmt(cap), unit });
    return null;
  })();
  const reasonTooShort = reason.trim().length < TRANSFER_NOTE_MIN_CHARS;
  const destinationMissing = destination === null;
  const canSubmit = !quantityError && !reasonTooShort && !destinationMissing;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!canSubmit || destination === null) return;
    setBusy(true);
    setError(null);
    try {
      await transferInventoryStock({
        article_id: articleId,
        from_location_id: sourceLocationId,
        to_location_id: destination.id,
        quantity: parsed,
        notes: reason.trim(),
        lot_id: chosenLot ? chosenLot.lot_id : null,
      });
      pushAppToast({
        title: t("transfer.done", {
          qty: fmt(parsed),
          unit,
          article: article.article_code,
          to: storeLabel(destination),
        }),
        variant: "success",
      });
      onTransferred();
      onOpenChange(false);
    } catch (err) {
      // The coded refusals (INV_TRANSFER_*) are translated by the shared error reason.
      setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  const formId = "stock-transfer-form";
  return (
    <EntityFormDialog
      open={open}
      onOpenChange={(next) => (!next && !busy ? onOpenChange(false) : undefined)}
      title={t("transfer.title")}
      description={t("transfer.description")}
      formId={formId}
      footer={
        <EntityFormFooter
          formId={formId}
          cancelLabel={t("transfer.cancel")}
          primaryLabel={
            <span className="inline-flex items-center gap-1.5">
              <ArrowRightLeft className="h-4 w-4" aria-hidden />
              {t("transfer.submit")}
            </span>
          }
          onCancel={() => onOpenChange(false)}
          cancelDisabled={busy}
          primaryLoading={busy}
        />
      }
    >
      <form id={formId} className="space-y-4" onSubmit={(e) => void submit(e)}>
        <EntityFormSection title={t("transfer.sections.source")}>
          <div className="text-sm">
            <div className="font-medium">
              {article.article_code} — {article.article_name}
            </div>
            <div className="text-text-muted">
              {t("transfer.from")}: {sourceLabel}
            </div>
            <div className="mt-1 text-xs text-text-muted" data-testid="transfer-available">
              {t("transfer.availableLine", {
                available: fmt(available),
                unit,
                onHand: fmt(source.on_hand_qty),
                reserved: fmt(source.reserved_qty),
              })}
            </div>
          </div>
        </EntityFormSection>

        <EntityFormSection title={t("transfer.sections.move")}>
          <FormField
            name="transfer-to"
            label={t("transfer.to")}
            required
            error={submitted && destinationMissing ? t("transfer.errors.toRequired") : undefined}
          >
            {storesFailed ? (
              <p role="alert" className="text-sm text-status-danger">
                {t("transfer.storesFailed")}
              </p>
            ) : destinations.length === 0 ? (
              <p className="text-sm text-text-muted">{t("transfer.noDestination")}</p>
            ) : (
              <Select value={toId} onValueChange={setToId}>
                <SelectTrigger id="transfer-to" className="h-10">
                  <SelectValue placeholder={t("transfer.toPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {destinations.map((s) => (
                    <SelectItem key={s.id} value={String(s.id)}>
                      {storeLabel(s)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </FormField>

          {tracked ? (
            <FormField name="transfer-lot" label={t("transfer.lot")}>
              <Select value={lotChoice} onValueChange={setLotChoice}>
                <SelectTrigger id="transfer-lot" className="h-10">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={AUTO_LOT}>{t("transfer.lotFefo")}</SelectItem>
                  {lots.map((l) => (
                    <SelectItem key={l.lot_id} value={String(l.lot_id)}>
                      {`${l.serial_number ?? l.lot_number ?? ""} · ${fmt(l.quantity)}${
                        l.expiry_date ? ` · ${formatDate(l.expiry_date)}` : ""
                      }`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
          ) : null}

          <FormField
            name="transfer-quantity"
            label={t("transfer.quantity", { unit })}
            required
            description={t("transfer.quantityMax", { max: fmt(cap), unit })}
            error={submitted ? (quantityError ?? undefined) : undefined}
          >
            <Input
              id="transfer-quantity"
              inputMode="decimal"
              value={quantity}
              onChange={(e) => setQuantity(e.target.value)}
            />
          </FormField>
          {serial ? (
            <EntityFormHint variant="info">{t("transfer.wholeUnitsHint")}</EntityFormHint>
          ) : null}

          <FormField
            name="transfer-reason"
            label={t("transfer.reason")}
            required
            error={
              submitted && reasonTooShort
                ? t("transfer.errors.reasonShort", { min: TRANSFER_NOTE_MIN_CHARS })
                : undefined
            }
          >
            <Textarea
              id="transfer-reason"
              rows={3}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </FormField>
        </EntityFormSection>

        {canSubmit && destination ? (
          <p className="rounded border border-surface-border bg-surface-subtle p-2.5 text-sm">
            {t("transfer.confirm", {
              qty: fmt(parsed),
              unit,
              article: article.article_name,
              from: sourceLabel,
              to: storeLabel(destination),
            })}
          </p>
        ) : null}
        {error ? (
          <p role="alert" className="text-sm text-status-danger">
            {error}
          </p>
        ) : null}
      </form>
    </EntityFormDialog>
  );
}
