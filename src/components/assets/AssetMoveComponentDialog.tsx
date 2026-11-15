/**
 * "Déplacer le composant" (L2-03): a serial-tracked component (a motor, a gearbox) goes under
 * another parent asset, keeping its own record and serial number. Also a first installation (no
 * current parent) and a removal to stock (no new parent). The server ends the current relation
 * and opens the new one at the effective date, and records who, when and why in the history.
 */
import { ArrowRightLeft } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { EquipmentParentPicker } from "@/components/assets/EquipmentParentPicker";
import { localIsoDate } from "@/components/documentation/document-validity";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
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
import { moveAssetComponent } from "@/services/asset-lifecycle-service";
import { listAssetChildren } from "@/services/asset-service";
import type { Asset } from "@shared/ipc-types";

/** The asset and every asset below it: none of them can become its parent (a loop). */
async function selfAndDescendants(assetId: number): Promise<Set<number>> {
  const seen = new Set<number>([assetId]);
  let level = [assetId];
  while (level.length > 0) {
    const rows = (await Promise.all(level.map((id) => listAssetChildren(id)))).flat();
    level = rows.map((r) => r.child_asset_id).filter((id) => !seen.has(id));
    for (const id of level) seen.add(id);
  }
  return seen;
}

interface AssetMoveComponentDialogProps {
  open: boolean;
  asset: Asset;
  /** The current parent, null when the component is in stock (not installed). */
  currentParent: Asset | null;
  onClose: () => void;
  /** Called once the server recorded the move (the caller reloads the asset). */
  onMoved: () => void | Promise<void>;
}

export function AssetMoveComponentDialog({
  open,
  asset,
  currentParent,
  onClose,
  onMoved,
}: AssetMoveComponentDialogProps) {
  const { t } = useTranslation("equipment");
  const [target, setTarget] = useState<number | null>(null);
  const [date, setDate] = useState("");
  const [dateInvalid, setDateInvalid] = useState(false);
  const [notes, setNotes] = useState("");
  const [excluded, setExcluded] = useState<Set<number>>(() => new Set([asset.id]));
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const today = localIsoDate(new Date());

  // Each opening starts from the current situation: today's date, no target chosen.
  useEffect(() => {
    if (!open) return;
    setTarget(null);
    setDate(localIsoDate(new Date()));
    setDateInvalid(false);
    setNotes("");
    setSubmitted(false);
    setError(null);
    let cancelled = false;
    setExcluded(new Set([asset.id]));
    void selfAndDescendants(asset.id)
      .then((ids) => {
        if (!cancelled) setExcluded(ids);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [open, asset.id]);

  const currentParentId = currentParent?.id ?? null;
  const sameParent = target === currentParentId;
  const dateMissing = date === "" || dateInvalid;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (sameParent || dateMissing) return;
    setBusy(true);
    setError(null);
    try {
      await moveAssetComponent({
        asset_id: asset.id,
        new_parent_asset_id: target,
        effective_date: date,
        notes: notes.trim() === "" ? null : notes.trim(),
        // The version the person saw: a change made meanwhile is refused.
        expected_row_version: asset.row_version,
      });
      await onMoved();
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
          <DialogTitle>{t("moveComponent.title")}</DialogTitle>
          <DialogDescription>
            {currentParent
              ? t("moveComponent.descriptionInstalled", {
                  code: asset.asset_code,
                  parent: `${currentParent.asset_code} — ${currentParent.asset_name}`,
                })
              : t("moveComponent.descriptionStock", { code: asset.asset_code })}
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(e) => void submit(e)}>
          <FormField
            name="move-component-target"
            label={t("moveComponent.target")}
            description={t("moveComponent.targetHint")}
            error={
              submitted && sameParent
                ? currentParent
                  ? t("moveComponent.sameParent")
                  : t("moveComponent.alreadyInStock")
                : undefined
            }
          >
            <EquipmentParentPicker
              id="move-component-target"
              value={target}
              onChange={setTarget}
              excludeIds={excluded}
              disabled={busy}
            />
          </FormField>
          <FormField
            name="move-component-date"
            label={t("moveComponent.date")}
            required
            error={submitted && dateMissing ? t("moveComponent.dateRequired") : undefined}
          >
            <DateField
              id="move-component-date"
              value={date}
              onChange={setDate}
              onInvalidChange={setDateInvalid}
              max={today}
              disabled={busy}
            />
          </FormField>
          <FormField name="move-component-notes" label={t("moveComponent.notes")}>
            <Textarea
              id="move-component-notes"
              rows={3}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              disabled={busy}
            />
          </FormField>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={busy}>
              {t("moveComponent.cancel")}
            </Button>
            <Button type="submit" disabled={busy} className="gap-1.5">
              <ArrowRightLeft className="h-4 w-4" aria-hidden />
              {target == null ? t("moveComponent.confirmRemove") : t("moveComponent.confirm")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
