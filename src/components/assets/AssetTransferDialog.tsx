/**
 * "Transférer" (L2-04): the asset moves to another org node (line, workshop, site). The person
 * gives the destination, the effective date (today by default, never in the future) and the
 * reason; before confirming, the dialog shows what stays attached to the asset (active preventive
 * plans, spare parts, open work orders). Nothing is stopped or reassigned automatically. The server
 * checks the right on both nodes and records who, when, from, to and why in the history.
 */
import { AlertTriangle, ArrowRightLeft } from "lucide-react";
import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { EquipmentSearchSelect } from "@/components/assets/EquipmentSearchSelect";
import { localIsoDate } from "@/components/documentation/document-validity";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { DateField } from "@/components/ui/date-field";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { appErrorReason } from "@/lib/app-error";
import {
  type AssetTransferPreview,
  previewAssetTransfer,
  transferAsset,
} from "@/services/asset-transfer-service";
import type { Asset } from "@shared/ipc-types";

interface AssetTransferDialogProps {
  open: boolean;
  asset: Asset;
  onClose: () => void;
  /** Called with the moved asset once the server recorded the transfer. */
  onTransferred: (updated: Asset) => void | Promise<void>;
}

export function AssetTransferDialog({
  open,
  asset,
  onClose,
  onTransferred,
}: AssetTransferDialogProps) {
  const { t } = useTranslation("equipment");
  const [preview, setPreview] = useState<AssetTransferPreview | null>(null);
  const [previewError, setPreviewError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const [target, setTarget] = useState("");
  const [date, setDate] = useState("");
  const [dateInvalid, setDateInvalid] = useState(false);
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const today = localIsoDate(new Date());

  // Each opening starts blank: today's date, no destination, no reason.
  useEffect(() => {
    if (!open) return;
    setTarget("");
    setDate(localIsoDate(new Date()));
    setDateInvalid(false);
    setReason("");
    setSubmitted(false);
    setError(null);
  }, [open, asset.id]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setPreviewError(null);
    setPreview(null);
    void previewAssetTransfer(asset.id)
      .then((p) => {
        if (!cancelled) setPreview(p);
      })
      .catch((err: unknown) => {
        if (!cancelled) setPreviewError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, asset.id, asset.row_version, reloadKey]);

  const options = useMemo(
    () =>
      (preview?.destinations ?? []).map((d) => ({
        value: String(d.id),
        label: `${d.code} — ${d.name}`,
        description: d.node_type_label,
      })),
    [preview],
  );

  const targetMissing = !options.some((o) => o.value === target);
  const dateMissing = date === "" || dateInvalid;
  const reasonMissing = reason.trim() === "";
  const impact = preview?.impact;
  const nothingAttached =
    impact != null &&
    impact.active_pm_plan_count === 0 &&
    impact.spare_part_line_count === 0 &&
    impact.open_wo_count === 0;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (targetMissing || dateMissing || reasonMissing) return;
    setBusy(true);
    setError(null);
    try {
      const result = await transferAsset({
        asset_id: asset.id,
        to_org_node_id: Number(target),
        effective_date: date,
        reason: reason.trim(),
        // The version the person saw: a change made meanwhile is refused.
        expected_row_version: asset.row_version,
      });
      await onTransferred(result.asset);
    } catch (err) {
      setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => (!next && !busy ? onClose() : undefined)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("transfer.title")}</DialogTitle>
          <DialogDescription>
            {t("transfer.description", {
              code: asset.asset_code,
              from: asset.org_node_name ?? t("transfer.noLocation"),
            })}
          </DialogDescription>
        </DialogHeader>
        {previewError != null ? (
          <ErrorState
            error={previewError}
            onRetry={() => setReloadKey((k) => k + 1)}
            retrying={loading}
          />
        ) : !preview ? (
          <LoadingState className="py-8" />
        ) : (
          <form className="space-y-4" onSubmit={(e) => void submit(e)}>
            <FormField
              name="transfer-target"
              label={t("transfer.target")}
              description={t("transfer.targetHint")}
              required
              error={submitted && targetMissing ? t("transfer.targetRequired") : undefined}
            >
              {options.length === 0 ? (
                <p className="text-sm text-text-muted">{t("transfer.noDestination")}</p>
              ) : (
                <EquipmentSearchSelect
                  id="transfer-target"
                  options={options}
                  value={target}
                  onChange={setTarget}
                  placeholder={t("transfer.targetPlaceholder")}
                  disabled={busy}
                  aria-invalid={submitted && targetMissing}
                />
              )}
            </FormField>
            <FormField
              name="transfer-date"
              label={t("transfer.date")}
              required
              error={submitted && dateMissing ? t("transfer.dateRequired") : undefined}
            >
              <DateField
                id="transfer-date"
                value={date}
                onChange={setDate}
                onInvalidChange={setDateInvalid}
                max={today}
                disabled={busy}
              />
            </FormField>
            <FormField
              name="transfer-reason"
              label={t("transfer.reason")}
              required
              error={submitted && reasonMissing ? t("transfer.reasonRequired") : undefined}
            >
              <Textarea
                id="transfer-reason"
                rows={3}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                disabled={busy}
              />
            </FormField>
            <div
              data-testid="transfer-impact"
              className="space-y-1 rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm"
            >
              <p className="flex items-center gap-1.5 font-medium">
                <AlertTriangle className="h-4 w-4" aria-hidden />
                {t("transfer.impact.title")}
              </p>
              {nothingAttached ? (
                <p>{t("transfer.impact.none")}</p>
              ) : (
                <>
                  <ul className="list-disc pl-5">
                    <li>
                      {t("transfer.impact.pmPlans", { count: impact?.active_pm_plan_count ?? 0 })}
                    </li>
                    <li>
                      {t("transfer.impact.spareParts", {
                        count: impact?.spare_part_line_count ?? 0,
                        critical: impact?.critical_spare_part_count ?? 0,
                      })}
                    </li>
                    <li>{t("transfer.impact.openWo", { count: impact?.open_wo_count ?? 0 })}</li>
                  </ul>
                  <p className="text-xs text-text-muted">{t("transfer.impact.notChanged")}</p>
                </>
              )}
            </div>
            {error ? (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {error}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="outline" onClick={onClose} disabled={busy}>
                {t("transfer.cancel")}
              </Button>
              <Button type="submit" disabled={busy || options.length === 0} className="gap-1.5">
                <ArrowRightLeft className="h-4 w-4" aria-hidden />
                {t("transfer.confirm")}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
