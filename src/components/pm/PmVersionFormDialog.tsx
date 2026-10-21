import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import type { PmPlanRow as PmPlan } from "@/services/pm-service";
import type {
  CreatePmPlanVersionInput,
  PmPlanVersion,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

import {
  PmMeterSpecFields,
  PmPartsPicker,
  PmSkillsPicker,
  PmToleranceFields,
} from "./PmVersionPickers";
import { pmUnreportedErrorText } from "./pm-errors";
import {
  PM_TRIGGER_FORM_START,
  type PmMeterSpec,
  type PmRequiredPart,
  type PmTriggerFields,
  buildTriggerJson,
  parseCodeList,
  parseRequiredParts,
  partsWithoutNecessity,
  parseTriggerJson,
  requiredPartsJson,
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

function jsonArrayToCsv(raw: string | null | undefined): string {
  return parseCodeList(raw).join(", ");
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

/** Largest crew on one job (`planning::load::MAX_CREW_SIZE`; also enforced by the database). */
const MAX_CREW_SIZE = 50;

/** The crew size typed: empty (none), a whole number of people from 1 to 50, or refused. */
function readCrewSize(raw: string): { ok: true; value: number | null } | { ok: false } {
  if (!raw.trim()) return { ok: true, value: null };
  const n = Number(raw);
  return Number.isInteger(n) && n >= 1 && n <= MAX_CREW_SIZE
    ? { ok: true, value: n }
    : { ok: false };
}

const NEW_OR_METER: PmMeterSpec = {
  assetMeterId: null,
  meterType: null,
  interval: PM_TRIGGER_FORM_START.meter.interval,
  startReading: null,
};

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
  const { formatCurrency } = useFormatters();
  const [effectiveFrom, setEffectiveFrom] = useState("");
  const [effectiveTo, setEffectiveTo] = useState("");
  const [trigger, setTrigger] = useState<PmTriggerFields>(PM_TRIGGER_FORM_START);
  const [taskLines, setTaskLines] = useState("");
  const [parts, setParts] = useState<PmRequiredPart[]>([]);
  const [skills, setSkills] = useState<string[]>([]);
  const [toolsCsv, setToolsCsv] = useState("");
  const [duration, setDuration] = useState("");
  const [crewSize, setCrewSize] = useState("");
  const [laborCost, setLaborCost] = useState("");
  const [partsCost, setPartsCost] = useState("");
  const [serviceCost, setServiceCost] = useState("");
  const [changeReason, setChangeReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  const strategy = plan.strategy_type;
  const isCalendar = strategy === "fixed" || strategy === "floating";
  const isMeter = strategy === "meter";
  const planAssetId =
    plan.asset_scope_type === "equipment" && plan.asset_scope_id != null
      ? plan.asset_scope_id
      : null;

  useEffect(() => {
    if (!open) return;
    if (mode === "edit" && version) {
      setEffectiveFrom(version.effective_from.slice(0, 10));
      setEffectiveTo(version.effective_to ? version.effective_to.slice(0, 10) : "");
      setTrigger(parseTriggerJson(strategy, version.trigger_definition_json));
      setTaskLines(taskPackageToLines(version.task_package_json));
      setParts(parseRequiredParts(version.required_parts_json));
      setSkills(parseCodeList(version.required_skills_json));
      setToolsCsv(jsonArrayToCsv(version.required_tools_json));
      setDuration(
        version.estimated_duration_hours != null ? String(version.estimated_duration_hours) : "",
      );
      setCrewSize(version.planned_crew_size != null ? String(version.planned_crew_size) : "");
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
      setTaskLines("");
      setParts([]);
      setSkills([]);
      setToolsCsv("");
      setDuration("");
      setCrewSize("");
      setLaborCost("");
      setPartsCost("");
      setServiceCost("");
      setChangeReason("");
    }
    setError(null);
  }, [open, mode, version, strategy]);

  function meterSpecError(spec: PmMeterSpec): string | null {
    if (planAssetId != null && spec.assetMeterId == null) return t("errors.meterRequired");
    if (planAssetId == null && !spec.meterType) return t("errors.meterTypeRequired");
    if (spec.startReading != null && spec.startReading < 0) return t("errors.startReadingRequired");
    return null;
  }

  async function handleSubmit() {
    setError(null);
    if (strategy === "event" && !trigger.eventCode.trim()) {
      setError(t("errors.eventCodeRequired"));
      return;
    }
    if (strategy === "condition" && !trigger.conditionCode.trim()) {
      setError(t("errors.conditionCodeRequired"));
      return;
    }
    const meterError = isMeter
      ? meterSpecError(trigger.meter)
      : isCalendar && trigger.orMeter
        ? meterSpecError(trigger.orMeter)
        : null;
    if (meterError) {
      setError(meterError);
      return;
    }
    const unanswered = partsWithoutNecessity(parts);
    if (unanswered.length > 0) {
      setError(t("hints.partNecessityMissing", { codes: unanswered.join(", ") }));
      return;
    }
    const crew = readCrewSize(crewSize);
    if (!crew.ok) {
      setError(t("fields.plannedCrewSizeInvalid", { max: MAX_CREW_SIZE }));
      return;
    }
    const payload = {
      effective_from: `${effectiveFrom}T00:00:00Z`,
      effective_to: effectiveTo ? `${effectiveTo}T23:59:59Z` : null,
      trigger_definition_json: buildTriggerJson({ ...trigger, strategy }),
      task_package_json: linesToTaskPackage(taskLines),
      required_parts_json: requiredPartsJson(parts),
      required_skills_json: skills.length > 0 ? JSON.stringify(skills) : null,
      required_tools_json: csvToJsonArray(toolsCsv),
      estimated_duration_hours: optionalNumber(duration),
      planned_crew_size: crew.value,
      estimated_labor_cost: optionalNumber(laborCost),
      estimated_parts_cost: optionalNumber(partsCost),
      estimated_service_cost: optionalNumber(serviceCost),
      change_reason: changeReason.trim() || null,
    };
    try {
      if (mode === "create") {
        await onCreate(plan.id, payload);
      } else if (version) {
        // An emptied field removes the crew size from the draft version.
        await onUpdate(version.id, version.row_version, {
          ...payload,
          clear_planned_crew_size: crew.value == null && version.planned_crew_size != null,
        });
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
          {isCalendar ? (
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
              <label className="flex min-h-9 items-center gap-2 text-sm md:col-span-2">
                <Checkbox
                  checked={trigger.orMeter != null}
                  onCheckedChange={(checked) =>
                    setTrigger((p) => ({ ...p, orMeter: checked === true ? NEW_OR_METER : null }))
                  }
                />
                {t("fields.whicheverFirst")}
              </label>
              {trigger.orMeter ? (
                <PmMeterSpecFields
                  idPrefix="pm-version-or"
                  value={trigger.orMeter}
                  equipmentId={planAssetId}
                  onChange={(orMeter) => setTrigger((p) => ({ ...p, orMeter }))}
                />
              ) : null}
            </>
          ) : null}
          {isMeter ? (
            <PmMeterSpecFields
              idPrefix="pm-version-meter"
              value={trigger.meter}
              equipmentId={planAssetId}
              onChange={(meter) => setTrigger((p) => ({ ...p, meter }))}
            />
          ) : null}
          {strategy === "event" ? (
            <div className="grid gap-1 md:col-span-2">
              <Label htmlFor="pm-version-event">{t("fields.eventCode")}</Label>
              <Input
                id="pm-version-event"
                value={trigger.eventCode}
                onChange={(e) => setTrigger((p) => ({ ...p, eventCode: e.target.value }))}
              />
            </div>
          ) : null}
          {strategy === "condition" ? (
            <div className="grid gap-1 md:col-span-2">
              <Label htmlFor="pm-version-condition">{t("fields.conditionCode")}</Label>
              <Input
                id="pm-version-condition"
                value={trigger.conditionCode}
                onChange={(e) => setTrigger((p) => ({ ...p, conditionCode: e.target.value }))}
              />
            </div>
          ) : null}
          <PmToleranceFields
            value={trigger.tolerance}
            percentAllowed={isCalendar || isMeter}
            onChange={(tolerance) => setTrigger((p) => ({ ...p, tolerance }))}
          />
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor="pm-version-tasks">{t("fields.taskPackage")}</Label>
            <Textarea
              id="pm-version-tasks"
              rows={4}
              value={taskLines}
              onChange={(e) => setTaskLines(e.target.value)}
            />
          </div>
          <PmPartsPicker value={parts} onChange={setParts} />
          <PmSkillsPicker value={skills} onChange={setSkills} />
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
            <Label htmlFor="pm-version-crew">{t("fields.plannedCrewSize")}</Label>
            <Input
              id="pm-version-crew"
              type="number"
              step="1"
              min="1"
              value={crewSize}
              onChange={(e) => setCrewSize(e.target.value)}
            />
            <p className="text-xs text-text-muted">{t("fields.plannedCrewSizeHint")}</p>
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
