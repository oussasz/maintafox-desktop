/**
 * Shared capture dialog for repairable order transitions that need extra input:
 * dispatch (serial + vendor), scrap / cancel (mandatory reason) and receive-back
 * (return location when the order has none).
 *
 * Collects the fields only — the caller owns the transition call and reload.
 */

import { useEffect, useState } from "react";
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
import { Textarea } from "@/components/ui/textarea";
import { formatEntityCode } from "@/lib/display";
import type { InventorySupplier, RepairableOrder, StockLocation } from "@shared/ipc-types";

export type RepairableActionKind = "dispatch" | "scrap" | "cancel" | "receive";

export type RepairableActionPayload = {
  reason?: string | null;
  serial_number?: string | null;
  vendor_supplier_id?: number | null;
  return_location_id?: number | null;
};

export interface RepairableActionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  kind: RepairableActionKind | null;
  order: RepairableOrder | null;
  suppliers: InventorySupplier[];
  locations: StockLocation[];
  saving?: boolean;
  onConfirm: (payload: RepairableActionPayload) => void | Promise<void>;
}

/** Scrapping a returned part writes stock off, so a target location is mandatory. */
export function repairableActionNeedsLocation(
  kind: RepairableActionKind,
  order: RepairableOrder | null,
): boolean {
  if (!order || order.return_location_id != null) return false;
  if (kind === "receive") return true;
  return kind === "scrap" && order.status === "RETURNED_FROM_REPAIR";
}

export function RepairableActionDialog({
  open,
  onOpenChange,
  kind,
  order,
  suppliers,
  locations,
  saving = false,
  onConfirm,
}: RepairableActionDialogProps) {
  const { t } = useTranslation("inventory");

  const [reason, setReason] = useState("");
  const [serial, setSerial] = useState("");
  const [vendorId, setVendorId] = useState(0);
  const [locationId, setLocationId] = useState(0);

  useEffect(() => {
    if (!open) return;
    setReason("");
    setSerial(order?.serial_number ?? "");
    setVendorId(order?.vendor_supplier_id ?? 0);
    setLocationId(order?.return_location_id ?? 0);
  }, [open, order]);

  if (!kind) return null;

  const needsReason = kind === "scrap" || kind === "cancel";
  const needsLocation = repairableActionNeedsLocation(kind, order);
  const reasonMissing = needsReason && reason.trim().length === 0;
  const locationMissing = needsLocation && locationId <= 0;

  const submit = () => {
    const payload: RepairableActionPayload = {};
    if (needsReason) payload.reason = reason.trim();
    if (kind === "dispatch") {
      payload.serial_number = serial.trim() || null;
      payload.vendor_supplier_id = vendorId > 0 ? vendorId : null;
    }
    if (needsLocation) payload.return_location_id = locationId;
    void onConfirm(payload);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2">
            <span>{t(`procurement.repairableAction.${kind}.title`)}</span>
            {order ? (
              <span className="font-mono text-sm text-text-muted">
                {formatEntityCode(order.order_code)}
              </span>
            ) : null}
          </DialogTitle>
        </DialogHeader>

        <div className="space-y-3">
          <p className="text-xs text-text-muted">
            {t(`procurement.repairableAction.${kind}.hint`)}
          </p>

          {kind === "dispatch" ? (
            <>
              <div className="space-y-1">
                <Label htmlFor="repair-action-serial">
                  {t("procurement.repairableAction.fields.serial")}
                </Label>
                <Input
                  id="repair-action-serial"
                  value={serial}
                  onChange={(e) => setSerial(e.target.value)}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.repairableAction.fields.vendor")}</Label>
                <Select value={String(vendorId)} onValueChange={(v) => setVendorId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">
                      {t("procurement.repairableAction.fields.noVendor")}
                    </SelectItem>
                    {suppliers.map((supplier) => (
                      <SelectItem key={supplier.id} value={String(supplier.id)}>
                        {supplier.code} — {supplier.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </>
          ) : null}

          {needsReason ? (
            <div className="space-y-1">
              <Label htmlFor="repair-action-reason">
                {t("procurement.repairableAction.fields.reason")}
              </Label>
              <Textarea
                id="repair-action-reason"
                rows={3}
                value={reason}
                placeholder={t("procurement.repairableAction.fields.reasonPlaceholder")}
                onChange={(e) => setReason(e.target.value)}
              />
            </div>
          ) : null}

          {needsLocation ? (
            <div className="space-y-1">
              <Label>{t("procurement.repairableAction.fields.returnLocation")}</Label>
              <Select value={String(locationId)} onValueChange={(v) => setLocationId(Number(v))}>
                <SelectTrigger>
                  <SelectValue
                    placeholder={t("procurement.repairableAction.fields.selectLocation")}
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.repairableAction.fields.selectLocation")}
                  </SelectItem>
                  {locations.map((location) => (
                    <SelectItem key={location.id} value={String(location.id)}>
                      {location.warehouse_code}/{location.code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          ) : null}
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t("procurement.repairableAction.close")}
          </Button>
          <Button
            type="button"
            variant={needsReason ? "destructive" : "default"}
            disabled={saving || reasonMissing || locationMissing}
            onClick={submit}
          >
            {t(`procurement.repairableAction.${kind}.confirm`)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
