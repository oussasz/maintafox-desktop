import { useCallback, useEffect, useMemo, useState } from "react";
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
import { listOrgTree } from "@/services/org-node-service";
import {
  addPart,
  addTask,
  assignWo,
  listParts,
  listTasks,
  planWo,
  type WoPart,
  type WoTask,
} from "@/services/wo-execution-service";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder, WoShift } from "@shared/ipc-types";

interface WoPlanningPanelProps {
  wo: WorkOrder;
  canEdit: boolean;
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
  article_ref: string;
  quantity_planned: string;
  unit_cost: string;
  persisted: boolean;
}

interface SelectOption {
  value: string;
  label: string;
}

const PLAN_EDITABLE_STATUSES = new Set([
  "draft",
  "awaiting_approval",
  "planned",
  "ready_to_schedule",
]);

const URGENCY_LEVELS = [
  { id: 1, i18nKey: "form.urgency.veryLow", swatch: "bg-emerald-500" },
  { id: 2, i18nKey: "form.urgency.low", swatch: "bg-lime-500" },
  { id: 3, i18nKey: "form.urgency.medium", swatch: "bg-amber-500" },
  { id: 4, i18nKey: "form.urgency.high", swatch: "bg-orange-500" },
  { id: 5, i18nKey: "form.urgency.critical", swatch: "bg-red-500" },
] as const;

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

function toTaskDraft(task: WoTask): TaskDraft {
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

function toPartDraft(part: WoPart): PartDraft {
  return {
    localId: `part-${part.id}`,
    id: part.id,
    article_ref: part.article_ref ?? "",
    quantity_planned: String(part.quantity_planned),
    unit_cost: part.unit_cost != null ? String(part.unit_cost) : "",
    persisted: true,
  };
}

export function WoPlanningPanel({ wo, canEdit }: WoPlanningPanelProps) {
  const { t } = useTranslation("ot");
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

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [groupOptions, setGroupOptions] = useState<SelectOption[]>([]);
  const [responsibleOptions, setResponsibleOptions] = useState<SelectOption[]>([]);

  useEffect(() => {
    setStatusCode(wo.status_code ?? "draft");
    setRowVersion(wo.row_version);
    setPlannerId(wo.planner_id != null ? String(wo.planner_id) : "");
    setPlannedStart(toDatetimeLocal(wo.planned_start));
    setPlannedEnd(toDatetimeLocal(wo.planned_end));
    setShift((wo.shift as WoShift | null) ?? "");
    setExpectedHours(wo.expected_duration_hours != null ? String(wo.expected_duration_hours) : "");
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
    const dedup = new Map<string, string>();

    for (const item of woItems) {
      if (item.primary_responsible_id != null) {
        const id = String(item.primary_responsible_id);
        const label = item.responsible_username?.trim() || `User #${id}`;
        dedup.set(id, label);
      }
    }

    if (info?.user_id != null) {
      const id = String(info.user_id);
      if (!dedup.has(id)) {
        dedup.set(id, info.username ?? `User #${id}`);
      }
    }

    if (wo.primary_responsible_id != null) {
      const id = String(wo.primary_responsible_id);
      if (!dedup.has(id)) {
        dedup.set(id, wo.responsible_username ?? `User #${id}`);
      }
    }

    const opts = Array.from(dedup.entries())
      .map(([value, label]) => ({ value, label }))
      .sort((a, b) => a.label.localeCompare(b.label));

    setResponsibleOptions(opts);
  }, [woItems, wo.primary_responsible_id, wo.responsible_username, info?.user_id, info?.username]);

  useEffect(() => {
    let cancelled = false;

    async function loadPlanningData() {
      try {
        const [taskRows, partRows] = await Promise.all([
          listTasks(wo.id).catch(() => []),
          listParts(wo.id).catch(() => []),
        ]);
        if (cancelled) return;
        setTasks(taskRows.map(toTaskDraft));
        setParts(partRows.map(toPartDraft));
      } catch {
        if (!cancelled) {
          setTasks([]);
          setParts([]);
        }
      }
    }

    void loadPlanningData();

    return () => {
      cancelled = true;
    };
  }, [wo.id]);

  const isPastPlannedStatus = !PLAN_EDITABLE_STATUSES.has(statusCode);
  const allFieldsDisabled = !canEdit || isPastPlannedStatus || busy;

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

  const handleMoveToReady = useCallback(async () => {
    if (!planReady || !requireActor) {
      return;
    }

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
        urgency_id: parseNumber(urgencyId),
      });

      setRowVersion(next.row_version);
      setStatusCode(next.status_code ?? statusCode);
      await refreshActiveWo();
    } catch (e) {
      setError(e instanceof Error ? e.message : t("planning.error.plan"));
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
    urgencyId,
    refreshActiveWo,
    statusCode,
    t,
  ]);

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
    } catch (e) {
      setError(e instanceof Error ? e.message : t("planning.error.assign"));
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
    t,
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
      if (row.persisted || allFieldsDisabled || !row.task_description.trim()) {
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
        setError(e instanceof Error ? e.message : t("planning.error.addTask"));
      } finally {
        setBusy(false);
      }
    },
    [allFieldsDisabled, wo.id, t],
  );

  const addPartRow = useCallback(() => {
    setParts((prev) => [
      ...prev,
      {
        localId: `new-part-${Date.now()}-${prev.length}`,
        article_ref: "",
        quantity_planned: "0",
        unit_cost: "0",
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
      if (row.persisted || allFieldsDisabled) {
        return;
      }

      const quantityPlanned = parseNumber(row.quantity_planned);
      if (quantityPlanned == null) {
        return;
      }

      setBusy(true);
      setError(null);
      try {
        const created = await addPart({
          wo_id: wo.id,
          article_ref: row.article_ref.trim() || null,
          quantity_planned: quantityPlanned,
          unit_cost: parseNumber(row.unit_cost),
          notes: null,
        });

        setParts((prev) =>
          prev.map((part) => (part.localId === row.localId ? toPartDraft(created) : part)),
        );
      } catch (e) {
        setError(e instanceof Error ? e.message : t("planning.error.addPart"));
      } finally {
        setBusy(false);
      }
    },
    [allFieldsDisabled, wo.id, t],
  );

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
            <Input
              type="number"
              value={plannerId}
              onChange={(e) => setPlannerId(e.target.value)}
              disabled={allFieldsDisabled}
            />
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
        </div>
      </section>

      <section className="space-y-3">
        <h3 className="text-sm font-semibold">2. {t("planning.urgency")}</h3>
        <Select value={urgencyId} onValueChange={setUrgencyId} disabled={allFieldsDisabled}>
          <SelectTrigger className="max-w-sm">
            <SelectValue placeholder={t("planning.selectUrgency")} />
          </SelectTrigger>
          <SelectContent>
            {URGENCY_LEVELS.map((level) => (
              <SelectItem key={level.id} value={String(level.id)}>
                <span className="inline-flex items-center gap-2">
                  <span className={`h-2.5 w-2.5 rounded-full ${level.swatch}`} />
                  {t(level.i18nKey)}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </section>

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
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">4. {t("planning.prerequisites")}</h3>
          <Button size="sm" variant="outline" onClick={addTaskRow} disabled={allFieldsDisabled}>
            {t("planning.addTask")}
          </Button>
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
                disabled={allFieldsDisabled || task.persisted}
              />
              <Input
                type="number"
                min="1"
                value={task.sequence_order}
                onChange={(e) =>
                  updateTaskRow(task.localId, { sequence_order: Number(e.target.value) || 1 })
                }
                disabled={allFieldsDisabled || task.persisted}
              />
              <Input
                type="number"
                min="0"
                value={task.estimated_minutes}
                onChange={(e) => updateTaskRow(task.localId, { estimated_minutes: e.target.value })}
                disabled={allFieldsDisabled || task.persisted}
                placeholder={t("planning.minutes")}
              />
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={task.is_mandatory}
                  onChange={(e) => updateTaskRow(task.localId, { is_mandatory: e.target.checked })}
                  disabled={allFieldsDisabled || task.persisted}
                />
                {t("planning.mandatory")}
              </label>
              <div className="flex items-center gap-2">
                {!task.persisted && (
                  <Button
                    size="sm"
                    onClick={() => void saveTaskRow(task)}
                    disabled={allFieldsDisabled}
                  >
                    {t("planning.saveTask")}
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => removeTaskRow(task.localId)}
                  disabled={allFieldsDisabled}
                >
                  {t("planning.removeTask")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">5. {t("planning.partsPlan")}</h3>
          <Button size="sm" variant="outline" onClick={addPartRow} disabled={allFieldsDisabled}>
            {t("planning.addRow")}
          </Button>
        </div>

        <div className="space-y-2">
          {parts.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("planning.noPlannedParts")}</div>
          )}
          {parts.map((part) => (
            <div
              key={part.localId}
              className="grid gap-2 rounded-md border p-3 md:grid-cols-[2fr_120px_140px_auto_auto]"
            >
              <Input
                placeholder={t("planning.partReference")}
                value={part.article_ref}
                onChange={(e) => updatePartRow(part.localId, { article_ref: e.target.value })}
                disabled={allFieldsDisabled || part.persisted}
              />
              <Input
                type="number"
                min="0"
                step="0.01"
                value={part.quantity_planned}
                onChange={(e) => updatePartRow(part.localId, { quantity_planned: e.target.value })}
                disabled={allFieldsDisabled || part.persisted}
              />
              <Input
                type="number"
                min="0"
                step="0.01"
                value={part.unit_cost}
                onChange={(e) => updatePartRow(part.localId, { unit_cost: e.target.value })}
                disabled={allFieldsDisabled || part.persisted}
              />
              <div className="flex items-center text-sm font-medium">
                {(
                  (parseNumber(part.quantity_planned) ?? 0) * (parseNumber(part.unit_cost) ?? 0)
                ).toFixed(2)}
              </div>
              <div className="flex items-center gap-2">
                {!part.persisted && (
                  <Button
                    size="sm"
                    onClick={() => void savePartRow(part)}
                    disabled={allFieldsDisabled}
                  >
                    {t("planning.savePart")}
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => removePartRow(part.localId)}
                  disabled={allFieldsDisabled}
                >
                  {t("planning.removePart")}
                </Button>
              </div>
            </div>
          ))}
        </div>

        <div className="text-right text-sm font-semibold">
          {t("planning.plannedTotalCost")}: {plannedCost.toFixed(2)}
        </div>
      </section>

      <div className="flex flex-wrap items-center gap-2 border-t pt-3">
        <Button
          onClick={() => void handleMoveToReady()}
          disabled={allFieldsDisabled || !planReady || !requireActor}
        >
          {t("planning.planWo")}
        </Button>
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
    </div>
  );
}
