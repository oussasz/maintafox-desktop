/**
 * Edit the commercial and warranty data of an asset: purchase date, end of warranty,
 * replacement value (tenant currency), cost centre (Budget catalogue) and notes.
 * The same rules are checked by the backend (`identity::validate_commercial`).
 */

import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { DateField } from "@/components/ui/date-field";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { updateAssetCommercial } from "@/services/asset-registry-service";
import { listCostCenters } from "@/services/budget-service";
import type { Asset, CostCenter } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Backend limit (`identity::ASSET_NOTES_MAX_CHARS`). */
const NOTES_MAX_CHARS = 4000;
/** Select value meaning "no cost centre on the asset" (Radix Select needs a non-empty value). */
const NO_COST_CENTER = "__none__";

interface AssetCommercialDialogProps {
  open: boolean;
  asset: Asset;
  onClose: () => void;
  onSaved: (asset: Asset) => void;
}

interface FormState {
  purchaseDate: string;
  warrantyExpiry: string;
  replacementValue: string;
  costCenter: string;
  notes: string;
}

function fromAsset(asset: Asset): FormState {
  return {
    purchaseDate: asset.purchase_date ?? "",
    warrantyExpiry: asset.warranty_expiry_date ?? "",
    replacementValue: asset.replacement_value != null ? String(asset.replacement_value) : "",
    costCenter: asset.cost_center_code ?? NO_COST_CENTER,
    notes: asset.notes ?? "",
  };
}

export function AssetCommercialDialog({
  open,
  asset,
  onClose,
  onSaved,
}: AssetCommercialDialogProps) {
  const { t } = useTranslation("equipment");
  const formId = useId();
  const { can } = usePermissions();
  const canReadCostCenters = can(P.FIN_VIEW);
  const [form, setForm] = useState<FormState>(() => fromAsset(asset));
  const [costCenters, setCostCenters] = useState<CostCenter[]>([]);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (open) setForm(fromAsset(asset));
  }, [open, asset]);

  useEffect(() => {
    if (!open || !canReadCostCenters) return;
    let cancelled = false;
    listCostCenters({ include_inactive: false })
      .then((rows) => {
        if (!cancelled) setCostCenters(rows);
      })
      .catch(() => {
        // The global IPC toast reports the failure; the field then offers the current value only.
        if (!cancelled) setCostCenters([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, canReadCostCenters]);

  const value = form.replacementValue.trim();
  const parsedValue = value === "" ? null : Number(value.replace(",", "."));
  const valueError =
    parsedValue !== null && (!Number.isFinite(parsedValue) || parsedValue < 0)
      ? t("commercial.errors.value")
      : undefined;
  const warrantyError =
    form.purchaseDate && form.warrantyExpiry && form.warrantyExpiry < form.purchaseDate
      ? t("commercial.errors.warrantyBeforePurchase")
      : undefined;
  const notesError =
    form.notes.length > NOTES_MAX_CHARS
      ? t("commercial.errors.notesTooLong", { max: NOTES_MAX_CHARS })
      : undefined;
  const invalid = Boolean(valueError || warrantyError || notesError);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid || saving) return;
    setSaving(true);
    try {
      const updated = await updateAssetCommercial(
        asset.id,
        {
          purchase_date: form.purchaseDate || null,
          warranty_expiry_date: form.warrantyExpiry || null,
          replacement_value: parsedValue,
          cost_center_code: form.costCenter === NO_COST_CENTER ? null : form.costCenter,
          notes: form.notes.trim() || null,
        },
        asset.row_version,
      );
      onSaved(updated);
    } catch {
      // Reported by the global IPC toast; the form stays open with the user's input.
    } finally {
      setSaving(false);
    }
  };

  const currentNotInList =
    form.costCenter !== NO_COST_CENTER && !costCenters.some((c) => c.code === form.costCenter);

  return (
    <EntityFormDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t("commercial.dialogTitle", { code: asset.asset_code })}
      description={t("commercial.dialogDescription")}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={saving ? t("commercial.saving") : t("commercial.save")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4 sm:grid-cols-2">
        <FormField name={`${formId}-purchase`} label={t("commercial.purchaseDate")}>
          <DateField
            id={`${formId}-purchase`}
            value={form.purchaseDate}
            onChange={(v) => setForm((f) => ({ ...f, purchaseDate: v }))}
          />
        </FormField>
        <FormField
          name={`${formId}-warranty`}
          label={t("commercial.warrantyExpiry")}
          error={warrantyError}
        >
          <DateField
            id={`${formId}-warranty`}
            value={form.warrantyExpiry}
            min={form.purchaseDate || undefined}
            onChange={(v) => setForm((f) => ({ ...f, warrantyExpiry: v }))}
          />
        </FormField>
        <FormField
          name={`${formId}-value`}
          label={t("commercial.replacementValue")}
          description={t("commercial.replacementValueHint")}
          error={valueError}
        >
          <Input
            id={`${formId}-value`}
            inputMode="decimal"
            value={form.replacementValue}
            onChange={(e) => setForm((f) => ({ ...f, replacementValue: e.target.value }))}
          />
        </FormField>
        <FormField
          name={`${formId}-cc`}
          label={t("commercial.costCenter")}
          description={
            canReadCostCenters
              ? t("commercial.costCenterHint", {
                  inherited: asset.org_cost_center_code ?? t("commercial.costCenterNone"),
                })
              : t("commercial.costCenterNoPermission")
          }
        >
          <Select
            value={form.costCenter}
            onValueChange={(v) => setForm((f) => ({ ...f, costCenter: v }))}
            disabled={!canReadCostCenters}
          >
            <SelectTrigger id={`${formId}-cc`} className="h-10">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_COST_CENTER}>
                {t("commercial.costCenterUseLocation")}
              </SelectItem>
              {currentNotInList ? (
                <SelectItem value={form.costCenter}>{form.costCenter}</SelectItem>
              ) : null}
              {costCenters.map((c) => (
                <SelectItem key={c.id} value={c.code}>
                  {c.code} — {c.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField
          name={`${formId}-notes`}
          label={t("commercial.notes")}
          error={notesError}
          className="sm:col-span-2"
        >
          <Textarea
            id={`${formId}-notes`}
            rows={4}
            value={form.notes}
            onChange={(e) => setForm((f) => ({ ...f, notes: e.target.value }))}
          />
        </FormField>
      </form>
    </EntityFormDialog>
  );
}
