/**
 * "Dupliquer ce plan" (L6-11): a new draft plan (version 1) copied from the plan's published
 * version (else its latest draft) onto the same or another asset. A trigger on a chosen meter
 * keeps a meter only when one of the target asset's meters with the same unit is picked here;
 * otherwise the meter stays to choose and the version cannot be published until it is.
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  type DuplicatePmPlanRowResult,
  type PmAssetMeter,
  type PmPlanRow,
  type PmScopeOption,
  listPmAssetMeters,
  listPmScopeOptions,
} from "@/services/pm-service";
import type { AssetSearchResult, DuplicatePmPlanInput, PmPlanVersion } from "@shared/ipc-types";

import { formatPmAsset } from "./pm-cells";
import { pmUnreportedErrorText } from "./pm-errors";

const METER_LATER = "__later";

interface PmDuplicatePlanDialogProps {
  open: boolean;
  plan: PmPlanRow | null;
  versions: PmPlanVersion[];
  saving?: boolean;
  onClose: () => void;
  onDuplicate: (input: DuplicatePmPlanInput) => Promise<DuplicatePmPlanRowResult>;
}

/** The version a copy takes: the current published one, else the latest draft (as the backend). */
export function versionToCopy(
  plan: Pick<PmPlanRow, "current_version_id">,
  versions: PmPlanVersion[],
): PmPlanVersion | null {
  const current = versions.find(
    (v) => v.id === plan.current_version_id && v.status === "published",
  );
  if (current) return current;
  const drafts = versions
    .filter((v) => v.status === "draft")
    .sort((a, b) => b.version_no - a.version_no || b.id - a.id);
  return drafts[0] ?? null;
}

/** The chosen asset meter of a trigger (`meter` plan, or the meter block of a calendar plan). */
export function sourceMeterIdOf(strategy: string, triggerJson: string): number | null {
  let trigger: unknown;
  try {
    trigger = JSON.parse(triggerJson);
  } catch {
    return null;
  }
  if (trigger == null || typeof trigger !== "object") return null;
  const obj = trigger as Record<string, unknown>;
  const spec =
    strategy === "meter"
      ? obj
      : strategy === "fixed" || strategy === "floating"
        ? (obj["meter"] as Record<string, unknown> | null | undefined)
        : null;
  const id = spec != null && typeof spec === "object" ? spec["asset_meter_id"] : null;
  return typeof id === "number" && id > 0 ? id : null;
}

export function PmDuplicatePlanDialog({
  open,
  plan,
  versions,
  saving,
  onClose,
  onDuplicate,
}: PmDuplicatePlanDialogProps) {
  const { t } = useTranslation("pm");
  const [code, setCode] = useState("");
  const [title, setTitle] = useState("");
  /** Another asset than the source one (null: the source asset). */
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [scopeId, setScopeId] = useState("");
  const [scopeOptions, setScopeOptions] = useState<PmScopeOption[]>([]);
  const [meterId, setMeterId] = useState(METER_LATER);
  /** `undefined`: unknown (source meter inactive or not loaded); `null`: meter without unit. */
  const [sourceUnit, setSourceUnit] = useState<number | null | undefined>(undefined);
  const [targetMeters, setTargetMeters] = useState<PmAssetMeter[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const base = plan ? versionToCopy(plan, versions) : null;
  const isEquipment = plan?.asset_scope_type === "equipment";
  const sourceMeterId =
    plan && base ? sourceMeterIdOf(plan.strategy_type, base.trigger_definition_json) : null;
  const sourceEquipmentId = isEquipment ? (plan?.asset_scope_id ?? null) : null;
  const targetEquipmentId = isEquipment ? (asset?.id ?? sourceEquipmentId) : null;

  useEffect(() => {
    if (!open || !plan) return;
    setCode("");
    setTitle(t("dialogs.duplicatePlan.prefilledTitle", { title: plan.title }));
    setAsset(null);
    setScopeId(plan.asset_scope_id != null ? String(plan.asset_scope_id) : "");
    setMeterId(METER_LATER);
    setError(null);
  }, [open, plan, t]);

  // Values of a fanned-out scope (family, location, criticality group), source one preselected.
  useEffect(() => {
    if (!open || !plan || isEquipment) {
      setScopeOptions([]);
      return;
    }
    let cancelled = false;
    listPmScopeOptions(plan.asset_scope_type)
      .then((rows) => {
        if (!cancelled) setScopeOptions(rows);
      })
      .catch(() => {
        // Reported by the global IPC toast.
        if (!cancelled) setScopeOptions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, plan, isEquipment]);

  // Unit of the source meter.
  useEffect(() => {
    setSourceUnit(undefined);
    if (!open || sourceMeterId == null || sourceEquipmentId == null) return;
    let cancelled = false;
    listPmAssetMeters(sourceEquipmentId)
      .then((rows) => {
        const meter = rows.find((m) => m.id === sourceMeterId);
        if (!cancelled) setSourceUnit(meter ? meter.unit_value_id : undefined);
      })
      .catch(() => {
        // Reported by the global IPC toast; the meter then stays to choose.
      });
    return () => {
      cancelled = true;
    };
  }, [open, sourceMeterId, sourceEquipmentId]);

  // Meters of the target asset.
  useEffect(() => {
    setTargetMeters(null);
    if (!open || sourceMeterId == null || targetEquipmentId == null) return;
    let cancelled = false;
    listPmAssetMeters(targetEquipmentId)
      .then((rows) => {
        if (!cancelled) setTargetMeters(rows);
      })
      .catch(() => {
        // Reported by the global IPC toast; the meter then stays to choose.
      });
    return () => {
      cancelled = true;
    };
  }, [open, sourceMeterId, targetEquipmentId]);

  const sameUnitMeters = useMemo(
    () =>
      sourceUnit === undefined
        ? []
        : (targetMeters ?? []).filter((m) => (m.unit_value_id ?? null) === sourceUnit),
    [targetMeters, sourceUnit],
  );

  // Same asset: its own meter is preselected; another asset: the person picks one.
  useEffect(() => {
    const own = sameUnitMeters.find((m) => m.id === sourceMeterId);
    setMeterId(own ? String(own.id) : METER_LATER);
  }, [sameUnitMeters, sourceMeterId]);

  if (!plan) return null;

  const sourceAsset = formatPmAsset(plan.asset_code, plan.asset_name);
  const meterLater = sourceMeterId != null && meterId === METER_LATER;

  async function handleSubmit() {
    if (!plan) return;
    setError(null);
    const targetScopeId = isEquipment ? targetEquipmentId : scopeId ? Number(scopeId) : null;
    try {
      await onDuplicate({
        source_plan_id: plan.id,
        code: code.trim(),
        title: title.trim(),
        target_scope_type: plan.asset_scope_type,
        target_scope_id: targetScopeId,
        target_meter_id: sourceMeterId != null && meterId !== METER_LATER ? Number(meterId) : null,
      });
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("dialogs.duplicatePlan.title", { code: plan.code })}</DialogTitle>
          <DialogDescription>
            {base
              ? t("dialogs.duplicatePlan.description", { version: `v${base.version_no}` })
              : t("dialogs.duplicatePlan.noVersion")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 py-2">
          <div className="grid gap-1">
            <Label htmlFor="pm-dup-code">{t("fields.code")}</Label>
            <Input id="pm-dup-code" value={code} onChange={(e) => setCode(e.target.value)} />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-dup-title">{t("fields.title")}</Label>
            <Input id="pm-dup-title" value={title} onChange={(e) => setTitle(e.target.value)} />
          </div>
          {isEquipment ? (
            <div className="grid gap-1">
              <Label>{t("dialogs.duplicatePlan.target")}</Label>
              <AssetPicker value={asset} onChange={setAsset} />
              {!asset ? (
                <p className="text-xs text-text-muted">
                  {sourceAsset
                    ? `${t("dialogs.duplicatePlan.targetCurrent", { asset: sourceAsset })} — `
                    : ""}
                  {t("dialogs.duplicatePlan.targetHint")}
                </p>
              ) : null}
            </div>
          ) : (
            <div className="grid gap-1">
              <Label htmlFor="pm-dup-scope">
                {t("dialogs.duplicatePlan.targetScope")} — {t(`scope.${plan.asset_scope_type}`)}
              </Label>
              <Select value={scopeId} onValueChange={setScopeId}>
                <SelectTrigger id="pm-dup-scope">
                  <SelectValue placeholder={t("errors.scopeRequired")} />
                </SelectTrigger>
                <SelectContent>
                  {scopeOptions.map((o) => (
                    <SelectItem key={o.id} value={String(o.id)}>
                      {o.label && o.label !== o.code ? `${o.code} — ${o.label}` : o.code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          {sourceMeterId != null && isEquipment ? (
            <div className="grid gap-1">
              <Label htmlFor="pm-dup-meter">{t("dialogs.duplicatePlan.meter")}</Label>
              {targetMeters != null && sameUnitMeters.length === 0 ? (
                <p className="text-sm text-tone-warning-fg">
                  {t("dialogs.duplicatePlan.meterNone")}
                </p>
              ) : (
                <>
                  <Select value={meterId} onValueChange={setMeterId}>
                    <SelectTrigger id="pm-dup-meter">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={METER_LATER}>
                        {t("dialogs.duplicatePlan.meterLater")}
                      </SelectItem>
                      {sameUnitMeters.map((m) => (
                        <SelectItem key={m.id} value={String(m.id)}>
                          {m.unit ? `${m.name} (${m.unit})` : m.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-text-muted">{t("dialogs.duplicatePlan.meterHint")}</p>
                </>
              )}
            </div>
          ) : null}
          {meterLater ? (
            <p className="text-sm text-tone-warning-fg">
              {t("dialogs.duplicatePlan.meterLaterWarning")}
            </p>
          ) : null}
          <p className="text-xs text-text-muted">{t("dialogs.duplicatePlan.notCopied")}</p>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button
            disabled={
              saving || !base || !code.trim() || !title.trim() || (!isEquipment && !scopeId)
            }
            onClick={() => void handleSubmit()}
          >
            {t("actions.duplicate")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
