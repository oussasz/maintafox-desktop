/**
 * "Dupliquer ce plan" (L6-11): a new draft plan (version 1) copied from the plan's published
 * version (else its latest draft) onto the same or another asset. A trigger on a chosen meter
 * keeps a meter only when one of the target asset's meters with the same unit is picked here;
 * otherwise the meter stays to choose and the version cannot be published until it is.
 */
import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
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
import { appErrorReason } from "@/lib/app-error";
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
  /** The meter preselected for the target (what "no change" means for the discard question). */
  const [defaultMeterId, setDefaultMeterId] = useState(METER_LATER);
  const [submitting, setSubmitting] = useState(false);
  /** "Quitter sans enregistrer ?" shown over the form. */
  const [asking, setAsking] = useState(false);

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
    setDefaultMeterId(METER_LATER);
    setError(null);
    setSubmitting(false);
    setAsking(false);
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
    const preselected = own ? String(own.id) : METER_LATER;
    setMeterId(preselected);
    setDefaultMeterId(preselected);
  }, [sameUnitMeters, sourceMeterId]);

  if (!plan) return null;

  const sourceAsset = formatPmAsset(plan.asset_code, plan.asset_name);
  const meterLater = sourceMeterId != null && meterId === METER_LATER;
  const prefilledTitle = t("dialogs.duplicatePlan.prefilledTitle", { title: plan.title });
  const initialScopeId = plan.asset_scope_id != null ? String(plan.asset_scope_id) : "";
  const busy = Boolean(saving) || submitting;
  // Changed from what the dialog opened with: closing asks first.
  const dirty =
    code.trim() !== "" ||
    title !== prefilledTitle ||
    asset != null ||
    (!isEquipment && scopeId !== initialScopeId) ||
    meterId !== defaultMeterId;
  // Why "Dupliquer" cannot be pressed, said under it (first missing item).
  const blockedReason = !base
    ? t("dialogs.duplicatePlan.noVersion")
    : !code.trim()
      ? t("dialogs.duplicatePlan.whyCode")
      : !title.trim()
        ? t("dialogs.duplicatePlan.whyTitle")
        : !isEquipment && !scopeId
          ? t("dialogs.duplicatePlan.whyScope")
          : null;

  function requestClose() {
    if (busy) return;
    if (asking) {
      setAsking(false);
      return;
    }
    if (dirty) setAsking(true);
    else onClose();
  }

  async function handleSubmit() {
    if (!plan) return;
    setError(null);
    setSubmitting(true);
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
      // The copy was not created: the refusal is said here (not sent to the global toast).
      setError(appErrorReason(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && requestClose()}>
      {/* Its own discard question: it also covers the asset, scope and meter choices and Cancel. */}
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto" confirmDiscard={false}>
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
            <Input
              id="pm-dup-code"
              autoFocus
              required
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
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
        {blockedReason && !busy ? (
          <p id="pm-dup-why" className="text-right text-sm text-text-muted">
            {blockedReason}
          </p>
        ) : null}
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={requestClose}>
            {t("actions.cancel")}
          </Button>
          <Button
            disabled={busy || blockedReason != null}
            aria-describedby={blockedReason ? "pm-dup-why" : undefined}
            onClick={() => void handleSubmit()}
          >
            {busy ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden />
                {t("dialogs.duplicatePlan.saving")}
              </>
            ) : (
              t("actions.duplicate")
            )}
          </Button>
        </DialogFooter>
        {asking ? <DiscardQuestion onKeep={() => setAsking(false)} onLeave={onClose} /> : null}
      </DialogContent>
    </Dialog>
  );
}

/**
 * The app's "Quitter sans enregistrer ?" question (texts of the shared dialog guard). While it
 * shows, the fields behind it cannot be reached by keyboard or screen reader (`inert`, as the
 * shared guard of `DialogContent` does).
 */
function DiscardQuestion({ onKeep, onLeave }: { onKeep: () => void; onLeave: () => void }) {
  const { t } = useTranslation("common");
  const questionEl = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const question = questionEl.current;
    const root = question?.parentElement;
    if (!question || !root) return;
    const behind = Array.from(root.children).filter((el) => el !== question);
    for (const el of behind) el.setAttribute("inert", "");
    return () => {
      for (const el of behind) el.removeAttribute("inert");
    };
  }, []);
  return (
    <div
      ref={questionEl}
      role="alertdialog"
      aria-modal="true"
      aria-label={t("discardGuard.title")}
      aria-describedby="pm-dup-discard-text"
      className="absolute inset-0 z-10 flex items-center justify-center rounded-[inherit] bg-background/90 p-6"
    >
      <div className="max-w-sm space-y-4 text-center">
        <p className="text-base font-semibold">{t("discardGuard.title")}</p>
        <p id="pm-dup-discard-text" className="text-sm text-text-muted">
          {t("discardGuard.description")}
        </p>
        <div className="flex justify-center gap-2">
          <Button type="button" variant="outline" autoFocus onClick={onKeep}>
            {t("discardGuard.keep")}
          </Button>
          <Button type="button" variant="destructive" onClick={onLeave}>
            {t("discardGuard.leave")}
          </Button>
        </div>
      </div>
    </div>
  );
}
