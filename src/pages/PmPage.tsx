import { Filter, Play, Plus, RefreshCw, Search, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
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
import { invoke } from "@/lib/ipc-invoke";
import { listInventoryArticles } from "@/services/inventory-service";
import { listPersonnelSkillReferenceValues } from "@/services/personnel-service";
import {
  createDraftReferenceSet,
  createReferenceDomain,
  createReferenceValue,
  listReferenceDomains,
  listReferenceSets,
  listReferenceValues,
} from "@/services/reference-service";
import { usePmStore } from "@/stores/pm-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  CreatePmPlanInput,
  CreatePmPlanVersionInput,
  ExecutePmOccurrenceInput,
  PmOccurrence,
  PmPlanVersion,
} from "@shared/ipc-types";

const PLAN_STATUS_OPTIONS = [
  "draft",
  "proposed",
  "approved",
  "active",
  "suspended",
  "retired",
] as const;
const STRATEGY_OPTIONS = ["fixed", "floating", "meter", "event", "condition"] as const;
const SCOPE_OPTIONS = ["equipment", "family", "location", "criticality_group"] as const;
const OCCURRENCE_STATUS_OPTIONS = [
  "forecasted",
  "generated",
  "ready_for_scheduling",
  "scheduled",
  "in_progress",
  "completed",
  "deferred",
  "missed",
  "cancelled",
] as const;
const REASON_OPTIONS = [
  "none",
  "capacity_constraint",
  "asset_unavailable",
  "material_missing",
  "weather_constraint",
  "safety_constraint",
  "other",
] as const;

type TimeIntervalUnit = "day" | "week" | "month" | "year";
type MeterSource = "odometer" | "operating_hours";

type TriggerBuilderModel = {
  timeUnit: TimeIntervalUnit;
  timeInterval: number;
  meterSource: MeterSource;
  meterInterval: number;
  eventCode: string;
  conditionCode: string;
};

type MaintenanceTaskReference = {
  id: number;
  setId: number;
  setStatus: string;
  label: string;
  description: string | null;
  taskPackageJson: string | null;
};

type RequirementOption = {
  value: string;
  label: string;
};

type InlineNotice = {
  variant: "info" | "success" | "warning" | "error";
  message: string;
};

const PM_TASK_LIST_DOMAIN_CODE = "PM.MAINTENANCE_TASK_LIST";
const PUBLISHABLE_PLAN_LIFECYCLES = new Set(["approved", "active", "suspended"]);

function allowedPlanLifecycleTargets(current: string): string[] {
  const now = current.toLowerCase();
  const transitions: Record<string, string[]> = {
    draft: ["proposed", "retired"],
    proposed: ["draft", "approved", "retired"],
    approved: ["active", "suspended", "retired"],
    active: ["suspended", "retired"],
    suspended: ["active", "retired"],
    retired: [],
  };
  return [now, ...(transitions[now] ?? [])];
}

function parseRequiredCodeArray(input: string | null | undefined): string[] {
  if (!input) return [];
  try {
    const parsed = JSON.parse(input) as unknown;
    if (!Array.isArray(parsed)) return [];
    const values = parsed
      .map((entry) => {
        if (typeof entry === "string") return entry.trim();
        if (!entry || typeof entry !== "object") return "";
        const obj = entry as Record<string, unknown>;
        const candidate =
          obj["code"] ??
          obj["article_code"] ??
          obj["part_code"] ??
          obj["part"] ??
          obj["skill_code"] ??
          obj["tool_code"] ??
          obj["name"];
        return typeof candidate === "string" ? candidate.trim() : "";
      })
      .filter((entry) => entry.length > 0);
    return Array.from(new Set(values));
  } catch {
    return [];
  }
}

function toRequiredCodeJson(values: string[]): string | null {
  if (values.length === 0) return null;
  return JSON.stringify(values);
}

function filterRequirementOptions(
  options: RequirementOption[],
  query: string,
): RequirementOption[] {
  const q = query.trim().toLowerCase();
  if (!q) return options;
  return options.filter(
    (item) => item.label.toLowerCase().includes(q) || item.value.toLowerCase().includes(q),
  );
}

const EMPTY_PLAN_FORM: CreatePmPlanInput = {
  code: "",
  title: "",
  description: null,
  asset_scope_type: "equipment",
  asset_scope_id: null,
  strategy_type: "fixed",
  criticality_value_id: null,
  assigned_group_id: null,
  requires_shutdown: false,
  requires_permit: false,
  is_active: true,
};

function nextUtcIso(hoursOffset = 0): string {
  const d = new Date(Date.now() + hoursOffset * 60 * 60 * 1000);
  return d.toISOString().replace(".000", "");
}

function toDateTimeLocalInputValue(isoValue: string | null | undefined): string {
  if (!isoValue) return "";
  const parsed = new Date(isoValue);
  if (Number.isNaN(parsed.getTime())) return "";
  const year = parsed.getFullYear();
  const month = String(parsed.getMonth() + 1).padStart(2, "0");
  const day = String(parsed.getDate()).padStart(2, "0");
  const hour = String(parsed.getHours()).padStart(2, "0");
  const minute = String(parsed.getMinutes()).padStart(2, "0");
  return `${year}-${month}-${day}T${hour}:${minute}`;
}

function fromDateTimeLocalInputValue(localValue: string): string | null {
  if (!localValue) return null;
  const parsed = new Date(localValue);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed.toISOString();
}

function formatIsoDateTime(isoValue: string | null | undefined): string {
  if (!isoValue) return "-";
  const parsed = new Date(isoValue);
  if (Number.isNaN(parsed.getTime())) return isoValue;
  return parsed.toLocaleString();
}

function formatEnumLabel(value: string | null | undefined): string {
  if (!value) return "-";
  return value
    .split("_")
    .filter((part) => part.length > 0)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join(" ");
}

function formatDueBasisLabel(value: string | null | undefined): string {
  if (!value) return "-";
  const separatorIndex = value.indexOf(":");
  if (separatorIndex <= 0 || separatorIndex >= value.length - 1) {
    return formatEnumLabel(value);
  }
  const kind = value.slice(0, separatorIndex);
  const payload = value.slice(separatorIndex + 1);
  const formattedPayload = formatIsoDateTime(payload);
  if (formattedPayload !== payload) {
    return `${formatEnumLabel(kind)} - ${formattedPayload}`;
  }
  return `${formatEnumLabel(kind)} - ${payload}`;
}

function parseTaskPackageToStepLines(taskPackageJson: string | null | undefined): string {
  if (!taskPackageJson) return "";
  try {
    const parsed = JSON.parse(taskPackageJson) as { steps?: unknown };
    if (Array.isArray(parsed.steps)) {
      const lines = parsed.steps
        .map((item) => {
          if (typeof item === "string") return item.trim();
          if (item && typeof item === "object" && "title" in item) {
            const title = (item as { title?: unknown }).title;
            return typeof title === "string" ? title.trim() : "";
          }
          return "";
        })
        .filter((line) => line.length > 0);
      return lines.join("\n");
    }
  } catch {
    // fallback to empty editor for invalid historical payloads
  }
  return "";
}

function buildTaskPackageFromStepLines(stepLines: string): string {
  const steps = stepLines
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => ({ title: line }));
  return JSON.stringify({ steps });
}

function makeReferenceCodeFromLabel(label: string): string {
  const base = label
    .trim()
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 40);
  const suffix = Date.now().toString().slice(-6);
  return `${base || "TASK_LIST"}_${suffix}`;
}

function isIsoBefore(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const aDt = new Date(a);
  const bDt = new Date(b);
  if (Number.isNaN(aDt.getTime()) || Number.isNaN(bDt.getTime())) return false;
  return aDt.getTime() < bDt.getTime();
}

function defaultTriggerFor(strategy: string): string {
  if (strategy === "meter") {
    return JSON.stringify({ meter_source: "operating_hours", interval_value: 100 });
  }
  if (strategy === "event") {
    return JSON.stringify({ event_code: "INSPECTION_COMPLETED" });
  }
  if (strategy === "condition") {
    return JSON.stringify({ condition_code: "VIBRATION_HIGH" });
  }
  return JSON.stringify({ interval_unit: "day", interval_value: 30 });
}

function parseTriggerBuilderModel(
  strategy: string,
  triggerDefinitionJson: string,
): TriggerBuilderModel {
  let obj: Record<string, unknown> = {};
  try {
    obj = JSON.parse(triggerDefinitionJson) as Record<string, unknown>;
  } catch {
    // keep defaults
  }
  const intervalUnitRaw = String(obj["interval_unit"] ?? "day").toLowerCase();
  const intervalUnit: TimeIntervalUnit =
    intervalUnitRaw === "week" || intervalUnitRaw === "month" || intervalUnitRaw === "year"
      ? intervalUnitRaw
      : "day";
  const timeInterval = Number(obj["interval_value"] ?? 30);
  const meterSourceRaw = String(obj["meter_source"] ?? "").toLowerCase();
  const meterSource: MeterSource = meterSourceRaw === "odometer" ? "odometer" : "operating_hours";
  const meterInterval = Number(obj["interval_value"] ?? obj["threshold_value"] ?? 100);
  const eventCode = String(obj["event_code"] ?? "INSPECTION_COMPLETED");
  const conditionCode = String(obj["condition_code"] ?? "VIBRATION_HIGH");
  if (strategy === "meter") {
    return {
      timeUnit: intervalUnit,
      timeInterval: Number.isFinite(timeInterval) && timeInterval > 0 ? timeInterval : 30,
      meterSource,
      meterInterval: Number.isFinite(meterInterval) && meterInterval > 0 ? meterInterval : 100,
      eventCode,
      conditionCode,
    };
  }
  return {
    timeUnit: intervalUnit,
    timeInterval: Number.isFinite(timeInterval) && timeInterval > 0 ? timeInterval : 30,
    meterSource,
    meterInterval: Number.isFinite(meterInterval) && meterInterval > 0 ? meterInterval : 100,
    eventCode,
    conditionCode,
  };
}

function buildTriggerDefinitionJson(strategy: string, model: TriggerBuilderModel): string {
  if (strategy === "meter") {
    return JSON.stringify({
      meter_source: model.meterSource,
      interval_value: model.meterInterval,
    });
  }
  if (strategy === "event") {
    return JSON.stringify({ event_code: model.eventCode.trim() || "INSPECTION_COMPLETED" });
  }
  if (strategy === "condition") {
    return JSON.stringify({ condition_code: model.conditionCode.trim() || "VIBRATION_HIGH" });
  }
  return JSON.stringify({
    interval_unit: model.timeUnit,
    interval_value: model.timeInterval,
  });
}

function makeEmptyVersionForm(strategy: string): CreatePmPlanVersionInput {
  return {
    effective_from: nextUtcIso(),
    effective_to: null,
    trigger_definition_json: defaultTriggerFor(strategy),
    task_package_json: JSON.stringify({ steps: [] }),
    required_parts_json: null,
    required_skills_json: JSON.stringify([]),
    required_tools_json: null,
    estimated_duration_hours: null,
    estimated_labor_cost: null,
    estimated_parts_cost: null,
    estimated_service_cost: null,
    change_reason: null,
  };
}

export function PmPage() {
  const { t } = useTranslation("pm");
  const plans = usePmStore((s) => s.plans);
  const versions = usePmStore((s) => s.versions);
  const occurrences = usePmStore((s) => s.occurrences);
  const executions = usePmStore((s) => s.executions);
  const recurringFindings = usePmStore((s) => s.recurringFindings);
  const findingsByExecutionId = usePmStore((s) => s.findingsByExecutionId);
  const metrics = usePmStore((s) => s.metrics);
  const selectedPlanId = usePmStore((s) => s.selectedPlanId);
  const loading = usePmStore((s) => s.loading);
  const saving = usePmStore((s) => s.saving);
  const error = usePmStore((s) => s.error);
  const loadPlans = usePmStore((s) => s.loadPlans);
  const selectPlan = usePmStore((s) => s.selectPlan);
  const createPlan = usePmStore((s) => s.createPlan);
  const updatePlan = usePmStore((s) => s.updatePlan);
  const transitionPlanLifecycle = usePmStore((s) => s.transitionPlanLifecycle);
  const createVersion = usePmStore((s) => s.createVersion);
  const updateVersion = usePmStore((s) => s.updateVersion);
  const publishVersion = usePmStore((s) => s.publishVersion);
  const generateOccurrences = usePmStore((s) => s.generateOccurrences);
  const transitionOccurrence = usePmStore((s) => s.transitionOccurrence);
  const executeOccurrence = usePmStore((s) => s.executeOccurrence);
  const loadFindings = usePmStore((s) => s.loadFindings);

  const [isPlanDialogOpen, setPlanDialogOpen] = useState(false);
  const [isVersionDialogOpen, setVersionDialogOpen] = useState(false);
  const [editingVersion, setEditingVersion] = useState<PmPlanVersion | null>(null);
  const [planForm, setPlanForm] = useState<CreatePmPlanInput>(EMPTY_PLAN_FORM);
  const [versionForm, setVersionForm] = useState<CreatePmPlanVersionInput>(
    makeEmptyVersionForm("fixed"),
  );
  const [triggerBuilder, setTriggerBuilder] = useState<TriggerBuilderModel>(
    parseTriggerBuilderModel("fixed", defaultTriggerFor("fixed")),
  );
  const [nextLifecycle, setNextLifecycle] = useState<string>("draft");
  const [nextOccurrenceStatus, setNextOccurrenceStatus] = useState<string>("generated");
  const [occurrenceReason, setOccurrenceReason] = useState("");
  const [executionDialogOpen, setExecutionDialogOpen] = useState(false);
  const [executionTarget, setExecutionTarget] = useState<PmOccurrence | null>(null);
  const [executionResult, setExecutionResult] = useState("completed_no_findings");
  const [executionNote, setExecutionNote] = useState("");
  const [executionReason, setExecutionReason] = useState("");
  const [findingType, setFindingType] = useState("ANOMALY");
  const [findingSeverity, setFindingSeverity] = useState("medium");
  const [findingDescription, setFindingDescription] = useState("");
  const [createFollowUpDi, setCreateFollowUpDi] = useState(false);
  const [createFollowUpWo, setCreateFollowUpWo] = useState(false);
  const [showFilters, setShowFilters] = useState(
    () => localStorage.getItem("pm-show-filters") !== "0",
  );
  const [searchInput, setSearchInput] = useState("");
  const [lifecycleFilter, setLifecycleFilter] = useState<string>("__all__");
  const [strategyFilter, setStrategyFilter] = useState<string>("__all__");
  const [taskStepsText, setTaskStepsText] = useState("");
  const [taskRefsLoading, setTaskRefsLoading] = useState(false);
  const [taskRefsSaving, setTaskRefsSaving] = useState(false);
  const [taskRefError, setTaskRefError] = useState<string | null>(null);
  const [taskRefItems, setTaskRefItems] = useState<MaintenanceTaskReference[]>([]);
  const [selectedTaskRefId, setSelectedTaskRefId] = useState<string>("__none__");
  const [newTaskListName, setNewTaskListName] = useState("");
  const [requirementsLoading, setRequirementsLoading] = useState(false);
  const [requirementsError, setRequirementsError] = useState<string | null>(null);
  const [partOptions, setPartOptions] = useState<RequirementOption[]>([]);
  const [skillOptions, setSkillOptions] = useState<RequirementOption[]>([]);
  const [toolOptions, setToolOptions] = useState<RequirementOption[]>([]);
  const [partSearch, setPartSearch] = useState("");
  const [skillSearch, setSkillSearch] = useState("");
  const [toolSearch, setToolSearch] = useState("");
  const [selectedPartCodes, setSelectedPartCodes] = useState<string[]>([]);
  const [selectedSkillCodes, setSelectedSkillCodes] = useState<string[]>([]);
  const [selectedToolCodes, setSelectedToolCodes] = useState<string[]>([]);
  const [inlineNotice, setInlineNotice] = useState<InlineNotice | null>(null);
  const [deletePlanDialogOpen, setDeletePlanDialogOpen] = useState(false);
  const [deleteVersionDialogOpen, setDeleteVersionDialogOpen] = useState(false);
  const [deleteVersionTarget, setDeleteVersionTarget] = useState<PmPlanVersion | null>(null);

  useEffect(() => {
    void loadPlans();
  }, [loadPlans]);

  useEffect(() => {
    let cancelled = false;
    const loadRequirements = async () => {
      setRequirementsLoading(true);
      setRequirementsError(null);
      try {
        const [articles, skills, domains] = await Promise.all([
          listInventoryArticles({ search: null }),
          listPersonnelSkillReferenceValues(),
          listReferenceDomains(),
        ]);
        if (cancelled) return;

        const articleOptions = articles
          .filter((row) => row.is_active === 1)
          .map((row) => ({
            value: row.article_code,
            label: `${row.article_code} - ${row.article_name}`,
          }))
          .sort((a, b) => a.label.localeCompare(b.label));
        setPartOptions(articleOptions);

        const skillOpts = skills
          .map((row) => ({
            value: row.code,
            label: `${row.code} - ${row.label}`,
          }))
          .sort((a, b) => a.label.localeCompare(b.label));
        setSkillOptions(skillOpts);

        const toolDomainIds = domains
          .filter(
            (domain) =>
              domain.code.toUpperCase().includes("TOOL") ||
              domain.name.toUpperCase().includes("TOOL"),
          )
          .map((domain) => domain.id);
        const refTools: RequirementOption[] = [];
        for (const domainId of toolDomainIds) {
          const sets = await listReferenceSets(domainId);
          for (const set of sets) {
            const values = await listReferenceValues(set.id);
            for (const value of values) {
              if (!value.is_active) continue;
              refTools.push({
                value: value.code,
                label: `${value.code} - ${value.label}`,
              });
            }
          }
        }
        if (cancelled) return;
        const dedupedTools = Array.from(
          new Map(
            (refTools.length > 0 ? refTools : articleOptions).map(
              (item) => [item.value, item] as const,
            ),
          ).values(),
        ).sort((a, b) => a.label.localeCompare(b.label));
        setToolOptions(dedupedTools);
      } catch (err) {
        if (cancelled) return;
        setRequirementsError(toErrorMessage(err));
      } finally {
        if (!cancelled) setRequirementsLoading(false);
      }
    };
    void loadRequirements();
    return () => {
      cancelled = true;
    };
  }, []);

  const selectedPlan = useMemo(
    () => plans.find((plan) => plan.id === selectedPlanId) ?? null,
    [plans, selectedPlanId],
  );

  const filteredPlans = useMemo(() => {
    const q = searchInput.trim().toLowerCase();
    return plans.filter((plan) => {
      if (lifecycleFilter !== "__all__" && plan.lifecycle_status !== lifecycleFilter) return false;
      if (strategyFilter !== "__all__" && plan.strategy_type !== strategyFilter) return false;
      if (!q) return true;
      return plan.code.toLowerCase().includes(q) || plan.title.toLowerCase().includes(q);
    });
  }, [plans, lifecycleFilter, strategyFilter, searchInput]);

  const visiblePartOptions = useMemo(
    () => filterRequirementOptions(partOptions, partSearch),
    [partOptions, partSearch],
  );
  const visibleSkillOptions = useMemo(
    () => filterRequirementOptions(skillOptions, skillSearch),
    [skillOptions, skillSearch],
  );
  const visibleToolOptions = useMemo(
    () => filterRequirementOptions(toolOptions, toolSearch),
    [toolOptions, toolSearch],
  );

  const canPublishForSelectedPlanLifecycle = useMemo(() => {
    if (!selectedPlan) return false;
    return PUBLISHABLE_PLAN_LIFECYCLES.has(selectedPlan.lifecycle_status);
  }, [selectedPlan]);

  const lifecycleStatusOptions = useMemo(() => {
    if (!selectedPlan) return PLAN_STATUS_OPTIONS;
    const allowed = new Set(allowedPlanLifecycleTargets(selectedPlan.lifecycle_status));
    return PLAN_STATUS_OPTIONS.filter((status) => allowed.has(status));
  }, [selectedPlan]);

  const hasPublishedVersion = useMemo(
    () => versions.some((version) => version.status === "published"),
    [versions],
  );

  const canGenerateForSelectedPlan = useMemo(() => {
    if (!selectedPlan) return false;
    if (selectedPlan.is_active !== 1) return false;
    if (!["active", "suspended"].includes(selectedPlan.lifecycle_status)) return false;
    if (!hasPublishedVersion && selectedPlan.current_version_id == null) return false;
    return true;
  }, [hasPublishedVersion, selectedPlan]);

  const generationReadinessHint = useMemo(() => {
    if (!selectedPlan) return t("hints.generationNeedPlan");
    if (
      selectedPlan.is_active !== 1 ||
      !["active", "suspended"].includes(selectedPlan.lifecycle_status)
    ) {
      return t("hints.generationNeedLifecycle");
    }
    if (!hasPublishedVersion && selectedPlan.current_version_id == null)
      return t("hints.generationNeedVersion");
    return t("hints.generationReady");
  }, [hasPublishedVersion, selectedPlan, t]);

  useEffect(() => {
    if (!selectedPlan) return;
    setNextLifecycle(selectedPlan.lifecycle_status);
  }, [selectedPlan]);

  const planFormValid = planForm.code.trim().length > 0 && planForm.title.trim().length > 0;

  const loadMaintenanceTaskReferences = async () => {
    setTaskRefsLoading(true);
    setTaskRefError(null);
    try {
      const domains = await listReferenceDomains();
      const domain = domains.find((item) => item.code === PM_TASK_LIST_DOMAIN_CODE);
      if (!domain) {
        setTaskRefItems([]);
        return;
      }

      const sets = await listReferenceSets(domain.id);
      if (sets.length === 0) {
        setTaskRefItems([]);
        return;
      }

      const candidates: MaintenanceTaskReference[] = [];
      for (const set of sets) {
        const values = await listReferenceValues(set.id);
        for (const value of values) {
          if (!value.is_active) continue;
          let taskPackageJson: string | null = null;
          if (value.metadata_json) {
            try {
              const metadata = JSON.parse(value.metadata_json) as { task_package_json?: unknown };
              if (typeof metadata.task_package_json === "string") {
                taskPackageJson = metadata.task_package_json;
              }
            } catch {
              // ignore invalid metadata payloads
            }
          }
          candidates.push({
            id: value.id,
            setId: set.id,
            setStatus: set.status,
            label: value.label,
            description: value.description,
            taskPackageJson,
          });
        }
      }

      candidates.sort((a, b) => a.label.localeCompare(b.label));
      setTaskRefItems(candidates);
    } catch (err) {
      setTaskRefError(toErrorMessage(err));
    } finally {
      setTaskRefsLoading(false);
    }
  };

  const ensureTaskReferenceDraftSet = async (): Promise<number> => {
    const domains = await listReferenceDomains();
    let domain = domains.find((item) => item.code === PM_TASK_LIST_DOMAIN_CODE);
    if (!domain) {
      domain = await createReferenceDomain({
        code: PM_TASK_LIST_DOMAIN_CODE,
        name: "PM Maintenance Task List",
        structure_type: "flat",
        governance_level: "tenant_managed",
        is_extendable: true,
      });
    }

    const sets = await listReferenceSets(domain.id);
    const draftSet =
      sets
        .filter((item) => item.status === "draft")
        .sort((a, b) => b.version_no - a.version_no)[0] ?? null;
    if (draftSet) return draftSet.id;

    const createdSet = await createDraftReferenceSet(domain.id);
    return createdSet.id;
  };

  const applyTaskReferenceToForm = (referenceIdRaw: string) => {
    setSelectedTaskRefId(referenceIdRaw);
    if (referenceIdRaw === "__none__") return;
    const referenceId = Number(referenceIdRaw);
    const selected = taskRefItems.find((item) => item.id === referenceId);
    if (!selected) return;
    const taskPackageJson =
      selected.taskPackageJson ??
      JSON.stringify({
        steps: (selected.description ?? "")
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter((line) => line.length > 0)
          .map((line) => ({ title: line })),
      });
    setVersionForm((prev) => ({ ...prev, task_package_json: taskPackageJson }));
    setTaskStepsText(parseTaskPackageToStepLines(taskPackageJson));
  };

  const saveTaskListToReferences = async () => {
    const label = newTaskListName.trim();
    if (!label) {
      setTaskRefError(t("errors.taskListNameRequired"));
      return;
    }
    const taskPackageJson = buildTaskPackageFromStepLines(taskStepsText);
    const firstLines = taskStepsText
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
      .slice(0, 8)
      .join("\n");

    setTaskRefsSaving(true);
    setTaskRefError(null);
    try {
      const setId = await ensureTaskReferenceDraftSet();
      const created = await createReferenceValue({
        set_id: setId,
        code: makeReferenceCodeFromLabel(label),
        label,
        description: firstLines || null,
        metadata_json: JSON.stringify({ task_package_json: taskPackageJson, source: "pm.version" }),
      });
      setSelectedTaskRefId(created.id.toString());
      setVersionForm((prev) => ({ ...prev, task_package_json: taskPackageJson }));
      setTaskStepsText(parseTaskPackageToStepLines(taskPackageJson));
      setNewTaskListName("");
      await loadMaintenanceTaskReferences();
    } catch (err) {
      setTaskRefError(toErrorMessage(err));
    } finally {
      setTaskRefsSaving(false);
    }
  };

  const applyRequiredSelections = (
    form: Pick<
      CreatePmPlanVersionInput,
      "required_parts_json" | "required_skills_json" | "required_tools_json"
    >,
  ) => {
    setSelectedPartCodes(parseRequiredCodeArray(form.required_parts_json));
    setSelectedSkillCodes(parseRequiredCodeArray(form.required_skills_json));
    setSelectedToolCodes(parseRequiredCodeArray(form.required_tools_json));
  };

  const updateRequiredPartCodes = (codes: string[]) => {
    setSelectedPartCodes(codes);
    setVersionForm((prev) => ({
      ...prev,
      required_parts_json: toRequiredCodeJson(codes),
    }));
  };

  const updateRequiredSkillCodes = (codes: string[]) => {
    setSelectedSkillCodes(codes);
    setVersionForm((prev) => ({
      ...prev,
      required_skills_json: toRequiredCodeJson(codes),
    }));
  };

  const updateRequiredToolCodes = (codes: string[]) => {
    setSelectedToolCodes(codes);
    setVersionForm((prev) => ({
      ...prev,
      required_tools_json: toRequiredCodeJson(codes),
    }));
  };

  const openCreatePlan = () => {
    setPlanForm(EMPTY_PLAN_FORM);
    setPlanDialogOpen(true);
  };

  const openCreateVersion = () => {
    if (!selectedPlan) return;
    setEditingVersion(null);
    const emptyForm = makeEmptyVersionForm(selectedPlan.strategy_type);
    setVersionForm(emptyForm);
    setTaskStepsText(parseTaskPackageToStepLines(emptyForm.task_package_json));
    setSelectedTaskRefId("__none__");
    setNewTaskListName("");
    setTaskRefError(null);
    setPartSearch("");
    setSkillSearch("");
    setToolSearch("");
    applyRequiredSelections(emptyForm);
    setTriggerBuilder(
      parseTriggerBuilderModel(selectedPlan.strategy_type, emptyForm.trigger_definition_json),
    );
    void loadMaintenanceTaskReferences();
    setVersionDialogOpen(true);
  };

  const openEditVersion = (version: PmPlanVersion) => {
    const nextForm = {
      effective_from: version.effective_from,
      effective_to: version.effective_to,
      trigger_definition_json: version.trigger_definition_json,
      task_package_json: version.task_package_json,
      required_parts_json: version.required_parts_json,
      required_skills_json: version.required_skills_json,
      required_tools_json: version.required_tools_json,
      estimated_duration_hours: version.estimated_duration_hours,
      estimated_labor_cost: version.estimated_labor_cost,
      estimated_parts_cost: version.estimated_parts_cost,
      estimated_service_cost: version.estimated_service_cost,
      change_reason: version.change_reason,
    };
    setEditingVersion(version);
    setVersionForm(nextForm);
    setTaskStepsText(parseTaskPackageToStepLines(version.task_package_json));
    setSelectedTaskRefId("__none__");
    setNewTaskListName("");
    setTaskRefError(null);
    setPartSearch("");
    setSkillSearch("");
    setToolSearch("");
    applyRequiredSelections(nextForm);
    setTriggerBuilder(
      parseTriggerBuilderModel(
        selectedPlan?.strategy_type ?? "fixed",
        version.trigger_definition_json,
      ),
    );
    void loadMaintenanceTaskReferences();
    setVersionDialogOpen(true);
  };

  const submitCreatePlan = async () => {
    await createPlan(planForm);
    setPlanDialogOpen(false);
  };

  const submitQuickPlanUpdate = async () => {
    if (!selectedPlan) return;
    setInlineNotice({ variant: "info", message: t("messages.touchStarted") });
    await updatePlan(selectedPlan.id, selectedPlan.row_version, {
      title: selectedPlan.title,
      description: selectedPlan.description,
      asset_scope_type: selectedPlan.asset_scope_type,
      asset_scope_id: selectedPlan.asset_scope_id,
      strategy_type: selectedPlan.strategy_type,
      criticality_value_id: selectedPlan.criticality_value_id,
      assigned_group_id: selectedPlan.assigned_group_id,
      requires_shutdown: selectedPlan.requires_shutdown === 1,
      requires_permit: selectedPlan.requires_permit === 1,
      is_active: selectedPlan.is_active === 1,
    });
    setInlineNotice({ variant: "success", message: t("messages.touchCompleted") });
  };

  const submitLifecycleTransition = async () => {
    if (!selectedPlan || nextLifecycle === selectedPlan.lifecycle_status) return;
    await transitionPlanLifecycle({
      plan_id: selectedPlan.id,
      expected_row_version: selectedPlan.row_version,
      next_status: nextLifecycle,
    });
  };

  const submitVersion = async () => {
    if (!selectedPlan) return;
    const triggerDefinitionJson = buildTriggerDefinitionJson(
      selectedPlan.strategy_type,
      triggerBuilder,
    );
    const taskPackageJson = buildTaskPackageFromStepLines(taskStepsText);
    if (editingVersion) {
      await updateVersion(editingVersion.id, editingVersion.row_version, {
        effective_from: versionForm.effective_from,
        effective_to: versionForm.effective_to ?? null,
        trigger_definition_json: triggerDefinitionJson,
        task_package_json: taskPackageJson,
        required_parts_json: versionForm.required_parts_json ?? null,
        required_skills_json: versionForm.required_skills_json ?? null,
        required_tools_json: versionForm.required_tools_json ?? null,
        estimated_duration_hours: versionForm.estimated_duration_hours ?? null,
        estimated_labor_cost: versionForm.estimated_labor_cost ?? null,
        estimated_parts_cost: versionForm.estimated_parts_cost ?? null,
        estimated_service_cost: versionForm.estimated_service_cost ?? null,
        change_reason: versionForm.change_reason ?? null,
      });
    } else {
      await createVersion(selectedPlan.id, {
        ...versionForm,
        trigger_definition_json: triggerDefinitionJson,
        task_package_json: taskPackageJson,
      });
    }
    setVersionDialogOpen(false);
  };

  const openDeletePlanDialog = () => {
    if (!selectedPlan) return;
    setDeletePlanDialogOpen(true);
  };

  const confirmDeletePlan = async () => {
    if (!selectedPlan) return;
    try {
      await invoke("delete_pm_plan", {
        planId: selectedPlan.id,
        expectedRowVersion: selectedPlan.row_version,
      });
      setInlineNotice({
        variant: "success",
        message: t("messages.planDeleted", { code: selectedPlan.code }),
      });
      setDeletePlanDialogOpen(false);
      await loadPlans();
    } catch (err) {
      setInlineNotice({ variant: "error", message: toErrorMessage(err) });
    }
  };

  const openDeleteVersionDialog = (version: PmPlanVersion) => {
    setDeleteVersionTarget(version);
    setDeleteVersionDialogOpen(true);
  };

  const confirmDeleteVersion = async () => {
    if (!deleteVersionTarget || !selectedPlan) return;
    try {
      await invoke("delete_pm_plan_version", {
        versionId: deleteVersionTarget.id,
        expectedRowVersion: deleteVersionTarget.row_version,
      });
      setInlineNotice({
        variant: "success",
        message: t("messages.versionDeleted", { version: deleteVersionTarget.version_no }),
      });
      setDeleteVersionDialogOpen(false);
      setDeleteVersionTarget(null);
      await selectPlan(selectedPlan.id);
    } catch (err) {
      setInlineNotice({ variant: "error", message: toErrorMessage(err) });
    }
  };

  const runGeneration = async () => {
    if (!selectedPlan) {
      setInlineNotice({ variant: "warning", message: t("hints.generationNeedPlan") });
      return;
    }
    if (!canGenerateForSelectedPlan) {
      setInlineNotice({ variant: "warning", message: generationReadinessHint });
      return;
    }

    setInlineNotice({ variant: "info", message: t("messages.generationStarted") });
    const beforeIds = new Set(occurrences.map((occurrence) => occurrence.id));
    try {
      await generateOccurrences({
        pm_plan_id: selectedPlanId,
        horizon_days: 30,
        as_of: nextUtcIso(),
        event_codes: ["INSPECTION_COMPLETED"],
        condition_codes: ["VIBRATION_HIGH"],
      });
      const updatedOccurrences = usePmStore
        .getState()
        .occurrences.filter((item) => item.pm_plan_id === selectedPlan.id);
      const generatedCount = updatedOccurrences.filter((item) => !beforeIds.has(item.id)).length;
      if (generatedCount > 0) {
        setInlineNotice({
          variant: "success",
          message: t("messages.generationCreated", { count: generatedCount }),
        });
      } else {
        setInlineNotice({ variant: "warning", message: t("messages.generationNoNew") });
      }
    } catch (err) {
      setInlineNotice({ variant: "error", message: toErrorMessage(err) });
    }
  };

  const openExecutionDialog = (occurrence: PmOccurrence) => {
    setExecutionTarget(occurrence);
    setExecutionResult("completed_no_findings");
    setExecutionNote("");
    setExecutionReason("");
    setFindingType("ANOMALY");
    setFindingSeverity("medium");
    setFindingDescription("");
    setCreateFollowUpDi(false);
    setCreateFollowUpWo(false);
    setExecutionDialogOpen(true);
  };

  const submitExecution = async () => {
    if (!executionTarget) return;
    const payload: ExecutePmOccurrenceInput = {
      occurrence_id: executionTarget.id,
      expected_occurrence_row_version: executionTarget.row_version,
      execution_result: executionResult,
      note: executionNote || null,
      actor_id: 1,
      defer_reason_code:
        executionResult === "deferred" && executionReason !== "none"
          ? executionReason || null
          : null,
      miss_reason_code:
        executionResult === "missed" && executionReason !== "none" ? executionReason || null : null,
      findings:
        executionResult === "completed_with_findings"
          ? [
              {
                finding_type: findingType,
                severity: findingSeverity,
                description: findingDescription,
                create_follow_up_di: createFollowUpDi,
                create_follow_up_work_order: createFollowUpWo,
              },
            ]
          : [],
    };
    const result = await executeOccurrence(payload);
    await loadFindings(result.execution.id);
    setExecutionDialogOpen(false);
  };

  const submitOccurrenceTransition = async (occurrence: PmOccurrence) => {
    if (nextOccurrenceStatus === occurrence.status) {
      setInlineNotice({ variant: "warning", message: t("hints.occurrenceSameStatus") });
      return;
    }
    await transitionOccurrence({
      occurrence_id: occurrence.id,
      expected_row_version: occurrence.row_version,
      next_status: nextOccurrenceStatus,
      reason_code: occurrenceReason && occurrenceReason !== "none" ? occurrenceReason : null,
      generate_work_order: nextOccurrenceStatus === "ready_for_scheduling",
      actor_id: 1,
    });
  };

  const toggleFilters = () => {
    setShowFilters((prev) => {
      const next = !prev;
      localStorage.setItem("pm-show-filters", next ? "1" : "0");
      return next;
    });
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-6 py-3">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
          <Badge variant="secondary">{plans.length}</Badge>
        </div>
        <div className="flex items-center gap-2">
          <PermissionGate anyOf={["pm.create", "pm.manage"]}>
            <Button size="sm" className="gap-1.5" onClick={openCreatePlan}>
              <Plus className="h-3.5 w-3.5" />
              {t("actions.newPlan")}
            </Button>
          </PermissionGate>
          <PermissionGate anyOf={["pm.create", "pm.manage"]}>
            <Button
              size="sm"
              variant="outline"
              className="gap-1.5"
              onClick={() => void runGeneration()}
              disabled={saving || !canGenerateForSelectedPlan}
              title={generationReadinessHint}
            >
              <Play className="h-3.5 w-3.5" />
              {t("actions.generate")}
            </Button>
          </PermissionGate>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadPlans()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
            {t("actions.refresh")}
          </Button>
        </div>
      </div>

      {error ? <div className="px-6 py-2 text-sm text-destructive">{error}</div> : null}
      {taskRefError ? (
        <div className="px-6 py-2 text-sm text-destructive">{taskRefError}</div>
      ) : null}
      {inlineNotice ? (
        <div
          className={`mx-6 mb-2 rounded-md border px-3 py-2 text-sm ${
            inlineNotice.variant === "success"
              ? "border-green-300 bg-green-50 text-green-900"
              : inlineNotice.variant === "warning"
                ? "border-amber-300 bg-amber-50 text-amber-900"
                : inlineNotice.variant === "error"
                  ? "border-destructive/40 bg-destructive/10 text-destructive"
                  : "border-blue-300 bg-blue-50 text-blue-900"
          }`}
        >
          {inlineNotice.message}
        </div>
      ) : null}

      <div className="grid grid-cols-4 gap-3 px-6 py-3">
        <div className="rounded-md border p-3">
          <div className="text-xs text-text-muted">{t("metrics.overdue")}</div>
          <div className="text-lg font-semibold">{metrics?.overdue_count ?? 0}</div>
        </div>
        <div className="rounded-md border p-3">
          <div className="text-xs text-text-muted">{t("metrics.dueToday")}</div>
          <div className="text-lg font-semibold">{metrics?.due_today_count ?? 0}</div>
        </div>
        <div className="rounded-md border p-3">
          <div className="text-xs text-text-muted">{t("metrics.next7d")}</div>
          <div className="text-lg font-semibold">{metrics?.due_next_7d_count ?? 0}</div>
        </div>
        <div className="rounded-md border p-3">
          <div className="text-xs text-text-muted">{t("metrics.ready")}</div>
          <div className="text-lg font-semibold">{metrics?.ready_for_scheduling_count ?? 0}</div>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 px-6 pb-3">
        <Button
          size="sm"
          variant={showFilters ? "secondary" : "outline"}
          className="gap-1.5"
          onClick={toggleFilters}
        >
          <Filter className="h-3.5 w-3.5" />
          {t("page.filters")}
        </Button>

        <div className="relative w-full min-w-[240px] flex-1 sm:max-w-[420px]">
          <Search className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
          <Input
            className="h-9 pl-8 pr-8"
            placeholder={t("page.searchPlaceholder")}
            value={searchInput}
            onChange={(e) => setSearchInput(e.target.value)}
          />
          {searchInput ? (
            <button
              type="button"
              className="absolute right-2 top-2 rounded p-1 text-text-muted hover:bg-surface-2"
              onClick={() => setSearchInput("")}
              aria-label={t("page.clearSearch")}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </div>

        {showFilters ? (
          <>
            <Select value={lifecycleFilter} onValueChange={setLifecycleFilter}>
              <SelectTrigger className="h-9 w-[180px]">
                <SelectValue placeholder={t("fields.lifecycle")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__all__">{t("page.allLifecycles")}</SelectItem>
                {PLAN_STATUS_OPTIONS.map((status) => (
                  <SelectItem key={status} value={status}>
                    {formatEnumLabel(status)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select value={strategyFilter} onValueChange={setStrategyFilter}>
              <SelectTrigger className="h-9 w-[170px]">
                <SelectValue placeholder={t("fields.strategy")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__all__">{t("page.allStrategies")}</SelectItem>
                {STRATEGY_OPTIONS.map((strategy) => (
                  <SelectItem key={strategy} value={strategy}>
                    {formatEnumLabel(strategy)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        ) : null}
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-12 gap-4 px-6 pb-6">
        <div className="col-span-4 min-h-0 overflow-auto rounded-md border">
          <table className="w-full text-sm">
            <thead className="bg-surface-2 text-left">
              <tr>
                <th className="px-3 py-2">{t("table.code")}</th>
                <th className="px-3 py-2">{t("table.strategy")}</th>
                <th className="px-3 py-2">{t("table.status")}</th>
              </tr>
            </thead>
            <tbody>
              {filteredPlans.map((plan) => (
                <tr
                  key={plan.id}
                  className={`cursor-pointer border-t hover:bg-surface-2 ${selectedPlanId === plan.id ? "bg-surface-2" : ""}`}
                  onClick={() => void selectPlan(plan.id)}
                >
                  <td className="px-3 py-2 font-medium">{plan.code}</td>
                  <td className="px-3 py-2">{formatEnumLabel(plan.strategy_type)}</td>
                  <td className="px-3 py-2">{formatEnumLabel(plan.lifecycle_status)}</td>
                </tr>
              ))}
              {filteredPlans.length === 0 ? (
                <tr className="border-t">
                  <td className="px-3 py-4 text-sm text-text-muted" colSpan={3}>
                    {t("empty.noPlansMatch")}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
        </div>

        <div className="col-span-8 min-h-0 overflow-auto rounded-md border p-4">
          {selectedPlan ? (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div>
                  <h2 className="text-lg font-semibold">{selectedPlan.title}</h2>
                  <p className="text-sm text-text-muted">{selectedPlan.code}</p>
                </div>
                <div className="flex items-center gap-2">
                  <Badge variant={selectedPlan.is_active === 1 ? "secondary" : "outline"}>
                    {selectedPlan.is_active === 1 ? t("labels.active") : t("labels.inactive")}
                  </Badge>
                  <Badge variant="outline">{formatEnumLabel(selectedPlan.lifecycle_status)}</Badge>
                </div>
              </div>

              <PermissionGate anyOf={["pm.edit", "pm.manage"]}>
                <div className="grid items-end gap-3 md:grid-cols-[1fr_auto_auto]">
                  <div className="space-y-1">
                    <Label>{t("fields.lifecycle")}</Label>
                    <Select value={nextLifecycle} onValueChange={setNextLifecycle}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {lifecycleStatusOptions.map((status) => (
                          <SelectItem key={status} value={status}>
                            {formatEnumLabel(status)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    variant="outline"
                    onClick={() => void submitLifecycleTransition()}
                    disabled={saving}
                  >
                    {t("actions.transition")}
                  </Button>
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={() => void submitQuickPlanUpdate()}
                    disabled={saving}
                    title={t("actions.touch")}
                    aria-label={t("actions.touch")}
                  >
                    <RefreshCw className="h-4 w-4" />
                  </Button>
                  <PermissionGate anyOf={["pm.delete", "pm.manage"]}>
                    <Button
                      variant="outline"
                      size="icon"
                      onClick={openDeletePlanDialog}
                      disabled={saving || selectedPlan.lifecycle_status !== "draft"}
                      title={
                        selectedPlan.lifecycle_status !== "draft"
                          ? t("hints.deletePlanDraftOnly")
                          : t("actions.deletePlan")
                      }
                      aria-label={t("actions.deletePlan")}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </PermissionGate>
                </div>
              </PermissionGate>

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <h3 className="font-medium">{t("sections.versions")}</h3>
                  <PermissionGate anyOf={["pm.create", "pm.manage"]}>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={openCreateVersion}
                      className="gap-1.5"
                    >
                      <Plus className="h-3.5 w-3.5" />
                      {t("actions.newVersion")}
                    </Button>
                  </PermissionGate>
                </div>
                <table className="w-full text-sm">
                  <thead className="bg-surface-2 text-left">
                    <tr>
                      <th className="px-3 py-2">{t("table.version")}</th>
                      <th className="px-3 py-2">{t("table.window")}</th>
                      <th className="px-3 py-2">{t("table.status")}</th>
                      <th className="px-3 py-2">{t("table.actions")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {versions.map((version) => (
                      <tr key={version.id} className="border-t">
                        <td className="px-3 py-2">v{version.version_no}</td>
                        <td className="px-3 py-2">
                          {formatIsoDateTime(version.effective_from)}
                          {version.effective_to
                            ? ` -> ${formatIsoDateTime(version.effective_to)}`
                            : ""}
                        </td>
                        <td className="px-3 py-2">{formatEnumLabel(version.status)}</td>
                        <td className="px-3 py-2">
                          <div className="flex gap-2">
                            <PermissionGate anyOf={["pm.edit", "pm.manage"]}>
                              <Button
                                size="sm"
                                variant="outline"
                                disabled={saving || version.status !== "draft"}
                                onClick={() => openEditVersion(version)}
                                title={
                                  version.status !== "draft"
                                    ? t("hints.editVersionDraftOnly")
                                    : undefined
                                }
                              >
                                {t("actions.editVersion")}
                              </Button>
                              <Button
                                size="sm"
                                disabled={
                                  saving ||
                                  version.status !== "draft" ||
                                  !canPublishForSelectedPlanLifecycle
                                }
                                onClick={() => void publishVersion(version.id, version.row_version)}
                                title={
                                  version.status !== "draft"
                                    ? t("hints.publishDraftOnly")
                                    : !canPublishForSelectedPlanLifecycle
                                      ? t("hints.publishBlockedByLifecycle")
                                      : undefined
                                }
                              >
                                {t("actions.publish")}
                              </Button>
                            </PermissionGate>
                            <PermissionGate anyOf={["pm.delete", "pm.manage"]}>
                              <Button
                                size="sm"
                                variant="destructive"
                                disabled={saving || version.status !== "draft"}
                                onClick={() => openDeleteVersionDialog(version)}
                                title={
                                  version.status !== "draft"
                                    ? t("hints.deleteVersionDraftOnly")
                                    : t("actions.deleteVersion")
                                }
                              >
                                {t("actions.delete")}
                              </Button>
                            </PermissionGate>
                          </div>
                          {version.status === "draft" && !canPublishForSelectedPlanLifecycle ? (
                            <p className="mt-1 text-xs text-text-muted">
                              {t("hints.publishBlockedByLifecycle")}
                            </p>
                          ) : null}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="space-y-2">
                <h3 className="font-medium">{t("sections.occurrences")}</h3>
                <table className="w-full text-sm">
                  <thead className="bg-surface-2 text-left">
                    <tr>
                      <th className="px-3 py-2">{t("table.due")}</th>
                      <th className="px-3 py-2">{t("table.basis")}</th>
                      <th className="px-3 py-2">{t("table.status")}</th>
                      <th className="px-3 py-2">{t("table.workOrder")}</th>
                      <th className="px-3 py-2">{t("table.actions")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {occurrences.map((occurrence) => (
                      <tr key={occurrence.id} className="border-t">
                        <td className="px-3 py-2">{formatIsoDateTime(occurrence.due_at)}</td>
                        <td className="px-3 py-2">{formatDueBasisLabel(occurrence.due_basis)}</td>
                        <td className="px-3 py-2">{formatEnumLabel(occurrence.status)}</td>
                        <td className="px-3 py-2">{occurrence.linked_work_order_code ?? "-"}</td>
                        <td className="px-3 py-2">
                          <div className="flex items-center gap-2">
                            <Select
                              value={nextOccurrenceStatus}
                              onValueChange={setNextOccurrenceStatus}
                            >
                              <SelectTrigger className="h-8 w-[150px]">
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                {OCCURRENCE_STATUS_OPTIONS.map((status) => (
                                  <SelectItem key={status} value={status}>
                                    {formatEnumLabel(status)}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                            <Select
                              value={occurrenceReason || "none"}
                              onValueChange={setOccurrenceReason}
                            >
                              <SelectTrigger className="h-8 w-[200px]">
                                <SelectValue placeholder={t("fields.reason")} />
                              </SelectTrigger>
                              <SelectContent>
                                {REASON_OPTIONS.map((reason) => (
                                  <SelectItem key={reason} value={reason}>
                                    {reason === "none" ? t("labels.none") : t(`reasons.${reason}`)}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                            <PermissionGate anyOf={["pm.edit", "pm.manage"]}>
                              <Button
                                size="sm"
                                variant="outline"
                                disabled={saving || nextOccurrenceStatus === occurrence.status}
                                onClick={() => void submitOccurrenceTransition(occurrence)}
                                title={
                                  nextOccurrenceStatus === occurrence.status
                                    ? t("hints.occurrenceSameStatus")
                                    : undefined
                                }
                              >
                                {t("actions.transitionOccurrence")}
                              </Button>
                              <Button
                                size="sm"
                                onClick={() => openExecutionDialog(occurrence)}
                                disabled={saving}
                              >
                                {t("actions.logExecution")}
                              </Button>
                            </PermissionGate>
                          </div>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>

              <div className="space-y-2">
                <h3 className="font-medium">{t("sections.executions")}</h3>
                <table className="w-full text-sm">
                  <thead className="bg-surface-2 text-left">
                    <tr>
                      <th className="px-3 py-2">{t("table.occurrence")}</th>
                      <th className="px-3 py-2">{t("table.result")}</th>
                      <th className="px-3 py-2">{t("table.workOrder")}</th>
                      <th className="px-3 py-2">{t("table.actualDuration")}</th>
                      <th className="px-3 py-2">{t("table.actions")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {executions.map((execution) => (
                      <tr key={execution.id} className="border-t">
                        <td className="px-3 py-2">#{execution.pm_occurrence_id}</td>
                        <td className="px-3 py-2">{formatEnumLabel(execution.execution_result)}</td>
                        <td className="px-3 py-2">{execution.work_order_code ?? "-"}</td>
                        <td className="px-3 py-2">{execution.actual_duration_hours ?? "-"}</td>
                        <td className="px-3 py-2">
                          <Button
                            size="sm"
                            variant="outline"
                            onClick={() => void loadFindings(execution.id)}
                          >
                            {t("actions.viewFindings")}
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                {executions.map((execution) => {
                  const findings = findingsByExecutionId[execution.id] ?? [];
                  if (findings.length === 0) return null;
                  return (
                    <div key={`findings-${execution.id}`} className="rounded-md border p-2 text-sm">
                      <div className="mb-1 font-medium">
                        {t("sections.findingsForExecution", { id: execution.id })}
                      </div>
                      {findings.map((finding) => (
                        <div
                          key={finding.id}
                          className="mb-1 border-t pt-1 first:border-t-0 first:pt-0"
                        >
                          <span className="font-medium">
                            {formatEnumLabel(finding.finding_type)}
                          </span>{" "}
                          - {finding.description}
                          {" | "}
                          DI: {finding.follow_up_di_code ?? "-"} | WO:{" "}
                          {finding.follow_up_work_order_code ?? "-"}
                        </div>
                      ))}
                    </div>
                  );
                })}
              </div>

              <div className="space-y-2">
                <h3 className="font-medium">{t("sections.recurringFindings")}</h3>
                <table className="w-full text-sm">
                  <thead className="bg-surface-2 text-left">
                    <tr>
                      <th className="px-3 py-2">{t("table.findingType")}</th>
                      <th className="px-3 py-2">{t("table.plan")}</th>
                      <th className="px-3 py-2">{t("table.count")}</th>
                      <th className="px-3 py-2">{t("table.lastSeen")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {recurringFindings.map((item) => (
                      <tr key={`${item.pm_plan_id}-${item.finding_type}`} className="border-t">
                        <td className="px-3 py-2">{formatEnumLabel(item.finding_type)}</td>
                        <td className="px-3 py-2">{item.plan_code ?? item.pm_plan_id}</td>
                        <td className="px-3 py-2">{item.occurrence_count}</td>
                        <td className="px-3 py-2">{formatIsoDateTime(item.last_seen_at)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          ) : (
            <p className="text-sm text-text-muted">{t("empty.noPlanSelected")}</p>
          )}
        </div>
      </div>

      <Dialog open={executionDialogOpen} onOpenChange={setExecutionDialogOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("dialogs.execution.title")}</DialogTitle>
            <DialogDescription>{t("dialogs.execution.description")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t("fields.executionResult")}</Label>
              <Select value={executionResult} onValueChange={setExecutionResult}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="completed_no_findings">
                    {formatEnumLabel("completed_no_findings")}
                  </SelectItem>
                  <SelectItem value="completed_with_findings">
                    {formatEnumLabel("completed_with_findings")}
                  </SelectItem>
                  <SelectItem value="deferred">{formatEnumLabel("deferred")}</SelectItem>
                  <SelectItem value="missed">{formatEnumLabel("missed")}</SelectItem>
                  <SelectItem value="cancelled">{formatEnumLabel("cancelled")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("fields.reason")}</Label>
              <Select value={executionReason || "none"} onValueChange={setExecutionReason}>
                <SelectTrigger>
                  <SelectValue placeholder={t("fields.reason")} />
                </SelectTrigger>
                <SelectContent>
                  {REASON_OPTIONS.map((reason) => (
                    <SelectItem key={reason} value={reason}>
                      {reason === "none" ? t("labels.none") : t(`reasons.${reason}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1 md:col-span-2">
              <Label>{t("fields.description")}</Label>
              <Textarea
                rows={2}
                value={executionNote}
                onChange={(e) => setExecutionNote(e.target.value)}
              />
            </div>
            {executionResult === "completed_with_findings" ? (
              <>
                <div className="space-y-1">
                  <Label>{t("fields.findingType")}</Label>
                  <Input value={findingType} onChange={(e) => setFindingType(e.target.value)} />
                </div>
                <div className="space-y-1">
                  <Label>{t("fields.severity")}</Label>
                  <Input
                    value={findingSeverity}
                    onChange={(e) => setFindingSeverity(e.target.value)}
                  />
                </div>
                <div className="space-y-1 md:col-span-2">
                  <Label>{t("fields.findingDescription")}</Label>
                  <Textarea
                    rows={2}
                    value={findingDescription}
                    onChange={(e) => setFindingDescription(e.target.value)}
                  />
                </div>
                <div className="flex items-center gap-2 md:col-span-2">
                  <Button
                    variant={createFollowUpDi ? "default" : "outline"}
                    onClick={() => setCreateFollowUpDi((v) => !v)}
                  >
                    {t("actions.createFollowUpDi")}
                  </Button>
                  <Button
                    variant={createFollowUpWo ? "default" : "outline"}
                    onClick={() => setCreateFollowUpWo((v) => !v)}
                  >
                    {t("actions.createFollowUpWo")}
                  </Button>
                </div>
              </>
            ) : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setExecutionDialogOpen(false)}>
              {t("actions.cancel")}
            </Button>
            <Button onClick={() => void submitExecution()} disabled={saving}>
              {t("actions.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={isPlanDialogOpen} onOpenChange={setPlanDialogOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("dialogs.newPlan.title")}</DialogTitle>
            <DialogDescription>{t("dialogs.newPlan.description")}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t("fields.code")}</Label>
              <Input
                value={planForm.code}
                onChange={(e) => setPlanForm((p) => ({ ...p, code: e.target.value }))}
              />
            </div>
            <div className="space-y-1">
              <Label>{t("fields.title")}</Label>
              <Input
                value={planForm.title}
                onChange={(e) => setPlanForm((p) => ({ ...p, title: e.target.value }))}
              />
            </div>
            <div className="space-y-1">
              <Label>{t("fields.strategy")}</Label>
              <Select
                value={planForm.strategy_type}
                onValueChange={(v) => setPlanForm((p) => ({ ...p, strategy_type: v }))}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {STRATEGY_OPTIONS.map((strategy) => (
                    <SelectItem key={strategy} value={strategy}>
                      {formatEnumLabel(strategy)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("fields.scopeType")}</Label>
              <Select
                value={planForm.asset_scope_type}
                onValueChange={(v) => setPlanForm((p) => ({ ...p, asset_scope_type: v }))}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SCOPE_OPTIONS.map((scope) => (
                    <SelectItem key={scope} value={scope}>
                      {formatEnumLabel(scope)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPlanDialogOpen(false)}>
              {t("actions.cancel")}
            </Button>
            <Button onClick={() => void submitCreatePlan()} disabled={saving || !planFormValid}>
              {t("actions.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={isVersionDialogOpen} onOpenChange={setVersionDialogOpen}>
        <DialogContent className="flex max-h-[90vh] w-[95vw] max-w-4xl flex-col overflow-hidden">
          <DialogHeader>
            <DialogTitle>
              {editingVersion ? t("dialogs.editVersion.title") : t("dialogs.newVersion.title")}
            </DialogTitle>
            <DialogDescription>
              {editingVersion
                ? t("dialogs.editVersion.description")
                : t("dialogs.newVersion.description")}
            </DialogDescription>
          </DialogHeader>
          <div className="min-h-0 flex-1 overflow-y-auto pr-1">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <Label>{t("fields.effectiveFrom")}</Label>
                <Input
                  type="datetime-local"
                  value={toDateTimeLocalInputValue(versionForm.effective_from)}
                  onChange={(e) => {
                    const nextFrom = fromDateTimeLocalInputValue(e.target.value) ?? "";
                    setVersionForm((v) => ({
                      ...v,
                      effective_from: nextFrom,
                      effective_to:
                        v.effective_to && nextFrom && isIsoBefore(v.effective_to, nextFrom)
                          ? nextFrom
                          : (v.effective_to ?? null),
                    }));
                  }}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("fields.effectiveTo")}</Label>
                <Input
                  type="datetime-local"
                  min={toDateTimeLocalInputValue(versionForm.effective_from)}
                  value={toDateTimeLocalInputValue(versionForm.effective_to)}
                  onChange={(e) => {
                    const nextTo = fromDateTimeLocalInputValue(e.target.value);
                    setVersionForm((v) => {
                      if (nextTo && v.effective_from && isIsoBefore(nextTo, v.effective_from)) {
                        return { ...v, effective_to: v.effective_from };
                      }
                      return { ...v, effective_to: nextTo };
                    });
                  }}
                />
              </div>
              {selectedPlan?.strategy_type === "fixed" ||
              selectedPlan?.strategy_type === "floating" ? (
                <>
                  <div className="space-y-1">
                    <Label>{t("fields.intervalUnit")}</Label>
                    <Select
                      value={triggerBuilder.timeUnit}
                      onValueChange={(value) =>
                        setTriggerBuilder((prev) => ({
                          ...prev,
                          timeUnit: value as TimeIntervalUnit,
                        }))
                      }
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="day">{t("options.intervalUnit.day")}</SelectItem>
                        <SelectItem value="week">{t("options.intervalUnit.week")}</SelectItem>
                        <SelectItem value="month">{t("options.intervalUnit.month")}</SelectItem>
                        <SelectItem value="year">{t("options.intervalUnit.year")}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1">
                    <Label>{t("fields.intervalValue")}</Label>
                    <Input
                      type="number"
                      min={1}
                      value={triggerBuilder.timeInterval}
                      onChange={(e) =>
                        setTriggerBuilder((prev) => ({
                          ...prev,
                          timeInterval: Number(e.target.value) > 0 ? Number(e.target.value) : 1,
                        }))
                      }
                    />
                  </div>
                </>
              ) : null}

              {selectedPlan?.strategy_type === "meter" ? (
                <>
                  <div className="space-y-1">
                    <Label>{t("fields.meterSource")}</Label>
                    <Select
                      value={triggerBuilder.meterSource}
                      onValueChange={(value) =>
                        setTriggerBuilder((prev) => ({
                          ...prev,
                          meterSource: value as MeterSource,
                        }))
                      }
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="odometer">
                          {t("options.meterSource.odometer")}
                        </SelectItem>
                        <SelectItem value="operating_hours">
                          {t("options.meterSource.operatingHours")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1">
                    <Label>{t("fields.intervalValue")}</Label>
                    <Input
                      type="number"
                      min={1}
                      value={triggerBuilder.meterInterval}
                      onChange={(e) =>
                        setTriggerBuilder((prev) => ({
                          ...prev,
                          meterInterval: Number(e.target.value) > 0 ? Number(e.target.value) : 1,
                        }))
                      }
                    />
                  </div>
                </>
              ) : null}

              {selectedPlan?.strategy_type === "event" ? (
                <div className="space-y-1 md:col-span-2">
                  <Label>{t("fields.eventCode")}</Label>
                  <Input
                    value={triggerBuilder.eventCode}
                    onChange={(e) =>
                      setTriggerBuilder((prev) => ({ ...prev, eventCode: e.target.value }))
                    }
                  />
                </div>
              ) : null}

              {selectedPlan?.strategy_type === "condition" ? (
                <div className="space-y-1 md:col-span-2">
                  <Label>{t("fields.conditionCode")}</Label>
                  <Input
                    value={triggerBuilder.conditionCode}
                    onChange={(e) =>
                      setTriggerBuilder((prev) => ({ ...prev, conditionCode: e.target.value }))
                    }
                  />
                </div>
              ) : null}

              <div className="space-y-2 rounded-md border p-3 md:col-span-2">
                <div className="flex items-center justify-between gap-2">
                  <Label>{t("fields.taskContent")}</Label>
                  {taskRefsLoading ? (
                    <span className="text-xs text-text-muted">{t("labels.loading")}</span>
                  ) : null}
                </div>
                <div className="grid gap-2 md:grid-cols-[1fr_auto]">
                  <Select value={selectedTaskRefId} onValueChange={applyTaskReferenceToForm}>
                    <SelectTrigger>
                      <SelectValue placeholder={t("fields.taskListReference")} />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none__">{t("labels.none")}</SelectItem>
                      {taskRefItems.map((item) => (
                        <SelectItem key={`${item.setId}-${item.id}`} value={item.id.toString()}>
                          {item.label} ({item.setStatus})
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void loadMaintenanceTaskReferences()}
                    disabled={taskRefsLoading}
                  >
                    {t("actions.refreshTaskLists")}
                  </Button>
                </div>
                <Textarea
                  rows={5}
                  placeholder={t("fields.taskContentPlaceholder")}
                  value={taskStepsText}
                  onChange={(e) => {
                    const next = e.target.value;
                    setTaskStepsText(next);
                    setVersionForm((prev) => ({
                      ...prev,
                      task_package_json: buildTaskPackageFromStepLines(next),
                    }));
                  }}
                />
                <div className="grid gap-2 md:grid-cols-[1fr_auto]">
                  <Input
                    placeholder={t("fields.newTaskListName")}
                    value={newTaskListName}
                    onChange={(e) => setNewTaskListName(e.target.value)}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void saveTaskListToReferences()}
                    disabled={taskRefsSaving}
                  >
                    {t("actions.saveTaskListToReferences")}
                  </Button>
                </div>
              </div>

              <div className="space-y-1 md:col-span-2">
                <div className="space-y-2 rounded-md border p-3">
                  <div className="flex items-center justify-between gap-2">
                    <Label>{t("fields.requiredParts")}</Label>
                    <span className="text-xs text-text-muted">
                      {t("labels.selectedCount", { count: selectedPartCodes.length })}
                    </span>
                  </div>
                  <Input
                    value={partSearch}
                    onChange={(e) => setPartSearch(e.target.value)}
                    placeholder={t("fields.searchPartsPlaceholder")}
                  />
                  <div className="max-h-40 space-y-2 overflow-y-auto rounded-md border p-2">
                    {visiblePartOptions.map((option) => {
                      const checked = selectedPartCodes.includes(option.value);
                      return (
                        <label
                          key={option.value}
                          className="flex cursor-pointer items-start gap-2 text-sm"
                        >
                          <Checkbox
                            checked={checked}
                            onCheckedChange={(isChecked) =>
                              updateRequiredPartCodes(
                                isChecked
                                  ? [...selectedPartCodes, option.value]
                                  : selectedPartCodes.filter((item) => item !== option.value),
                              )
                            }
                          />
                          <span>{option.label}</span>
                        </label>
                      );
                    })}
                    {visiblePartOptions.length === 0 ? (
                      <p className="text-xs text-text-muted">{t("labels.noResults")}</p>
                    ) : null}
                  </div>
                  <div className="flex justify-end">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => updateRequiredPartCodes([])}
                    >
                      {t("actions.clearSelection")}
                    </Button>
                  </div>
                </div>
              </div>

              <div className="space-y-1 md:col-span-2">
                <div className="space-y-2 rounded-md border p-3">
                  <div className="flex items-center justify-between gap-2">
                    <Label>{t("fields.requiredSkills")}</Label>
                    <span className="text-xs text-text-muted">
                      {t("labels.selectedCount", { count: selectedSkillCodes.length })}
                    </span>
                  </div>
                  <Input
                    value={skillSearch}
                    onChange={(e) => setSkillSearch(e.target.value)}
                    placeholder={t("fields.searchSkillsPlaceholder")}
                  />
                  <div className="max-h-40 space-y-2 overflow-y-auto rounded-md border p-2">
                    {visibleSkillOptions.map((option) => {
                      const checked = selectedSkillCodes.includes(option.value);
                      return (
                        <label
                          key={option.value}
                          className="flex cursor-pointer items-start gap-2 text-sm"
                        >
                          <Checkbox
                            checked={checked}
                            onCheckedChange={(isChecked) =>
                              updateRequiredSkillCodes(
                                isChecked
                                  ? [...selectedSkillCodes, option.value]
                                  : selectedSkillCodes.filter((item) => item !== option.value),
                              )
                            }
                          />
                          <span>{option.label}</span>
                        </label>
                      );
                    })}
                    {visibleSkillOptions.length === 0 ? (
                      <p className="text-xs text-text-muted">{t("labels.noResults")}</p>
                    ) : null}
                  </div>
                  <div className="flex justify-end">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => updateRequiredSkillCodes([])}
                    >
                      {t("actions.clearSelection")}
                    </Button>
                  </div>
                </div>
              </div>

              <div className="space-y-1 md:col-span-2">
                <div className="space-y-2 rounded-md border p-3">
                  <div className="flex items-center justify-between gap-2">
                    <Label>{t("fields.requiredTools")}</Label>
                    <span className="text-xs text-text-muted">
                      {t("labels.selectedCount", { count: selectedToolCodes.length })}
                    </span>
                  </div>
                  <Input
                    value={toolSearch}
                    onChange={(e) => setToolSearch(e.target.value)}
                    placeholder={t("fields.searchToolsPlaceholder")}
                  />
                  <div className="max-h-40 space-y-2 overflow-y-auto rounded-md border p-2">
                    {visibleToolOptions.map((option) => {
                      const checked = selectedToolCodes.includes(option.value);
                      return (
                        <label
                          key={option.value}
                          className="flex cursor-pointer items-start gap-2 text-sm"
                        >
                          <Checkbox
                            checked={checked}
                            onCheckedChange={(isChecked) =>
                              updateRequiredToolCodes(
                                isChecked
                                  ? [...selectedToolCodes, option.value]
                                  : selectedToolCodes.filter((item) => item !== option.value),
                              )
                            }
                          />
                          <span>{option.label}</span>
                        </label>
                      );
                    })}
                    {visibleToolOptions.length === 0 ? (
                      <p className="text-xs text-text-muted">{t("labels.noResults")}</p>
                    ) : null}
                  </div>
                  <div className="flex justify-end">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => updateRequiredToolCodes([])}
                    >
                      {t("actions.clearSelection")}
                    </Button>
                  </div>
                </div>
              </div>
              {requirementsLoading ? (
                <p className="text-xs text-text-muted md:col-span-2">{t("labels.loading")}</p>
              ) : null}
              {requirementsError ? (
                <p className="text-xs text-danger md:col-span-2">{requirementsError}</p>
              ) : null}

              <div className="space-y-1">
                <Label>{t("fields.estimatedDurationHours")}</Label>
                <Input
                  type="number"
                  min={0}
                  step={0.1}
                  value={versionForm.estimated_duration_hours ?? ""}
                  onChange={(e) =>
                    setVersionForm((v) => ({
                      ...v,
                      estimated_duration_hours:
                        e.target.value === "" ? null : Number(e.target.value),
                    }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label>{t("fields.estimatedLaborCost")}</Label>
                <Input
                  type="number"
                  min={0}
                  step={0.01}
                  value={versionForm.estimated_labor_cost ?? ""}
                  onChange={(e) =>
                    setVersionForm((v) => ({
                      ...v,
                      estimated_labor_cost: e.target.value === "" ? null : Number(e.target.value),
                    }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label>{t("fields.estimatedPartsCost")}</Label>
                <Input
                  type="number"
                  min={0}
                  step={0.01}
                  value={versionForm.estimated_parts_cost ?? ""}
                  onChange={(e) =>
                    setVersionForm((v) => ({
                      ...v,
                      estimated_parts_cost: e.target.value === "" ? null : Number(e.target.value),
                    }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label>{t("fields.estimatedServiceCost")}</Label>
                <Input
                  type="number"
                  min={0}
                  step={0.01}
                  value={versionForm.estimated_service_cost ?? ""}
                  onChange={(e) =>
                    setVersionForm((v) => ({
                      ...v,
                      estimated_service_cost: e.target.value === "" ? null : Number(e.target.value),
                    }))
                  }
                />
              </div>

              <div className="space-y-1 md:col-span-2">
                <Label>{t("fields.changeReason")}</Label>
                <Input
                  value={versionForm.change_reason ?? ""}
                  onChange={(e) =>
                    setVersionForm((v) => ({ ...v, change_reason: e.target.value || null }))
                  }
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setVersionDialogOpen(false)}>
              {t("actions.cancel")}
            </Button>
            <Button onClick={() => void submitVersion()} disabled={saving}>
              {editingVersion ? t("actions.save") : t("actions.create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={deletePlanDialogOpen} onOpenChange={setDeletePlanDialogOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("dialogs.deletePlan.title")}</DialogTitle>
            <DialogDescription>
              {t("dialogs.deletePlan.description", {
                code: selectedPlan?.code ?? "-",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeletePlanDialogOpen(false)}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void confirmDeletePlan()}
              disabled={saving || !selectedPlan || selectedPlan.lifecycle_status !== "draft"}
            >
              {t("actions.confirmDelete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={deleteVersionDialogOpen}
        onOpenChange={(open) => {
          setDeleteVersionDialogOpen(open);
          if (!open) setDeleteVersionTarget(null);
        }}
      >
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("dialogs.deleteVersion.title")}</DialogTitle>
            <DialogDescription>
              {t("dialogs.deleteVersion.description", {
                version: deleteVersionTarget?.version_no ?? "-",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteVersionDialogOpen(false)}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void confirmDeleteVersion()}
              disabled={saving || !deleteVersionTarget || deleteVersionTarget.status !== "draft"}
            >
              {t("actions.confirmDelete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
