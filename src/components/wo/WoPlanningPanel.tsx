import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useSession } from "@/hooks/use-session";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import {
  checkWoPartStockAvailability,
  createProcurementRequisitionFromWoPart,
  evaluateInventoryUnitCost,
  listInventoryArticles,
  listInventoryLocations,
} from "@/services/inventory-service";
import { listOrgTree } from "@/services/org-node-service";
import { addPart, addTask, addWoTool, listParts, listTasks, listWoTools } from "@/services/wo-execution-service";
import { assignWo, evaluateWoReadiness, planWo } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  InventoryArticle,
  StockLocation,
  WoExecPart,
  WoExecTask,
  WoMaterialReadiness,
  WoReadinessResult,
  WoTool,
  WorkOrder,
  WorkOrderPriorityOption,
  WoShift,
} from "@shared/ipc-types";

export interface WoPlanningPanelHandle {
  refreshReadiness: () => Promise<WoReadinessResult | null>;
}

interface WoPlanningPanelProps {
  wo: WorkOrder;
  canEdit: boolean;
  onReadinessChange?: (report: WoReadinessResult | null) => void;
}

interface TaskDraft {
  localId: string;
  id?: number;
  task_description: string;
  sequence_order: number;
  estimated_minutes: string;
  is_mandatory: boolean;
  persisted: boolean;
}

interface PartDraft {
  localId: string;
  id?: number;
  article_id: number | null;
  article_ref: string;
  stock_location_id: number | null;
  article_query: string;
  quantity_planned: string;
  unit_cost: string;
  reservation_id: number | null;
  persisted: boolean;
}

interface SelectOption {
  value: string;
  label: string;
}

/** Full plan/tasks/parts editable only in planning */
const PLANNING_EDITABLE = new Set(["planning"]);
/** Ready allows reschedule / reassign only */
const READY_SCHEDULE_EDITABLE = new Set(["ready"]);

function planningPriorityLabel(p: WorkOrderPriorityOption, lang: string): string {
  return lang.toLowerCase().startsWith("fr") ? p.label_fr : p.label;
}

const SHIFT_OPTIONS: Array<{ value: WoShift; label: string }> = [
  { value: "morning", label: "morning" },
  { value: "afternoon", label: "afternoon" },
  { value: "night", label: "night" },
  { value: "full_day", label: "full_day" },
];

function toDatetimeLocal(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return "";
  const tzOffset = d.getTimezoneOffset() * 60000;
  return new Date(d.getTime() - tzOffset).toISOString().slice(0, 16);
}

function fromDatetimeLocal(value: string): string {
  if (!value) return "";
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toISOString();
}

function parseNumber(value: string): number | null {
  if (!value.trim()) return null;
  const n = Number(value);
  return Number.isFinite(n) ? n : null;
}

function toTaskDraft(task: WoExecTask): TaskDraft {
  return {
    localId: `task-${task.id}`,
    id: task.id,
    task_description: task.task_description,
    sequence_order: task.sequence_order,
    estimated_minutes: task.estimated_minutes != null ? String(task.estimated_minutes) : "",
    is_mandatory: task.is_mandatory,
    persisted: true,
  };
}

function toPartDraft(part: WoExecPart): PartDraft {
  return {
    localId: `part-${part.id}`,
    id: part.id,
    article_id: part.article_id ?? null,
    article_ref: part.article_ref ?? "",
    stock_location_id: part.stock_location_id ?? null,
    article_query: part.article_ref ?? "",
    quantity_planned: String(part.quantity_planned),
    unit_cost: part.unit_cost != null ? String(part.unit_cost) : "",
    reservation_id: part.reservation_id ?? null,
    persisted: true,
  };
}

function blockingMessagesFromReport(result: WoReadinessResult): string[] {
  return result.checks
    .filter((c) => c.blocking && c.outcome === "fail")
    .map((c) => (c.message?.trim() ? c.message : c.code));
}

export const WoPlanningPanel = forwardRef<WoPlanningPanelHandle, WoPlanningPanelProps>(
  function WoPlanningPanel({ wo, canEdit, onReadinessChange }, ref) {
  const { t, i18n } = useTranslation("ot");
  const prioritiesCatalog = useWorkOrderPrioritiesCatalog((s) => s.priorities);
  const loadPriorities = useWorkOrderPrioritiesCatalog((s) => s.load);
  const activePriorities = useMemo(
    () => prioritiesCatalog.filter((p) => p.is_active).sort((a, b) => a.level - b.level),
    [prioritiesCatalog],
  );
  const { info } = useSession();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const woItems = useWoStore((s) => s.items);

  const [statusCode, setStatusCode] = useState(wo.status_code ?? "draft");
  const [rowVersion, setRowVersion] = useState(wo.row_version);

  const [plannerId, setPlannerId] = useState(wo.planner_id != null ? String(wo.planner_id) : "");
  const [plannedStart, setPlannedStart] = useState(toDatetimeLocal(wo.planned_start));
  const [plannedEnd, setPlannedEnd] = useState(toDatetimeLocal(wo.planned_end));
  const [shift, setShift] = useState<WoShift | "">((wo.shift as WoShift | null) ?? "");
  const [expectedHours, setExpectedHours] = useState(
    wo.expected_duration_hours != null ? String(wo.expected_duration_hours) : "",
  );
  const [plannedDowntimeHours, setPlannedDowntimeHours] = useState(
    wo.planned_downtime_hours != null ? String(wo.planned_downtime_hours) : "",
  );
  const [urgencyId, setUrgencyId] = useState(wo.urgency_id != null ? String(wo.urgency_id) : "");

  const [assignedGroupId, setAssignedGroupId] = useState(
    wo.assigned_group_id != null ? String(wo.assigned_group_id) : "",
  );
  const [primaryResponsibleId, setPrimaryResponsibleId] = useState(
    wo.primary_responsible_id != null ? String(wo.primary_responsible_id) : "",
  );
  const [scheduledAt, setScheduledAt] = useState(toDatetimeLocal(wo.scheduled_at));

  const [tasks, setTasks] = useState<TaskDraft[]>([]);
  const [parts, setParts] = useState<PartDraft[]>([]);
  const [tools, setTools] = useState<WoTool[]>([]);
  const [newToolLabel, setNewToolLabel] = useState("");
  const [newToolCode, setNewToolCode] = useState("");
  const [articleOptions, setArticleOptions] = useState<InventoryArticle[]>([]);
  const [locationOptions, setLocationOptions] = useState<StockLocation[]>([]);
  const [activeArticleField, setActiveArticleField] = useState<string | null>(null);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [groupOptions, setGroupOptions] = useState<SelectOption[]>([]);
  const [responsibleOptions, setResponsibleOptions] = useState<SelectOption[]>([]);
  const lastValuationKeyByPart = useRef<Record<string, string>>({});

  // Readiness evaluation state (planning → ready)
  const [readinessReport, setReadinessReport] = useState<WoReadinessResult | null>(null);
  const [readinessBlocking, setReadinessBlocking] = useState<string[]>([]);
  const [plannerOptions, setPlannerOptions] = useState<SelectOption[]>([]);

  // Material readiness (shortage detection)
  const [materialReadiness, setMaterialReadiness] = useState<WoMaterialReadiness | null>(null);
  const [shortageReqCreating, setShortageReqCreating] = useState<number | null>(null);
  const [shortageSuccessIds, setShortageSuccessIds] = useState<number[]>([]);

  useEffect(() => {
    setStatusCode(wo.status_code ?? "draft");
    setRowVersion(wo.row_version);
    setPlannerId(wo.planner_id != null ? String(wo.planner_id) : "");
    setPlannedStart(toDatetimeLocal(wo.planned_start));
    setPlannedEnd(toDatetimeLocal(wo.planned_end));
    setShift((wo.shift as WoShift | null) ?? "");
    setExpectedHours(wo.expected_duration_hours != null ? String(wo.expected_duration_hours) : "");
    setPlannedDowntimeHours(
      wo.planned_downtime_hours != null ? String(wo.planned_downtime_hours) : "",
    );
    setUrgencyId(wo.urgency_id != null ? String(wo.urgency_id) : "");
    setAssignedGroupId(wo.assigned_group_id != null ? String(wo.assigned_group_id) : "");
    setPrimaryResponsibleId(
      wo.primary_responsible_id != null ? String(wo.primary_responsible_id) : "",
    );
    setScheduledAt(toDatetimeLocal(wo.scheduled_at));
  }, [wo]);

  useEffect(() => {
    if (!plannerId && info?.user_id) {
      setPlannerId(String(info.user_id));
    }
  }, [plannerId, info?.user_id]);

  useEffect(() => {
    void loadPriorities();
  }, [loadPriorities]);

  useEffect(() => {
    let cancelled = false;

    async function loadGroupOptions() {
      try {
        const rows = await listOrgTree();
        if (cancelled) return;

        const opts = rows
          .map((row) => ({
            value: String(row.node.id),
            label: row.node.code ? `${row.node.name} (${row.node.code})` : row.node.name,
          }))
          .sort((a, b) => a.label.localeCompare(b.label));

        setGroupOptions(opts);
      } catch {
        if (!cancelled) setGroupOptions([]);
      }
    }

    void loadGroupOptions();

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const responsibleDedup = new Map<string, string>();
    const plannerDedup = new Map<string, string>();

    for (const item of woItems) {
      if (item.primary_responsible_id != null) {
        const id = String(item.primary_responsible_id);
        const label =
          item.responsible_display_name?.trim() ||
          item.responsible_username?.trim() ||
          formatOrDash(null);
        responsibleDedup.set(id, label);
      }
      if (item.planner_id != null) {
        const id = String(item.planner_id);
        const label =
          item.planner_display_name?.trim() ||
          item.planner_username?.trim() ||
          formatOrDash(null);
        plannerDedup.set(id, label);
      }
    }

    if (info?.user_id != null) {
      const id = String(info.user_id);
      const selfLabel = formatPersonLabel(info.display_name, info.username);
      if (!responsibleDedup.has(id)) responsibleDedup.set(id, selfLabel);
      if (!plannerDedup.has(id)) plannerDedup.set(id, selfLabel);
    }

    if (wo.primary_responsible_id != null) {
      const id = String(wo.primary_responsible_id);
      if (!responsibleDedup.has(id)) {
        responsibleDedup.set(
          id,
          formatPersonLabel(wo.responsible_display_name, wo.responsible_username),
        );
      }
    }

    if (wo.planner_id != null) {
      const id = String(wo.planner_id);
      if (!plannerDedup.has(id)) {
        plannerDedup.set(
          id,
          formatPersonLabel(wo.planner_display_name, wo.planner_username),
        );
      }
    }

    setResponsibleOptions(
      Array.from(responsibleDedup.entries())
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    );
    setPlannerOptions(
      Array.from(plannerDedup.entries())
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    );
  }, [
    woItems,
    wo.primary_responsible_id,
    wo.responsible_username,
    wo.responsible_display_name,
    wo.planner_id,
    wo.planner_username,
    wo.planner_display_name,
    info?.user_id,
    info?.username,
    info?.display_name,
  ]);

  useEffect(() => {
    let cancelled = false;
    async function loadInventoryData() {
      try {
        const [articlesRows, locationRows] = await Promise.all([
          listInventoryArticles({ search: null }),
          listInventoryLocations(null),
        ]);
        if (cancelled) return;
        setArticleOptions(articlesRows.filter((row) => row.is_active === 1));
        setLocationOptions(locationRows.filter((row) => row.is_active === 1));
      } catch {
        if (cancelled) return;
        setArticleOptions([]);
        setLocationOptions([]);
      }
    }
    void loadInventoryData();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function loadPlanningData() {
      try {
        const [taskRows, partRows, toolRows] = await Promise.all([
          listTasks(wo.id).catch(() => []),
          listParts(wo.id).catch(() => []),
          listWoTools(wo.id).catch(() => [] as WoTool[]),
        ]);
        if (cancelled) return;
        setTasks(taskRows.map(toTaskDraft));
        setParts(partRows.map(toPartDraft));
        setTools(toolRows);
      } catch {
        if (!cancelled) {
          setTasks([]);
          setParts([]);
          setTools([]);
        }
      }
    }

    void loadPlanningData();

    return () => {
      cancelled = true;
    };
  }, [wo.id]);

  // Check material availability whenever parts change (planning status only)
  useEffect(() => {
    const hasPersisted = parts.some((p) => p.persisted);
    if (!hasPersisted || statusCode !== "planning") {
      setMaterialReadiness(null);
      return;
    }
    let cancelled = false;
    void checkWoPartStockAvailability(wo.id)
      .then((result) => {
        if (!cancelled) setMaterialReadiness(result);
      })
      .catch(() => {
        if (!cancelled) setMaterialReadiness(null);
      });
    return () => {
      cancelled = true;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wo.id, parts, statusCode]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      for (const part of parts) {
        if (part.persisted || part.article_id == null || part.stock_location_id == null) continue;
        const loc = locationOptions.find((l) => l.id === part.stock_location_id);
        if (!loc) continue;
        const key = `${part.article_id}:${part.stock_location_id}`;
        if (lastValuationKeyByPart.current[part.localId] === key) continue;
        try {
          const r = await evaluateInventoryUnitCost(
            part.article_id,
            loc.warehouse_id,
            part.stock_location_id,
          );
          if (cancelled) return;
          lastValuationKeyByPart.current[part.localId] = key;
          setParts((prev) =>
            prev.map((p) =>
              p.localId === part.localId && !p.persisted
                ? { ...p, unit_cost: String(r.unit_cost) }
                : p,
            ),
          );
        } catch {
          lastValuationKeyByPart.current[part.localId] = key;
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [parts, locationOptions]);

  const isPlanningEditable = PLANNING_EDITABLE.has(statusCode);
  const isReadyScheduleEditable = READY_SCHEDULE_EDITABLE.has(statusCode);
  const scheduleFieldsDisabled = !canEdit || (!isPlanningEditable && !isReadyScheduleEditable) || busy;
  const tasksPartsDisabled = !canEdit || !isPlanningEditable || busy;
  const allFieldsDisabled = scheduleFieldsDisabled;

  const planReady =
    plannerId.trim().length > 0 &&
    plannedStart.trim().length > 0 &&
    plannedEnd.trim().length > 0 &&
    fromDatetimeLocal(plannedEnd) >= fromDatetimeLocal(plannedStart);

  const assignReady = assignedGroupId.trim().length > 0 || primaryResponsibleId.trim().length > 0;

  const plannedCost = useMemo(() => {
    return parts.reduce((sum, part) => {
      const qty = parseNumber(part.quantity_planned) ?? 0;
      const unit = parseNumber(part.unit_cost) ?? 0;
      return sum + qty * unit;
    }, 0);
  }, [parts]);

  const requireActor = info?.user_id ?? null;

  const loadReadiness = useCallback(async () => {
    if (statusCode !== "planning") {
      setReadinessReport(null);
      setReadinessBlocking([]);
      onReadinessChange?.(null);
      return null;
    }
    try {
      const result = await evaluateWoReadiness({ wo_id: wo.id });
      setReadinessReport(result);
      setReadinessBlocking(blockingMessagesFromReport(result));
      onReadinessChange?.(result);
      return result;
    } catch (e) {
      setReadinessReport(null);
      setReadinessBlocking([]);
      onReadinessChange?.(null);
      setError(toErrorMessage(e));
      return null;
    }
  }, [statusCode, wo.id, onReadinessChange]);

  useEffect(() => {
    void loadReadiness();
  }, [loadReadiness]);

  useImperativeHandle(ref, () => ({ refreshReadiness: loadReadiness }), [loadReadiness]);

  /** Save planning data without status change (non-transitioning) */
  const handleSavePlan = useCallback(async () => {
    if (!planReady || !requireActor) return;

    setBusy(true);
    setError(null);
    try {
      const next = await planWo({
        wo_id: wo.id,
        actor_id: requireActor,
        expected_row_version: rowVersion,
        planner_id: Number(plannerId),
        planned_start: fromDatetimeLocal(plannedStart),
        planned_end: fromDatetimeLocal(plannedEnd),
        shift: shift || null,
        expected_duration_hours: parseNumber(expectedHours),
        planned_downtime_hours: parseNumber(plannedDowntimeHours),
        urgency_id: parseNumber(urgencyId),
      });

      setRowVersion(next.row_version);
      setStatusCode(next.status_code ?? statusCode);
      await refreshActiveWo();
      if ((next.status_code ?? statusCode) === "planning") {
        await loadReadiness();
      }
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [
    planReady,
    requireActor,
    wo.id,
    rowVersion,
    plannerId,
    plannedStart,
    plannedEnd,
    shift,
    expectedHours,
    plannedDowntimeHours,
    urgencyId,
    refreshActiveWo,
    statusCode,
    loadReadiness,
  ]);

  // Kept as alias — planWo is a non-status save in Option B.
  const handleMoveToReady = handleSavePlan;

  const handleAssign = useCallback(async () => {
    if (!assignReady || !requireActor) {
      return;
    }

    setBusy(true);
    setError(null);
    try {
      const next = await assignWo({
        wo_id: wo.id,
        actor_id: requireActor,
        expected_row_version: rowVersion,
        assigned_group_id: parseNumber(assignedGroupId),
        primary_responsible_id: parseNumber(primaryResponsibleId),
        scheduled_at: scheduledAt ? fromDatetimeLocal(scheduledAt) : null,
      });

      setRowVersion(next.row_version);
      setStatusCode(next.status_code ?? statusCode);
      await refreshActiveWo();
      if ((next.status_code ?? statusCode) === "planning") {
        await loadReadiness();
      }
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [
    assignReady,
    requireActor,
    wo.id,
    rowVersion,
    assignedGroupId,
    primaryResponsibleId,
    scheduledAt,
    refreshActiveWo,
    statusCode,
    loadReadiness,
  ]);

  const addTaskRow = useCallback(() => {
    setTasks((prev) => [
      ...prev,
      {
        localId: `new-task-${Date.now()}-${prev.length}`,
        task_description: "",
        sequence_order: prev.length + 1,
        estimated_minutes: "",
        is_mandatory: false,
        persisted: false,
      },
    ]);
  }, []);

  const removeTaskRow = useCallback((localId: string) => {
    setTasks((prev) => prev.filter((row) => row.localId !== localId));
  }, []);

  const updateTaskRow = useCallback((localId: string, patch: Partial<TaskDraft>) => {
    setTasks((prev) => prev.map((row) => (row.localId === localId ? { ...row, ...patch } : row)));
  }, []);

  const saveTaskRow = useCallback(
    async (row: TaskDraft) => {
      if (row.persisted || tasksPartsDisabled || !row.task_description.trim()) {
        return;
      }

      setBusy(true);
      setError(null);
      try {
        const created = await addTask({
          wo_id: wo.id,
          task_description: row.task_description.trim(),
          sequence_order: row.sequence_order,
          is_mandatory: row.is_mandatory,
          estimated_minutes: parseNumber(row.estimated_minutes),
        });

        setTasks((prev) =>
          prev.map((task) => (task.localId === row.localId ? toTaskDraft(created) : task)),
        );
      } catch (e) {
        setError(toErrorMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [tasksPartsDisabled, wo.id],
  );

  const addPartRow = useCallback(() => {
    setParts((prev) => [
      ...prev,
      {
        localId: `new-part-${Date.now()}-${prev.length}`,
        article_id: null,
        article_ref: "",
        stock_location_id: null,
        article_query: "",
        quantity_planned: "",
        unit_cost: "",
        reservation_id: null,
        persisted: false,
      },
    ]);
  }, []);

  const removePartRow = useCallback((localId: string) => {
    setParts((prev) => prev.filter((row) => row.localId !== localId));
  }, []);

  const updatePartRow = useCallback((localId: string, patch: Partial<PartDraft>) => {
    setParts((prev) => prev.map((row) => (row.localId === localId ? { ...row, ...patch } : row)));
  }, []);

  const savePartRow = useCallback(
    async (row: PartDraft) => {
      if (row.persisted || tasksPartsDisabled) {
        return;
      }

      const quantityPlanned = parseNumber(row.quantity_planned);
      if (quantityPlanned == null || quantityPlanned <= 0) {
        setError(t("planning.error.addPart"));
        return;
      }
      if (row.article_id == null) {
        setError(t("planning.error.addPart"));
        return;
      }
      if (row.stock_location_id == null) {
        setError(t("planning.error.addPart"));
        return;
      }

      setBusy(true);
      setError(null);
      try {
        const created = await addPart({
          wo_id: wo.id,
          article_id: row.article_id,
          article_ref: row.article_ref.trim() || null,
          quantity_planned: quantityPlanned,
          unit_cost: parseNumber(row.unit_cost),
          stock_location_id: row.stock_location_id,
          auto_reserve: true,
          notes: null,
        });

        setParts((prev) =>
          prev.map((part) => (part.localId === row.localId ? toPartDraft(created) : part)),
        );
      } catch (e) {
        setError(toErrorMessage(e));
      } finally {
        setBusy(false);
      }
    },
    [tasksPartsDisabled, wo.id, t],
  );

  const savePlannedTool = useCallback(async () => {
    if (tasksPartsDisabled) return;
    const label = newToolLabel.trim();
    if (!label) {
      setError(t("execution.tools.labelRequired"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const created = await addWoTool({
        wo_id: wo.id,
        tool_label: label,
        tool_code: newToolCode.trim() || null,
        origin: "planned",
      });
      setTools((prev) => [...prev, created]);
      setNewToolLabel("");
      setNewToolCode("");
    } catch (e) {
      setError(toErrorMessage(e));
    } finally {
      setBusy(false);
    }
  }, [tasksPartsDisabled, newToolLabel, newToolCode, wo.id, t]);

  // ── Draft: submit-only identification summary (footer owns Submit) ─────
  if (statusCode === "draft") {
    return (
      <div className="space-y-4">
        <p className="text-sm text-muted-foreground">{t("planning.draftSubmitHint")}</p>
        <div className="rounded-md border p-3 grid gap-2 text-sm sm:grid-cols-2">
          <div>
            <span className="text-muted-foreground">{t("planning.draftIdentification")}: </span>
            <span className="font-medium">{wo.title || "—"}</span>
          </div>
          <div>
            <span className="text-muted-foreground">{t("detail.fields.type")}: </span>
            <span className="font-medium">{wo.type_label ?? "—"}</span>
          </div>
          <div>
            <span className="text-muted-foreground">{t("detail.fields.equipment")}: </span>
            <span className="font-medium">{wo.asset_label ?? "—"}</span>
          </div>
        </div>
      </div>
    );
  }

  if (statusCode === "cancelled") {
    return (
      <div className="space-y-4">
        <p className="text-sm text-muted-foreground">{t("planning.cancelledStubHint")}</p>
        <div className="rounded-md border p-3 grid gap-2 text-sm sm:grid-cols-2">
          <div>
            <span className="text-muted-foreground">{t("planning.draftIdentification")}: </span>
            <span className="font-medium">{wo.title || "—"}</span>
          </div>
          <div>
            <span className="text-muted-foreground">{t("detail.fields.type")}: </span>
            <span className="font-medium">{wo.type_label ?? "—"}</span>
          </div>
          <div>
            <span className="text-muted-foreground">{t("detail.fields.equipment")}: </span>
            <span className="font-medium">{wo.asset_label ?? "—"}</span>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {error && (
        <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </div>
      )}

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">1. {t("planning.timing")}</h3>
        <div className="grid gap-3 md:grid-cols-3">
          <div className="space-y-1">
            <Label>{t("planning.plannerId")}</Label>
            <Select
              value={plannerId || "__none"}
              onValueChange={(value) => setPlannerId(value === "__none" ? "" : value)}
              disabled={allFieldsDisabled}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("planning.plannerId")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none">—</SelectItem>
                {plannerOptions.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label>{t("planning.plannedStart")}</Label>
            <Input
              type="datetime-local"
              value={plannedStart}
              onChange={(e) => setPlannedStart(e.target.value)}
              disabled={allFieldsDisabled}
            />
          </div>
          <div className="space-y-1">
            <Label>{t("planning.plannedEnd")}</Label>
            <Input
              type="datetime-local"
              value={plannedEnd}
              onChange={(e) => setPlannedEnd(e.target.value)}
              disabled={allFieldsDisabled}
            />
          </div>
          <div className="space-y-1">
            <Label>{t("planning.shift")}</Label>
            <Select
              value={shift}
              onValueChange={(value) => setShift(value as WoShift)}
              disabled={allFieldsDisabled}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("planning.selectShift")} />
              </SelectTrigger>
              <SelectContent>
                {SHIFT_OPTIONS.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {t(`shift.${option.label}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label>{t("planning.expectedDuration")}</Label>
            <Input
              type="number"
              step="0.25"
              min="0"
              value={expectedHours}
              onChange={(e) => setExpectedHours(e.target.value)}
              disabled={allFieldsDisabled}
            />
          </div>
          <div className="space-y-1">
            <Label>{t("planning.plannedDowntimeHours")}</Label>
            <Input
              type="number"
              step="0.25"
              min="0"
              value={plannedDowntimeHours}
              onChange={(e) => setPlannedDowntimeHours(e.target.value)}
              disabled={allFieldsDisabled}
              placeholder={t("planning.plannedDowntimeHoursPlaceholder")}
            />
          </div>
        </div>
      </section>

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">2. {t("planning.urgency")}</h3>
        <Select
          {...(urgencyId ? { value: urgencyId } : {})}
          onValueChange={setUrgencyId}
          disabled={allFieldsDisabled}
        >
          <SelectTrigger className="max-w-sm">
            <SelectValue placeholder={t("planning.selectUrgency")} />
          </SelectTrigger>
          <SelectContent>
            {activePriorities.map((p) => (
              <SelectItem key={p.id} value={String(p.id)}>
                <span className="inline-flex items-center gap-2">
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    style={{ backgroundColor: p.hex_color }}
                  />
                  {planningPriorityLabel(p, i18n.language)}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </section>

      {(isPlanningEditable || isReadyScheduleEditable) && (
        <div className="flex flex-wrap items-center gap-2 border-t pt-3">
          <Button
            variant="outline"
            onClick={() => void handleMoveToReady()}
            disabled={allFieldsDisabled || !planReady || !requireActor}
          >
            {t("planning.planWo")}
          </Button>
          {!requireActor && (
            <span className="text-sm text-muted-foreground">{t("planning.sessionRequired")}</span>
          )}
        </div>
      )}

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">3. {t("planning.assignment")}</h3>
        <div className="grid gap-3 md:grid-cols-3">
          <div className="space-y-1">
            <Label>{t("planning.assignedGroupId")}</Label>
            <Select
              value={assignedGroupId || "__none"}
              onValueChange={(value) => setAssignedGroupId(value === "__none" ? "" : value)}
              disabled={allFieldsDisabled}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("planning.assignedGroupId")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none">—</SelectItem>
                {groupOptions.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label>{t("planning.primaryResponsibleId")}</Label>
            <Select
              value={primaryResponsibleId || "__none"}
              onValueChange={(value) => setPrimaryResponsibleId(value === "__none" ? "" : value)}
              disabled={allFieldsDisabled}
            >
              <SelectTrigger>
                <SelectValue placeholder={t("planning.primaryResponsibleId")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none">—</SelectItem>
                {responsibleOptions.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label>{t("planning.scheduledAt")}</Label>
            <Input
              type="datetime-local"
              value={scheduledAt}
              onChange={(e) => setScheduledAt(e.target.value)}
              disabled={allFieldsDisabled}
            />
          </div>
        </div>
        {groupOptions.length === 0 ? (
          <p className="text-xs text-muted-foreground">{t("planning.noOrgGroups")}</p>
        ) : null}
        {(isPlanningEditable || isReadyScheduleEditable) && (
          <div className="flex flex-wrap items-center gap-2 border-t pt-3">
            <Button
              variant="outline"
              onClick={() => void handleAssign()}
              disabled={allFieldsDisabled || !assignReady || !requireActor}
            >
              {t("planning.assign")}
            </Button>
            {!requireActor && (
              <span className="text-sm text-muted-foreground">{t("planning.sessionRequired")}</span>
            )}
          </div>
        )}
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">4. {t("planning.prerequisites")}</h3>
          {isPlanningEditable && (
            <Button size="sm" variant="outline" onClick={addTaskRow} disabled={tasksPartsDisabled}>
              {t("planning.addTask")}
            </Button>
          )}
        </div>

        <div className="space-y-2">
          {tasks.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("planning.noTasks")}</div>
          )}
          {tasks.map((task) => (
            <div
              key={task.localId}
              className="grid gap-2 rounded-md border p-3 md:grid-cols-[2fr_100px_120px_auto_auto]"
            >
              <Input
                placeholder={t("planning.taskDescription")}
                value={task.task_description}
                onChange={(e) => updateTaskRow(task.localId, { task_description: e.target.value })}
                disabled={tasksPartsDisabled || task.persisted}
              />
              <Input
                type="number"
                min="1"
                value={task.sequence_order}
                onChange={(e) =>
                  updateTaskRow(task.localId, { sequence_order: Number(e.target.value) || 1 })
                }
                disabled={tasksPartsDisabled || task.persisted}
              />
              <Input
                type="number"
                min="0"
                value={task.estimated_minutes}
                onChange={(e) => updateTaskRow(task.localId, { estimated_minutes: e.target.value })}
                disabled={tasksPartsDisabled || task.persisted}
                placeholder={t("planning.minutes")}
              />
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={task.is_mandatory}
                  onChange={(e) => updateTaskRow(task.localId, { is_mandatory: e.target.checked })}
                  disabled={tasksPartsDisabled || task.persisted}
                />
                {t("planning.mandatory")}
              </label>
              <div className="flex items-center gap-2">
                {!task.persisted && isPlanningEditable && (
                  <Button
                    size="sm"
                    onClick={() => void saveTaskRow(task)}
                    disabled={tasksPartsDisabled}
                  >
                    {t("planning.saveTask")}
                  </Button>
                )}
                {isPlanningEditable && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => removeTaskRow(task.localId)}
                    disabled={tasksPartsDisabled}
                  >
                    {t("planning.removeTask")}
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">5. {t("planning.partsPlan")}</h3>
          {isPlanningEditable && (
            <Button size="sm" variant="outline" onClick={addPartRow} disabled={tasksPartsDisabled}>
              {t("planning.addRow")}
            </Button>
          )}
        </div>

        <div className="space-y-2">
          {parts.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("planning.noPlannedParts")}</div>
          )}
          {parts.map((part) => {
            const filteredArticles = articleOptions
              .filter((article) => {
                const term = part.article_query.trim().toLowerCase();
                if (!term) return true;
                return `${article.article_code} ${article.article_name}`
                  .toLowerCase()
                  .includes(term);
              })
              .slice(0, 60);

            return (
              <div
                key={part.localId}
                className="grid gap-2 rounded-md border p-3 md:grid-cols-[2fr_2fr_120px_140px_auto_auto]"
              >
                <div className="relative space-y-1">
                  <Label>{t("planning.partReference")}</Label>
                  <Input
                    placeholder={t("planning.partReference")}
                    value={part.article_query}
                    onFocus={() => setActiveArticleField(part.localId)}
                    onBlur={() => {
                      setTimeout(() => {
                        setActiveArticleField((current) =>
                          current === part.localId ? null : current,
                        );
                      }, 100);
                    }}
                    onChange={(e) =>
                      updatePartRow(part.localId, {
                        article_query: e.target.value,
                        article_id: null,
                        article_ref: "",
                      })
                    }
                    disabled={tasksPartsDisabled || part.persisted}
                  />
                  {activeArticleField === part.localId && !tasksPartsDisabled && !part.persisted && (
                    <div className="absolute z-20 mt-1 max-h-56 w-full overflow-auto rounded-md border bg-popover p-1 shadow-md">
                      {filteredArticles.length === 0 ? (
                        <div className="px-2 py-1.5 text-sm text-muted-foreground">
                          No matching article
                        </div>
                      ) : (
                        filteredArticles.map((article) => (
                          <button
                            key={article.id}
                            type="button"
                            className="w-full rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent hover:text-accent-foreground"
                            onMouseDown={(event) => event.preventDefault()}
                            onClick={() => {
                              const articleLabel = `${article.article_code} - ${article.article_name}`;
                              updatePartRow(part.localId, {
                                article_id: article.id,
                                article_ref: articleLabel,
                                article_query: articleLabel,
                              });
                              setActiveArticleField(null);
                            }}
                          >
                            {article.article_code} - {article.article_name}
                          </button>
                        ))
                      )}
                    </div>
                  )}
                </div>
                <div className="space-y-1">
                  <Label>Stock location</Label>
                  <Select
                    value={
                      part.stock_location_id != null ? String(part.stock_location_id) : "__none"
                    }
                    onValueChange={(value) =>
                      updatePartRow(part.localId, {
                        stock_location_id: value === "__none" ? null : Number(value),
                      })
                    }
                    disabled={tasksPartsDisabled || part.persisted}
                  >
                    <SelectTrigger>
                      <SelectValue placeholder="Location" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__none">--</SelectItem>
                      {locationOptions.map((location) => (
                        <SelectItem key={location.id} value={String(location.id)}>
                          {location.warehouse_code}/{location.code} - {location.name}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1">
                  <Label>Planned qty</Label>
                  <Input
                    type="number"
                    min="0"
                    step="0.01"
                    value={part.quantity_planned}
                    onChange={(e) =>
                      updatePartRow(part.localId, { quantity_planned: e.target.value })
                    }
                    disabled={tasksPartsDisabled || part.persisted}
                  />
                </div>
                <div className="space-y-1">
                  <Label>Unit cost</Label>
                  <Input
                    type="number"
                    min="0"
                    step="0.01"
                    value={part.unit_cost}
                    onChange={(e) => updatePartRow(part.localId, { unit_cost: e.target.value })}
                    disabled={tasksPartsDisabled || part.persisted}
                  />
                  {part.persisted && part.reservation_id != null ? (
                    <p className="text-xs text-muted-foreground">
                      {t("planning.stockReserved")}
                    </p>
                  ) : null}
                  {part.persisted && part.reservation_id == null ? (
                    <p className="text-xs text-amber-800">{t("planning.noStockReservation")}</p>
                  ) : null}
                </div>
                <div className="flex items-end text-sm font-medium">
                  {(
                    (parseNumber(part.quantity_planned) ?? 0) * (parseNumber(part.unit_cost) ?? 0)
                  ).toFixed(2)}
                </div>
                <div className="flex items-end gap-2">
                  {!part.persisted && isPlanningEditable && (
                    <Button
                      size="sm"
                      onClick={() => void savePartRow(part)}
                      disabled={tasksPartsDisabled}
                    >
                      {t("planning.savePart")}
                    </Button>
                  )}
                  {isPlanningEditable && (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => removePartRow(part.localId)}
                      disabled={tasksPartsDisabled}
                    >
                      {t("planning.removePart")}
                    </Button>
                  )}
                </div>
              </div>
            );
          })}
        </div>

        <div className="text-right text-sm font-semibold">
          {t("planning.plannedTotalCost")}: {plannedCost.toFixed(2)}
        </div>
      </section>

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">6. {t("planning.toolsPlan")}</h3>
        {tools.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("planning.noPlannedTools")}</p>
        ) : (
          <ul className="space-y-1 text-sm">
            {tools.map((tool) => (
              <li key={tool.id} className="flex items-center gap-2 border-b border-border/50 py-1.5">
                <span className="font-medium">{tool.tool_label}</span>
                {tool.tool_code ? (
                  <span className="text-muted-foreground">({tool.tool_code})</span>
                ) : null}
                <span className="text-xs text-muted-foreground">
                  {t("execution.badge.planned")}
                </span>
              </li>
            ))}
          </ul>
        )}
        {isPlanningEditable && (
          <div className="flex flex-wrap items-end gap-2">
            <div className="min-w-[160px] flex-1 space-y-1">
              <Label>{t("execution.tools.label")}</Label>
              <Input
                value={newToolLabel}
                onChange={(e) => setNewToolLabel(e.target.value)}
                disabled={tasksPartsDisabled}
                placeholder={t("execution.tools.labelPlaceholder")}
              />
            </div>
            <div className="w-[140px] space-y-1">
              <Label>{t("execution.tools.code")}</Label>
              <Input
                value={newToolCode}
                onChange={(e) => setNewToolCode(e.target.value)}
                disabled={tasksPartsDisabled}
                placeholder={t("execution.tools.codePlaceholder")}
              />
            </div>
            <Button
              size="sm"
              onClick={() => void savePlannedTool()}
              disabled={tasksPartsDisabled || busy}
            >
              {t("planning.addTool")}
            </Button>
          </div>
        )}
      </section>

      {materialReadiness ? (
        <section className="space-y-2 rounded-md border p-3">
          <h3 className="text-sm font-semibold">
            {t("wo.materialReadiness.title", {
              ns: "inventory",
              defaultValue: "Material readiness",
            })}
          </h3>
          <div className="flex flex-wrap gap-3 text-xs text-muted-foreground">
            <span>
              {t("wo.materialReadiness.ready", { ns: "inventory", defaultValue: "Ready" })}:{" "}
              <span className="font-medium text-foreground">
                {materialReadiness.is_fully_available
                  ? t("wo.materialReadiness.yes", { ns: "inventory", defaultValue: "YES" })
                  : t("wo.materialReadiness.no", { ns: "inventory", defaultValue: "NO" })}
              </span>
            </span>
            {materialReadiness.ready_pct != null ? (
              <span>
                {t("wo.materialReadiness.readyPct", { ns: "inventory", defaultValue: "Ready %" })}:{" "}
                <span className="font-medium tabular-nums text-foreground">
                  {Math.round(materialReadiness.ready_pct)}%
                </span>
              </span>
            ) : null}
            {materialReadiness.reserved_pct != null ? (
              <span>
                {t("wo.materialReadiness.reservedPct", {
                  ns: "inventory",
                  defaultValue: "Reserved %",
                })}
                :{" "}
                <span className="font-medium tabular-nums text-foreground">
                  {Math.round(materialReadiness.reserved_pct)}%
                </span>
              </span>
            ) : null}
            {materialReadiness.expected_arrival ? (
              <span>
                {t("wo.materialReadiness.expectedArrival", {
                  ns: "inventory",
                  defaultValue: "Expected arrival",
                })}
                :{" "}
                <span className="font-medium text-foreground">
                  {materialReadiness.expected_arrival}
                </span>
              </span>
            ) : null}
          </div>
        </section>
      ) : null}

      {/* ── Parts shortage → purchase requisition ── */}
      {materialReadiness && !materialReadiness.is_fully_available && materialReadiness.shortages.length > 0 ? (
        <section className="space-y-2 rounded-md border border-amber-300 bg-amber-50 p-3">
          <h3 className="text-sm font-semibold text-amber-800">
            {t("wo.shortage.title", { ns: "inventory" })}
          </h3>
          <p className="text-xs text-amber-700">
            {t("wo.shortage.description", { ns: "inventory" })}
          </p>
          <div className="space-y-1.5">
            {materialReadiness.shortages.map((shortage) => {
              const wasCreated = shortageSuccessIds.includes(shortage.article_id);
              return (
                <div
                  key={shortage.article_id}
                  className="flex flex-wrap items-center gap-2 rounded border border-amber-200 bg-white p-2 text-xs"
                >
                  <span className="font-medium">{shortage.article_code}</span>
                  <span className="text-muted-foreground">— {shortage.article_name}</span>
                  <span className="ml-auto tabular-nums text-amber-700">
                    {t("wo.shortage.columns.requested", { ns: "inventory" })}: {shortage.requested_qty} ·{" "}
                    {t("wo.shortage.columns.available", { ns: "inventory" })}: {shortage.available_qty} ·{" "}
                    <span className="font-semibold text-red-700">
                      {t("wo.shortage.columns.shortage", { ns: "inventory" })}: {shortage.shortage_qty}
                    </span>
                  </span>
                  {wasCreated ? (
                    <span className="text-xs font-medium text-green-700">
                      ✓ {t("wo.shortage.requisitionCreated", { ns: "inventory" })}
                    </span>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-6 px-2 text-xs"
                      disabled={shortageReqCreating === shortage.article_id}
                      onClick={() => {
                        setShortageReqCreating(shortage.article_id);
                        void createProcurementRequisitionFromWoPart(
                          wo.id,
                          shortage.article_id,
                          shortage.shortage_qty,
                          shortage.preferred_location_id,
                        )
                          .then(() => {
                            setShortageSuccessIds((prev) => [...prev, shortage.article_id]);
                          })
                          .catch((err) => {
                            setError(toErrorMessage(err));
                          })
                          .finally(() => {
                            setShortageReqCreating(null);
                          });
                      }}
                    >
                      {shortageReqCreating === shortage.article_id
                        ? "…"
                        : t("wo.shortage.createRequisition", { ns: "inventory" })}
                    </Button>
                  )}
                </div>
              );
            })}
          </div>
        </section>
      ) : null}

      {statusCode === "planning" && readinessReport && (
        <section className="space-y-2 rounded-md border p-3">
          <h3 className="text-sm font-semibold">{t("planning.readinessChecklist")}</h3>
          <ul className="space-y-1 text-sm">
            {readinessReport.checks.map((check) => (
              <li key={check.code} className="flex items-start gap-2">
                <span
                  className={
                    check.outcome === "pass"
                      ? "text-green-700"
                      : check.outcome === "na"
                        ? "text-muted-foreground"
                        : check.blocking
                          ? "text-amber-800"
                          : "text-muted-foreground"
                  }
                >
                  {check.outcome === "pass" ? "✓" : check.outcome === "na" ? "—" : "✗"}
                </span>
                <span>
                  {check.message?.trim() || check.code}
                  {!check.blocking && check.outcome === "fail" ? (
                    <span className="text-muted-foreground"> ({t("planning.readinessRecommended")})</span>
                  ) : null}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {readinessBlocking.length > 0 && statusCode === "planning" && (
        <div className="w-full rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800">
          <p className="mb-1 font-medium">{t("planning.readinessBlocking")}</p>
          <ul className="list-disc list-inside space-y-0.5">
            {readinessBlocking.map((msg, i) => (
              <li key={i}>{msg}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
});

