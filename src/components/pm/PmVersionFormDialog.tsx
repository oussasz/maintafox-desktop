import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { listAssetMeters } from "@/services/asset-lifecycle-service";
import type { PmPlanRow as PmPlan } from "@/services/pm-service";
import type {
  AssetMeter,
  CreatePmPlanVersionInput,
  PmPlanVersion,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

import { pmUnreportedErrorText } from "./pm-errors";
import {
  PM_TRIGGER_FORM_START,
  type PmTriggerFields,
  buildTriggerJson,
  parseTriggerJson,
} from "./pm-model";

interface PmVersionFormDialogProps {
  open: boolean;
  mode: "create" | "edit";
  plan: PmPlan;
  version?: PmPlanVersion | null;
  saving?: boolean;
  onClose: () => void;
  onCreate: (pmPlanId: number, input: CreatePmPlanVersionInput) => Promise<void>;
  onUpdate: (
    versionId: number,
    rowVersion: number,
    input: UpdatePmPlanVersionInput,
  ) => Promise<void>;
}

function csvToJsonArray(raw: string): string | null {
  const parts = raw
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return null;
  return JSON.stringify(parts);
}

/** Shows plain codes and `{article_code, quantity}` entries as an editable comma list. */
function jsonArrayToCsv(raw: string | null | undefined): string {
  if (!raw) return "";
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed)) {
      return parsed
        .map((item: unknown) => {
          if (typeof item === "string") return item;
          if (item && typeof item === "object") {
            const obj = item as Record<string, unknown>;
            const code = obj["article_code"] ?? obj["code"] ?? obj["description"];
            return typeof code === "string" ? code : "";
          }
          return "";
        })
        .filter(Boolean)
        .join(", ");
    }
  } catch {
    return raw;
  }
  return "";
}

function taskPackageToLines(raw: string | null | undefined): string {
  if (!raw) return "";
  try {
    const parsed = JSON.parse(raw) as unknown;
    const steps: unknown[] | null = Array.isArray(parsed)
      ? parsed
      : parsed && typeof parsed === "object" && "steps" in parsed
        ? ((parsed as { steps?: unknown }).steps as unknown[] | null)
        : null;
    if (Array.isArray(steps)) {
      return steps
        .map((s: unknown) => {
          if (typeof s === "string") return s;
          if (s && typeof s === "object") {
            const d = (s as Record<string, unknown>)["description"];
            return typeof d === "string" ? d : "";
          }
          return "";
        })
        .filter(Boolean)
        .join("\n");
    }
  } catch {
    return raw;
  }
  return raw;
}

function linesToTaskPackage(raw: string): string | null {
  const steps = raw
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  if (steps.length === 0) return null;
  return JSON.stringify({ steps });
}

function optionalNumber(raw: string): number | null {
  if (!raw.trim()) return null;
  const n = Number(raw.replace(",", "."));
  return Number.isFinite(n) ? n : null;
}

export function PmVersionFormDialog({
  open,
  mode,
  plan,
  version,
  saving,
  onClose,
  onCreate,
  onUpdate,
}: PmVersionFormDialogProps) {
  const { t } = useTranslation("pm");
  const { formatCurrency, formatNumber } = useFormatters();
  const [effectiveFrom, setEffectiveFrom] = useState("");
  const [effectiveTo, setEffectiveTo] = useState("");
  const [trigger, setTrigger] = useState<PmTriggerFields>(PM_TRIGGER_FORM_START);
  const [startReadingText, setStartReadingText] = useState("");
  const [taskLines, setTaskLines] = useState("");
  const [partsCsv, setPartsCsv] = useState("");
  const [skillsCsv, setSkillsCsv] = useState("");
  const [toolsCsv, setToolsCsv] = useState("");
  const [duration, setDuration] = useState("");
  const [laborCost, setLaborCost] = useState("");
  const [partsCost, setPartsCost] = useState("");
  const [serviceCost, setServiceCost] = useState("");
  const [changeReason, setChangeReason] = useState("");
  const [meters, setMeters] = useState<AssetMeter[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const isMeter = plan.strategy_type === "meter";
  const meterAssetId =
    plan.asset_scope_type === "equipment" && plan.asset_scope_id != null
      ? plan.asset_scope_id
      : null;

  useEffect(() => {
    if (!open || !isMeter || meterAssetId == null) {
      setMeters(null);
      return;
    }
    let cancelled = false;
    listAssetMeters(meterAssetId)
      .then((rows) => {
        if (!cancelled) setMeters(rows.filter((m) => m.is_active));
      })
      .catch(() => {
        // The global IPC toast reports the failure; no meter can be chosen.
        if (!cancelled) setMeters([]);
      });
    return () => {
      cancelled = true;
    };
  }, [open, isMeter, meterAssetId]);

  useEffect(() => {
    if (!open) return;
    if (mode === "edit" && version) {
      const parsed = parseTriggerJson(plan.strategy_type, version.trigger_definition_json);
      setEffectiveFrom(version.effective_from.slice(0, 10));
      setEffectiveTo(version.effective_to ? version.effective_to.slice(0, 10) : "");
      setTrigger(parsed);
      setStartReadingText(parsed.startReading != null ? String(parsed.startReading) : "");
      setTaskLines(taskPackageToLines(version.task_package_json));
      setPartsCsv(jsonArrayToCsv(version.required_parts_json));
      setSkillsCsv(jsonArrayToCsv(version.required_skills_json));
      setToolsCsv(jsonArrayToCsv(version.required_tools_json));
      setDuration(
        version.estimated_duration_hours != null ? String(version.estimated_duration_hours) : "",
      );
      setLaborCost(
        version.estimated_labor_cost != null ? String(version.estimated_labor_cost) : "",
      );
      setPartsCost(
        version.estimated_parts_cost != null ? String(version.estimated_parts_cost) : "",
      );
      setServiceCost(
        version.estimated_service_cost != null ? String(version.estimated_service_cost) : "",
      );
      setChangeReason(version.change_reason ?? "");
    } else {
      setEffectiveFrom(new Date().toISOString().slice(0, 10));
      setEffectiveTo("");
      setTrigger(PM_TRIGGER_FORM_START);
      setStartReadingText("");
      setTaskLines("");
      setPartsCsv("");
      setSkillsCsv("");
      setToolsCsv("");
      setDuration("");
      setLaborCost("");
      setPartsCost("");
      setServiceCost("");
      setChangeReason("");
    }
    setError(null);
  }, [open, mode, version, plan.strategy_type]);

  const selectedMeter = useMemo(
    () => meters?.find((m) => m.id === trigger.assetMeterId) ?? null,
    [meters, trigger.assetMeterId],
  );
  const unit = selectedMeter?.unit?.trim() || null;

  function chooseMeter(id: string) {
    const meter = meters?.find((m) => String(m.id) === id) ?? null;
    setTrigger((prev) => ({ ...prev, assetMeterId: meter?.id ?? null }));
    // Start counting from the meter's current reading unless the planner already stated one.
    if (meter && !startReadingText.trim()) setStartReadingText(String(meter.current_reading));
  }

  function keepOrRebuild(csv: string, stored: string | null | undefined): string | null {
    if (mode === "edit" && stored != null && csv === jsonArrayToCsv(stored)) return stored;
    return csvToJsonArray(csv);
  }

  async function handleSubmit() {
    setError(null);
    if (plan.strategy_type === "event" && !trigger.eventCode.trim()) {
      setError(t("errors.eventCodeRequired"));
      return;
    }
    if (plan.strategy_type === "condition" && !trigger.conditionCode.trim()) {
      setError(t("errors.conditionCodeRequired"));
      return;
    }
    const startReading = optionalNumber(startReadingText);
    if (isMeter) {
      if (trigger.assetMeterId == null) {
        setError(t("errors.meterRequired"));
        return;
      }
      if (startReading == null || startReading < 0) {
        setError(t("errors.startReadingRequired"));
        return;
      }
    }
    const trigger_definition_json = buildTriggerJson({
      ...trigger,
      startReading,
      strategy: plan.strategy_type,
    });
    const payload = {
      effective_from: `${effectiveFrom}T00:00:00Z`,
      effective_to: effectiveTo ? `${effectiveTo}T23:59:59Z` : null,
      trigger_definition_json,
      task_package_json: linesToTaskPackage(taskLines),
      // An untouched list keeps its stored JSON (structured entries such as quantities included).
      required_parts_json: keepOrRebuild(partsCsv, version?.required_parts_json),
      required_skills_json: keepOrRebuild(skillsCsv, version?.required_skills_json),
      required_tools_json: keepOrRebuild(toolsCsv, version?.required_tools_json),
      estimated_duration_hours: optionalNumber(duration),
      estimated_labor_cost: optionalNumber(laborCost),
      estimated_parts_cost: optionalNumber(partsCost),
      estimated_service_cost: optionalNumber(serviceCost),
      change_reason: changeReason.trim() || null,
    };

    try {
      if (mode === "create") {
        await onCreate(plan.id, payload);
      } else if (version) {
        await onUpdate(version.id, version.row_version, payload);
      }
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }

  const costPreview = (raw: string) => {
    const n = optionalNumber(raw);
    return n != null ? <p className="text-xs text-text-muted">{formatCurrency(n)}</p> : null;
  };

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {mode === "create" ? t("dialogs.newVersion.title") : t("dialogs.editVersion.title")}
          </DialogTitle>
          <DialogDescription>
            {mode === "create"
              ? t("dialogs.newVersion.description")
              : t("dialogs.editVersion.description")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 py-2 md:grid-cols-2">
          <div className="grid gap-1">
            <Label htmlFor="pm-version-from">{t("fields.effectiveFrom")}</Label>
            <Input
              id="pm-version-from"
              type="date"
              value={effectiveFrom}
              onChange={(e) => setEffectiveFrom(e.target.value)}
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-version-to">{t("fields.effectiveTo")}</Label>
            <Input
              id="pm-version-to"
              type="date"
              value={effectiveTo}
              onChange={(e) => setEffectiveTo(e.target.value)}
            />
          </div>
          {(plan.strategy_type === "fixed" || plan.strategy_type === "floating") && (
            <>
              <div className="grid gap-1">
                <Label htmlFor="pm-version-unit">{t("fields.intervalUnit")}</Label>
                <Select
                  value={trigger.intervalUnit}
                  onValueChange={(v) => setTrigger((p) => ({ ...p, intervalUnit: v }))}
                >
                  <SelectTrigger id="pm-version-unit">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {(["day", "week", "month", "year"] as const).map((u) => (
                      <SelectItem key={u} value={u}>
                        {t(`options.intervalUnit.${u}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1">
                <Label htmlFor="pm-version-interval">{t("fields.intervalValue")}</Label>
                <Input
                  id="pm-version-interval"
                  type="number"
                  min={1}
                  value={trigger.intervalValue}
                  onChange={(e) =>
                    setTrigger((p) => ({ ...p, intervalValue: Number(e.target.value) || 1 }))
                  }
                />
              </div>
            </>
          )}
          {isMeter && (
            <>
              {meterAssetId == null ? (
                <p className="text-sm text-tone-warning-fg md:col-span-2">
                  {t("hints.meterNeedsAsset")}
                </p>
              ) : meters != null && meters.length === 0 ? (
                <p className="text-sm text-tone-warning-fg md:col-span-2">{t("hints.noMeters")}</p>
              ) : (
                <div className="grid gap-1 md:col-span-2">
                  <Label htmlFor="pm-version-meter">{t("fields.meter")}</Label>
                  <Select
                    value={trigger.assetMeterId != null ? String(trigger.assetMeterId) : ""}
                    onValueChange={chooseMeter}
                  >
                    <SelectTrigger id="pm-version-meter">
                      <SelectValue placeholder={t("errors.meterRequired")} />
                    </SelectTrigger>
                    <SelectContent>
                      {(meters ?? []).map((m) => (
                        <SelectItem key={m.id} value={String(m.id)}>
                          {m.name}
                          {m.unit ? ` (${m.unit})` : ""}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {selectedMeter ? (
                    <p className="text-xs text-text-muted">
                      {t("fields.meterCurrent", {
                        value: formatNumber(selectedMeter.current_reading),
                        unit: unit ?? "",
                      })}
                    </p>
                  ) : trigger.assetMeterId == null && mode === "edit" ? (
                    <p className="text-xs text-text-muted">
                      {t("hints.meterLegacy", {
                        type: t(`options.meterSource.${trigger.meterSource}`, {
                          defaultValue: trigger.meterSource,
                        }),
                      })}
                    </p>
                  ) : null}
                </div>
              )}
              <div className="grid gap-1">
                <Label htmlFor="pm-version-meter-interval">
                  {unit ? t("fields.meterInterval", { unit }) : t("fields.meterIntervalNoUnit")}
                </Label>
                <Input
                  id="pm-version-meter-interval"
                  type="number"
                  min={1}
                  value={trigger.meterInterval}
                  onChange={(e) =>
                    setTrigger((p) => ({ ...p, meterInterval: Number(e.target.value) || 1 }))
                  }
                />
              </div>
              <div className="grid gap-1">
                <Label htmlFor="pm-version-start-reading">
                  {unit ? t("fields.startReading", { unit }) : t("fields.startReadingNoUnit")}
                </Label>
                <Input
                  id="pm-version-start-reading"
                  type="number"
                  min={0}
                  value={startReadingText}
                  onChange={(e) => setStartReadingText(e.target.value)}
                />
              </div>
            </>
          )}
          {plan.strategy_type === "event" && (
            <div className="grid gap-1 md:col-span-2">
              <Label htmlFor="pm-version-event">{t("fields.eventCode")}</Label>
              <Input
                id="pm-version-event"
                value={trigger.eventCode}
                onChange={(e) => setTrigger((p) => ({ ...p, eventCode: e.target.value }))}
              />
            </div>
          )}
          {plan.strategy_type === "condition" && (
            <div className="grid gap-1 md:col-span-2">
              <Label htmlFor="pm-version-condition">{t("fields.conditionCode")}</Label>
              <Input
                id="pm-version-condition"
                value={trigger.conditionCode}
                onChange={(e) => setTrigger((p) => ({ ...p, conditionCode: e.target.value }))}
              />
            </div>
          )}
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-tasks">{t("fields.taskPackage")}</Label>
            <Textarea
              id="pm-version-tasks"
              rows={4}
              value={taskLines}
              onChange={(e) => setTaskLines(e.target.value)}
            />
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-parts">{t("fields.requiredParts")}</Label>
            <Input
              id="pm-version-parts"
              value={partsCsv}
              onChange={(e) => setPartsCsv(e.target.value)}
            />
            <p className="text-xs text-text-muted">{t("hints.partsFormat")}</p>
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-skills">{t("fields.requiredSkills")}</Label>
            <Input
              id="pm-version-skills"
              value={skillsCsv}
              onChange={(e) => setSkillsCsv(e.target.value)}
            />
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-tools">{t("fields.requiredTools")}</Label>
            <Input
              id="pm-version-tools"
              value={toolsCsv}
              onChange={(e) => setToolsCsv(e.target.value)}
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-version-duration">{t("fields.estimatedDurationHours")}</Label>
            <Input
              id="pm-version-duration"
              inputMode="decimal"
              value={duration}
              onChange={(e) => setDuration(e.target.value)}
            />
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-version-labor">{t("fields.estimatedLaborCost")}</Label>
            <Input
              id="pm-version-labor"
              inputMode="decimal"
              value={laborCost}
              onChange={(e) => setLaborCost(e.target.value)}
            />
            {costPreview(laborCost)}
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-version-parts-cost">{t("fields.estimatedPartsCost")}</Label>
            <Input
              id="pm-version-parts-cost"
              inputMode="decimal"
              value={partsCost}
              onChange={(e) => setPartsCost(e.target.value)}
            />
            {costPreview(partsCost)}
          </div>
          <div className="grid gap-1">
            <Label htmlFor="pm-version-service-cost">{t("fields.estimatedServiceCost")}</Label>
            <Input
              id="pm-version-service-cost"
              inputMode="decimal"
              value={serviceCost}
              onChange={(e) => setServiceCost(e.target.value)}
            />
            {costPreview(serviceCost)}
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-reason">{t("fields.changeReason")}</Label>
            <Input
              id="pm-version-reason"
              value={changeReason}
              onChange={(e) => setChangeReason(e.target.value)}
            />
          </div>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg md:col-span-2">
              {error}
            </p>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button disabled={saving || !effectiveFrom} onClick={() => void handleSubmit()}>
            {mode === "create" ? t("actions.create") : t("actions.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
