/**
 * Returns a retired asset to service — the counterpart of "Déclasser". Recorded as the asset's
 * `RECOMMISSIONED` lifecycle event (who, when and why), which sets the chosen status and clears
 * the end-of-life date. Same right as decommissioning (equipment management).
 */
import { RotateCcw } from "lucide-react";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { statusLabelKey } from "@/components/assets/asset-tones";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
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
import { appErrorReason } from "@/lib/app-error";
import { recordLifecycleEvent } from "@/services/asset-lifecycle-service";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";
import type { Asset } from "@shared/ipc-types";

interface AssetRecommissionDialogProps {
  open: boolean;
  asset: Asset;
  onClose: () => void;
  /** Called once the server recorded the return to service (the caller reloads the asset). */
  onRecommissioned: (statusLabel: string) => void | Promise<void>;
}

export function AssetRecommissionDialog({
  open,
  asset,
  onClose,
  onRecommissioned,
}: AssetRecommissionDialogProps) {
  const { t } = useTranslation("equipment");
  const { catalog } = useEquipmentTaxonomy();
  const [status, setStatus] = useState("");
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Statuses an asset can return to: every published status that does not mean "retired".
  const statuses = useMemo(
    () => (catalog?.statuses ?? []).filter((s) => s.semantic_tag !== "retired"),
    [catalog],
  );
  const labelOf = (code: string) => {
    const key = statusLabelKey(code);
    return key ? String(t(key)) : (statuses.find((s) => s.code === code)?.label ?? code);
  };

  // Each opening starts blank: the person chooses the status, nothing is proposed for them.
  useEffect(() => {
    if (!open) return;
    setReason("");
    setSubmitted(false);
    setError(null);
    setStatus("");
  }, [open]);

  const reasonMissing = reason.trim() === "";
  // A choice only counts while the plant's list still offers it (the list can be republished).
  const statusChosen = statuses.some((s) => s.code === status);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (reasonMissing || !statusChosen) return;
    setBusy(true);
    setError(null);
    try {
      await recordLifecycleEvent({
        asset_id: asset.id,
        event_type: "RECOMMISSIONED",
        to_status_code: status,
        notes: reason.trim(),
        // The version the person saw: a change made meanwhile is refused (ASSET_STALE_VERSION).
        expected_row_version: asset.row_version,
      });
      await onRecommissioned(labelOf(status));
    } catch (err) {
      // The server's coded refusals (not retired, reason, retired target, stale version) are
      // translated in equipment errors.
      setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => (!next && !busy ? onClose() : undefined)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("recommission.title")}</DialogTitle>
          <DialogDescription>
            {t("recommission.description", { code: asset.asset_code })}
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(e) => void submit(e)}>
          <FormField
            name="recommission-status"
            label={t("recommission.status")}
            required
            error={
              submitted && statuses.length > 0 && !statusChosen
                ? t("recommission.statusRequired")
                : undefined
            }
          >
            {statuses.length === 0 ? (
              <p className="text-sm text-text-muted">{t("recommission.noStatus")}</p>
            ) : (
              <Select value={statusChosen ? status : ""} onValueChange={setStatus}>
                <SelectTrigger id="recommission-status" className="h-10">
                  <SelectValue placeholder={t("recommission.statusPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {statuses.map((s) => (
                    <SelectItem key={s.code} value={s.code}>
                      {labelOf(s.code)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </FormField>
          <FormField
            name="recommission-reason"
            label={t("recommission.reason")}
            required
            error={submitted && reasonMissing ? t("recommission.reasonRequired") : undefined}
          >
            <Textarea
              id="recommission-reason"
              rows={3}
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </FormField>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={busy}>
              {t("recommission.cancel")}
            </Button>
            <Button type="submit" disabled={busy || statuses.length === 0} className="gap-1.5">
              <RotateCcw className="h-4 w-4" aria-hidden />
              {t("recommission.confirm")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
