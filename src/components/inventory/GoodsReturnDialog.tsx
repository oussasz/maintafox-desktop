/**
 * GoodsReturnDialog.tsx
 *
 * Return of a received part to its supplier (L3-05): a part found non-conforming after reception
 * leaves the stock and the reception line records the quantity and the reason. At most the
 * accepted quantity not yet returned can go back; a lot- or serial-tracked article names the lot
 * sent back (listed from the stock held at the reception location).
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { DecimalField } from "@/components/ui/decimal-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  type LotStock,
  listInventoryLotStock,
  returnInventoryGoodsToSupplier,
} from "@/services/inventory-service";
import type { GoodsReceiptLine } from "@shared/ipc-types";

const MIN_REASON_LENGTH = 4;

export interface GoodsReturnDialogProps {
  /** The reception line to return from; null closes the dialog. */
  line: GoodsReceiptLine | null;
  onOpenChange: (open: boolean) => void;
  /** Called with the updated reception line once the return is posted. */
  onReturned: (line: GoodsReceiptLine) => void;
}

export function GoodsReturnDialog({ line, onOpenChange, onReturned }: GoodsReturnDialogProps) {
  const { t } = useTranslation("inventory");
  const [quantity, setQuantity] = useState<number | null>(null);
  const [reason, setReason] = useState("");
  const [lotId, setLotId] = useState("");
  const [lots, setLots] = useState<LotStock[]>([]);
  const [saving, setSaving] = useState(false);

  const returnable = line ? Math.max(0, line.accepted_qty - line.returned_qty) : 0;

  useEffect(() => {
    if (!line) return;
    setQuantity(null);
    setReason("");
    setLotId("");
    setLots([]);
    let cancelled = false;
    void listInventoryLotStock(line.article_id, line.location_id)
      .then((rows) => {
        if (!cancelled) setLots(rows);
      })
      .catch(() => {
        // Reported by the global toast; the return then fails with the missing-lot refusal.
      });
    return () => {
      cancelled = true;
    };
  }, [line]);

  const lotRequired = lots.length > 0;
  const quantityValid = quantity != null && quantity > 0 && quantity <= returnable + 1e-9;
  const valid =
    line != null &&
    quantityValid &&
    reason.trim().length >= MIN_REASON_LENGTH &&
    (!lotRequired || lotId !== "");

  const submit = async () => {
    if (!line || !valid || quantity == null) return;
    setSaving(true);
    try {
      const updated = await returnInventoryGoodsToSupplier({
        goods_receipt_line_id: line.id,
        quantity,
        reason: reason.trim(),
        lot_id: lotId === "" ? null : Number(lotId),
      });
      onReturned(updated);
      onOpenChange(false);
    } catch {
      // The global IPC toast reports the refusal; the dialog stays open with what was typed.
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={line != null} onOpenChange={(open) => !saving && onOpenChange(open)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("procurement.goodsReceipts.return.title")}</DialogTitle>
          <DialogDescription>
            {t("procurement.goodsReceipts.return.description", {
              article: line ? `${line.article_code} — ${line.article_name}` : "",
              max: returnable,
            })}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3">
          <FormField
            name="gr-return-qty"
            label={t("procurement.goodsReceipts.return.quantity")}
            required
            error={
              quantity != null && !quantityValid
                ? t("procurement.goodsReceipts.return.quantityInvalid", { max: returnable })
                : undefined
            }
          >
            <DecimalField
              id="gr-return-qty"
              decimals={3}
              min={0}
              value={quantity}
              onValueChange={setQuantity}
            />
          </FormField>
          {lotRequired ? (
            <FormField
              name="gr-return-lot"
              label={t("procurement.goodsReceipts.return.lot")}
              required
            >
              <Select value={lotId} onValueChange={setLotId}>
                <SelectTrigger id="gr-return-lot">
                  <SelectValue placeholder={t("procurement.goodsReceipts.return.lotPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {lots.map((lot) => (
                    <SelectItem key={lot.lot_id} value={String(lot.lot_id)}>
                      {`${lot.serial_number ?? lot.lot_number ?? lot.lot_id} (${lot.quantity})`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
          ) : null}
          <FormField
            name="gr-return-reason"
            label={t("procurement.goodsReceipts.return.reason")}
            required
          >
            <Textarea
              id="gr-return-reason"
              rows={3}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </FormField>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("procurement.goodsReceipts.return.cancel")}
          </Button>
          <Button className="h-11" onClick={() => void submit()} disabled={!valid || saving}>
            {t("procurement.goodsReceipts.return.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
