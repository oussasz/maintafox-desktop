import { useEffect, useState } from "react";
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
import type {
  CreatePmPlanVersionInput,
  PmPlan,
  PmPlanVersion,
  UpdatePmPlanVersionInput,
} from "@shared/ipc-types";

import { buildTriggerJson, parseTriggerJson } from "./pm-model";

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
  if (!raw) return "";
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed)) return parsed.map(String).join(", ");
  } catch {
    /* ignore */
  }
  return "";
}

function taskPackageToLines(raw: string | null | undefined): string {
  if (!raw) return "";
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed)) return parsed.map(String).join("\n");
    if (parsed && typeof parsed === "object" && "steps" in parsed) {
      const steps = (parsed as { steps?: unknown }).steps;
      if (Array.isArray(steps)) return steps.map(String).join("\n");
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
  const [intervalUnit, setIntervalUnit] = useState("day");
  const [intervalValue, setIntervalValue] = useState(30);
  const [meterSource, setMeterSource] = useState("operating_hours");
  const [meterInterval, setMeterInterval] = useState(250);
  const [eventCode, setEventCode] = useState("");
  const [conditionCode, setConditionCode] = useState("");
  const [taskLines, setTaskLines] = useState("");
  const [partsCsv, setPartsCsv] = useState("");
  const [skillsCsv, setSkillsCsv] = useState("");
  const [toolsCsv, setToolsCsv] = useState("");
  const [duration, setDuration] = useState("");
  const [laborCost, setLaborCost] = useState("");
  const [partsCost, setPartsCost] = useState("");
  const [serviceCost, setServiceCost] = useState("");
  const [changeReason, setChangeReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    if (mode === "edit" && version) {
      const parsed = parseTriggerJson(plan.strategy_type, version.trigger_definition_json);
      setEffectiveFrom(version.effective_from.slice(0, 10));
      setEffectiveTo(version.effective_to ? version.effective_to.slice(0, 10) : "");
      setIntervalUnit(parsed.intervalUnit);
      setIntervalValue(parsed.intervalValue);
      setMeterSource(parsed.meterSource);
      setMeterInterval(parsed.meterInterval);
      setEventCode(parsed.eventCode);
      setConditionCode(parsed.conditionCode);
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
      setIntervalUnit("day");
      setIntervalValue(30);
      setMeterSource("operating_hours");
      setMeterInterval(250);
      setEventCode("");
      setConditionCode("");
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

  async function handleSubmit() {
    setError(null);
    if (plan.strategy_type === "event" && !eventCode.trim()) {
      setError(t("errors.eventCodeRequired"));
      return;
    }
    if (plan.strategy_type === "condition" && !conditionCode.trim()) {
      setError(t("errors.conditionCodeRequired"));
      return;
    }
    const trigger_definition_json = buildTriggerJson({
      strategy: plan.strategy_type,
      intervalUnit,
      intervalValue,
      meterSource,
      meterInterval,
      eventCode,
      conditionCode,
    });
    const payload = {
      effective_from: `${effectiveFrom}T00:00:00Z`,
      effective_to: effectiveTo ? `${effectiveTo}T23:59:59Z` : null,
      trigger_definition_json,
      task_package_json: linesToTaskPackage(taskLines),
      required_parts_json: csvToJsonArray(partsCsv),
      required_skills_json: csvToJsonArray(skillsCsv),
      required_tools_json: csvToJsonArray(toolsCsv),
      estimated_duration_hours: duration ? Number(duration) : null,
      estimated_labor_cost: laborCost ? Number(laborCost) : null,
      estimated_parts_cost: partsCost ? Number(partsCost) : null,
      estimated_service_cost: serviceCost ? Number(serviceCost) : null,
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
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
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
            <Label>{t("fields.effectiveFrom")}</Label>
            <Input
              type="date"
              value={effectiveFrom}
              onChange={(e) => setEffectiveFrom(e.target.value)}
            />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.effectiveTo")}</Label>
            <Input
              type="date"
              value={effectiveTo}
              onChange={(e) => setEffectiveTo(e.target.value)}
            />
          </div>
          {(plan.strategy_type === "fixed" || plan.strategy_type === "floating") && (
            <>
              <div className="grid gap-1">
                <Label>{t("fields.intervalUnit")}</Label>
                <Select value={intervalUnit} onValueChange={setIntervalUnit}>
                  <SelectTrigger>
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
                <Label>{t("fields.intervalValue")}</Label>
                <Input
                  type="number"
                  min={1}
                  value={intervalValue}
                  onChange={(e) => setIntervalValue(Number(e.target.value) || 1)}
                />
              </div>
            </>
          )}
          {plan.strategy_type === "meter" && (
            <>
              <div className="grid gap-1">
                <Label>{t("fields.meterSource")}</Label>
                <Select value={meterSource} onValueChange={setMeterSource}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="odometer">{t("options.meterSource.odometer")}</SelectItem>
                    <SelectItem value="operating_hours">
                      {t("options.meterSource.operating_hours")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1">
                <Label>{t("fields.intervalValue")}</Label>
                <Input
                  type="number"
                  min={1}
                  value={meterInterval}
                  onChange={(e) => setMeterInterval(Number(e.target.value) || 1)}
                />
              </div>
            </>
          )}
          {plan.strategy_type === "event" && (
            <div className="grid gap-1 md:col-span-2">
              <Label>{t("fields.eventCode")}</Label>
              <Input value={eventCode} onChange={(e) => setEventCode(e.target.value)} />
            </div>
          )}
          {plan.strategy_type === "condition" && (
            <div className="grid gap-1 md:col-span-2">
              <Label>{t("fields.conditionCode")}</Label>
              <Input value={conditionCode} onChange={(e) => setConditionCode(e.target.value)} />
            </div>
          )}
          <div className="grid gap-1 md:col-span-2">
            <Label>{t("fields.taskPackage")}</Label>
            <Textarea rows={4} value={taskLines} onChange={(e) => setTaskLines(e.target.value)} />
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label>{t("fields.requiredParts")}</Label>
            <Input value={partsCsv} onChange={(e) => setPartsCsv(e.target.value)} />
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label>{t("fields.requiredSkills")}</Label>
            <Input value={skillsCsv} onChange={(e) => setSkillsCsv(e.target.value)} />
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label>{t("fields.requiredTools")}</Label>
            <Input value={toolsCsv} onChange={(e) => setToolsCsv(e.target.value)} />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.estimatedDurationHours")}</Label>
            <Input value={duration} onChange={(e) => setDuration(e.target.value)} />
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.estimatedLaborCost")}</Label>
            <Input value={laborCost} onChange={(e) => setLaborCost(e.target.value)} />
            {laborCost ? (
              <p className="text-xs text-muted-foreground">
                {formatCurrency(Number(laborCost) || 0)}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.estimatedPartsCost")}</Label>
            <Input value={partsCost} onChange={(e) => setPartsCost(e.target.value)} />
            {partsCost ? (
              <p className="text-xs text-muted-foreground">
                {formatCurrency(Number(partsCost) || 0)}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1">
            <Label>{t("fields.estimatedServiceCost")}</Label>
            <Input value={serviceCost} onChange={(e) => setServiceCost(e.target.value)} />
            {serviceCost ? (
              <p className="text-xs text-muted-foreground">
                {formatCurrency(Number(serviceCost) || 0)}
              </p>
            ) : null}
          </div>
          <div className="grid gap-1 md:col-span-2">
            <Label>{t("fields.changeReason")}</Label>
            <Input value={changeReason} onChange={(e) => setChangeReason(e.target.value)} />
          </div>
          {error ? <p className="md:col-span-2 text-sm text-destructive">{error}</p> : null}
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
