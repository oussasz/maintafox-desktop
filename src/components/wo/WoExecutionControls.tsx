/**
 * WO Execution tab — top-to-bottom technician workflow (UI only).
 * Lifecycle Start/Hold/Resume/Complete stay in the detail dialog footer.
 *
 * Phase 1+2 Plan vs Actual:
 * - Parts: Planned pane (Use / Not used…) + Actually used pane with origin badges
 * - Tasks: planned vs execution-added; cancel with result_code="cancelled"
 * - Labor: Planned / Actual / Variance / Efficiency %
 * - Downtime: Planned / Actual / Cause
 * - Tools: planned vs used with badges
 * - Execution Log timeline
 */

import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
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
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import {
  elapsedMinutesBetween,
  formatAssetLabel,
  formatDurationMinutes,
  formatHoursWorked,
  formatOrDash,
  formatPersonLabel,
  hoursToMinutes,
} from "@/lib/display";
import { listInventoryReservations, releaseInventoryReservation } from "@/services/inventory-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import {
  addLabor,
  addPart,
  addWoTool,
  closeDowntime,
  completeTask,
  confirmNoParts,
  getPlanAdherence,
  listDowntimeSegments,
  listExecutionEvents,
  listLabor,
  listParts,
  listTasks,
  listWoTools,
  markPartNotUsed,
  markWoToolNotUsed,
  markWoToolUsed,
  openDowntime,
  recordPartUsage,
  unconfirmNoParts,
  type TaskResultCode,
  type WoDowntimeSegment,
  type WoExecutionEvent,
  type WoIntervener,
  type WoPlanAdherence,
  type WoTool,
} from "@/services/wo-execution-service";
import { holdWo, pauseWo, resumeWo, startWo } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { formatDateTime } from "@/utils/format-date";
import type { DowntimeType, StockReservation, WoExecPart, WoExecTask, WorkOrder } from "@shared/ipc-types";

export interface WoExecutionControlsHandle {
  openHoldForm: () => void;
  start: () => Promise<void>;
  resume: () => Promise<void>;
}

interface WoExecutionControlsProps {
  wo: WorkOrder;
  canEdit: boolean;
}

type DelayIntent = "pause" | "hold";

const LOGGING_EDITABLE_STATUSES = new Set(["in_progress"]);
const TASK_RESULT_OPTIONS: TaskResultCode[] = ["ok", "nok", "na", "deferred", "cancelled"];

function nowIso(): string {
  return new Date().toISOString();
}

function todayLocalDate(): string {
  const d = new Date();
  const tz = d.getTimezoneOffset() * 60_000;
  return new Date(d.getTime() - tz).toISOString().slice(0, 10);
}

function combineLocalDateTime(date: string, time: string): string | null {
  if (!date.trim() || !time.trim()) return null;
  const d = new Date(`${date}T${time}`);
  if (Number.isNaN(d.getTime())) return null;
  return d.toISOString();
}

/** Compact "Planned / Actual / Variance / Efficiency" row. */
function LaborMetricRow({
  labelPlanned,
  labelActual,
  labelVariance,
  labelEfficiency,
  planned,
  actual,
  durationLabels,
}: {
  labelPlanned: string;
  labelActual: string;
  labelVariance: string;
  labelEfficiency: string;
  planned: number | null;
  actual: number;
  durationLabels: { hours: string; minutes: string };
}) {
  const variance =
    planned != null && actual > 0 ? actual - hoursToMinutes(planned) : null;
  const efficiency =
    planned != null && actual > 0
      ? Math.round((hoursToMinutes(planned) / actual) * 100)
      : null;

  return (
    <div className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-4">
      <div className="space-y-0.5">
        <p className="text-xs text-muted-foreground">{labelPlanned}</p>
        <p className="font-medium tabular-nums">
          {planned != null
            ? formatDurationMinutes(hoursToMinutes(planned), "auto", durationLabels)
            : "—"}
        </p>
      </div>
      <div className="space-y-0.5">
        <p className="text-xs text-muted-foreground">{labelActual}</p>
        <p className="font-medium tabular-nums">
          {formatDurationMinutes(actual, "auto", durationLabels)}
        </p>
      </div>
      <div className="space-y-0.5">
        <p className="text-xs text-muted-foreground">{labelVariance}</p>
        <p
          className={
            variance == null
              ? "font-medium tabular-nums"
              : variance > 0
                ? "font-medium tabular-nums text-amber-700"
                : "font-medium tabular-nums text-green-700"
          }
        >
          {variance == null
            ? "—"
            : `${variance > 0 ? "+" : ""}${formatDurationMinutes(Math.abs(variance), "auto", durationLabels)}`}
        </p>
      </div>
      <div className="space-y-0.5">
        <p className="text-xs text-muted-foreground">{labelEfficiency}</p>
        <p
          className={
            efficiency == null
              ? "font-medium tabular-nums"
              : efficiency >= 100
                ? "font-medium tabular-nums text-green-700"
                : "font-medium tabular-nums text-amber-700"
          }
        >
          {efficiency != null ? `${efficiency} %` : "—"}
        </p>
      </div>
    </div>
  );
}

/** Small badge showing part/task origin. */
function OriginBadge({ origin, t }: { origin?: string | null | undefined; t: (k: string) => string }) {
  const isAdded = origin === "execution_added";
  const isGenerated = origin === "generated";
  return (
    <Badge variant="secondary" className="ml-1 text-[10px]">
      {isAdded
        ? t("execution.badge.executionAdded")
        : isGenerated
          ? t("execution.badge.generated")
          : t("execution.badge.planned")}
    </Badge>
  );
}

function executionLogI18nKey(summaryKey: string): string {
  if (summaryKey.startsWith("executionLog.")) {
    return `execution.${summaryKey}`;
  }
  if (summaryKey.startsWith("execution.")) {
    return summaryKey;
  }
  return `execution.executionLog.${summaryKey}`;
}

export const WoExecutionControls = forwardRef<WoExecutionControlsHandle, WoExecutionControlsProps>(
  function WoExecutionControls({ wo, canEdit }, ref) {
    const { t, i18n } = useTranslation("ot");
    const { info } = useSession();
    const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
    const woItems = useWoStore((s) => s.items);

    const durationLabels = useMemo(
      () => ({
        hours: t("execution.durationHours"),
        minutes: t("execution.durationMinutes"),
      }),
      [t],
    );

    const [statusCode, setStatusCode] = useState(wo.status_code ?? "draft");
    const [rowVersion, setRowVersion] = useState(wo.row_version);

    const [laborEntries, setLaborEntries] = useState<WoIntervener[]>([]);
    const [parts, setParts] = useState<WoExecPart[]>([]);
    const [tasks, setTasks] = useState<WoExecTask[]>([]);
    const [downtimeSegments, setDowntimeSegments] = useState<WoDowntimeSegment[]>([]);
    const [reservations, setReservations] = useState<StockReservation[]>([]);
    const [tools, setTools] = useState<WoTool[]>([]);
    const [executionEvents, setExecutionEvents] = useState<WoExecutionEvent[]>([]);
    const [planAdherence, setPlanAdherence] = useState<WoPlanAdherence | null>(null);

    const [partUsage, setPartUsage] = useState<Record<number, string>>({});
    const [taskResultCodes, setTaskResultCodes] = useState<Record<number, TaskResultCode>>({});

    // Delay intent
    const [delayIntent, setDelayIntent] = useState<DelayIntent | null>(null);
    const [delayReasonId, setDelayReasonId] = useState<string | null>(null);
    const [delayComment, setDelayComment] = useState("");
    const [delayError, setDelayError] = useState<string | null>(null);

    // Labor dialog
    const [showLaborDialog, setShowLaborDialog] = useState(false);
    const [laborTechId, setLaborTechId] = useState("");
    const [laborDate, setLaborDate] = useState(todayLocalDate());
    const [laborStart, setLaborStart] = useState("08:00");
    const [laborEnd, setLaborEnd] = useState("10:00");
    const [laborBreakMin, setLaborBreakMin] = useState("");
    const [laborNotes, setLaborNotes] = useState("");
    const [laborFormError, setLaborFormError] = useState<string | null>(null);

    // Not-used dialog
    const [notUsedPartId, setNotUsedPartId] = useState<number | null>(null);
    const [notUsedReasonId, setNotUsedReasonId] = useState<string | null>(null);
    const [notUsedComment, setNotUsedComment] = useState("");
    const [notUsedError, setNotUsedError] = useState<string | null>(null);

    // Add part dialog (during execution)
    const [showAddPartDialog, setShowAddPartDialog] = useState(false);
    const [addPartRef, setAddPartRef] = useState("");
    const [addPartQty, setAddPartQty] = useState("1");
    const [addPartDialogError, setAddPartDialogError] = useState<string | null>(null);

    // Add tool dialog
    const [showAddToolDialog, setShowAddToolDialog] = useState(false);
    const [addToolLabel, setAddToolLabel] = useState("");
    const [addToolCode, setAddToolCode] = useState("");
    const [addToolDialogError, setAddToolDialogError] = useState<string | null>(null);

    // Add task during execution
    const [showAddTaskDialog, setShowAddTaskDialog] = useState(false);
    const [addTaskDesc, setAddTaskDesc] = useState("");
    const [addTaskMandatory, setAddTaskMandatory] = useState(false);
    const [addTaskDialogError, setAddTaskDialogError] = useState<string | null>(null);

    // Downtime
    const [downtimeType, setDowntimeType] = useState<DowntimeType>("full");
    const [downtimeComment, setDowntimeComment] = useState("");
    const [downtimeClassification, setDowntimeClassification] = useState<string>("");
    const [unusedReasonCodesById, setUnusedReasonCodesById] = useState<Record<string, string>>(
      {},
    );

    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // ── Intervener options ──────────────────────────────────────────────────

    const intervenerOptions = useMemo(() => {
      const dedup = new Map<string, string>();
      for (const item of woItems) {
        if (item.primary_responsible_id != null) {
          dedup.set(
            String(item.primary_responsible_id),
            item.responsible_display_name?.trim() ||
              item.responsible_username?.trim() ||
              formatOrDash(null),
          );
        }
        if (item.planner_id != null) {
          dedup.set(
            String(item.planner_id),
            item.planner_display_name?.trim() || item.planner_username?.trim() || formatOrDash(null),
          );
        }
      }
      for (const entry of laborEntries) {
        if (entry.intervener_id != null) {
          dedup.set(
            String(entry.intervener_id),
            formatPersonLabel(entry.intervener_display_name) !== "—"
              ? formatPersonLabel(entry.intervener_display_name)
              : String(entry.intervener_id),
          );
        }
      }
      return Array.from(dedup.entries()).map(([value, label]) => ({ value, label }));
    }, [woItems, laborEntries]);

    // ── Load data ──────────────────────────────────────────────────────────

    const loadData = useCallback(async () => {
      try {
        const [laborRows, partRows, taskRows, downtimeRows, reservationRows, toolRows, eventRows, adherenceRow] =
          await Promise.all([
            listLabor(wo.id),
            listParts(wo.id),
            listTasks(wo.id),
            listDowntimeSegments(wo.id),
            listInventoryReservations({
              source_type: "work_order",
              source_id: wo.id,
            }).catch(() => [] as StockReservation[]),
            listWoTools(wo.id).catch(() => [] as WoTool[]),
            listExecutionEvents(wo.id).catch(() => [] as WoExecutionEvent[]),
            getPlanAdherence(wo.id).catch(() => null),
          ]);
        setLaborEntries(laborRows);
        setParts(partRows);
        setTasks(taskRows);
        setDowntimeSegments(downtimeRows);
        setReservations(reservationRows);
        setTools(toolRows);
        setExecutionEvents(eventRows);
        setPlanAdherence(adherenceRow);
      } catch (e) {
        setError(t("execution.error.loadData"));
        console.error("loadData", e);
      }
    }, [wo.id, t]);

    useEffect(() => {
      void loadData();
    }, [loadData]);

    useEffect(() => {
      setStatusCode(wo.status_code ?? "draft");
      setRowVersion(wo.row_version);
    }, [wo.status_code, wo.row_version]);

    const actorId = info?.user_id ?? null;
    const loggingEnabled = LOGGING_EDITABLE_STATUSES.has(statusCode);
    const controlsDisabled = !canEdit || !loggingEnabled || busy;

    const handleRefreshState = useCallback(
      async (next: WorkOrder) => {
        setRowVersion(next.row_version);
        setStatusCode(next.status_code ?? statusCode);
        await refreshActiveWo();
        await loadData();
      },
      [refreshActiveWo, loadData, statusCode],
    );

    // ── Lifecycle actions ──────────────────────────────────────────────────

    const handleStart = useCallback(async () => {
      if (!actorId) return;
      setBusy(true);
      setError(null);
      try {
        const next = await startWo({
          wo_id: wo.id,
          actor_id: actorId,
          expected_row_version: rowVersion,
        });
        await handleRefreshState(next);
      } catch (e) {
        setError(t("execution.error.startWo"));
        console.error("handleStart", e);
      } finally {
        setBusy(false);
      }
    }, [actorId, wo.id, rowVersion, handleRefreshState, t]);

    const handleResume = useCallback(async () => {
      if (!actorId) return;
      setBusy(true);
      setError(null);
      try {
        const next = await resumeWo({
          wo_id: wo.id,
          actor_id: actorId,
          expected_row_version: rowVersion,
        });
        await handleRefreshState(next);
      } catch (e) {
        setError(t("execution.error.resumeWo"));
        console.error("handleResume", e);
      } finally {
        setBusy(false);
      }
    }, [actorId, wo.id, rowVersion, handleRefreshState, t]);

    const openDelayForm = useCallback((intent: DelayIntent) => {
      setDelayIntent(intent);
      setDelayReasonId(null);
      setDelayComment("");
      setDelayError(null);
    }, []);

    useImperativeHandle(
      ref,
      () => ({
        openHoldForm: () => openDelayForm("hold"),
        start: handleStart,
        resume: handleResume,
      }),
      [openDelayForm, handleStart, handleResume],
    );

    const submitDelayAction = useCallback(async () => {
      if (!actorId || !delayIntent) return;
      const reasonId = delayReasonId ? Number(delayReasonId) : NaN;
      if (!Number.isFinite(reasonId) || reasonId <= 0) {
        setDelayError(t("execution.delayReasonRequired"));
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const input = {
          wo_id: wo.id,
          actor_id: actorId,
          expected_row_version: rowVersion,
          delay_reason_id: reasonId,
          comment: delayComment.trim() || null,
        };
        const next = delayIntent === "pause" ? await pauseWo(input) : await holdWo(input);
        setDelayIntent(null);
        await handleRefreshState(next);
      } catch (e) {
        setError(t("execution.error.submitDelay"));
        console.error("submitDelayAction", e);
      } finally {
        setBusy(false);
      }
    }, [actorId, delayIntent, delayReasonId, delayComment, wo.id, rowVersion, handleRefreshState, t]);

    // ── Labor dialog ───────────────────────────────────────────────────────

    const laborPreviewMinutes = useMemo(() => {
      const started = combineLocalDateTime(laborDate, laborStart);
      const ended = combineLocalDateTime(laborDate, laborEnd);
      const elapsed = elapsedMinutesBetween(started, ended);
      if (elapsed == null) return null;
      const brk = Number(laborBreakMin);
      const breakMin = Number.isFinite(brk) && brk > 0 ? Math.round(brk) : 0;
      return Math.max(0, elapsed - breakMin);
    }, [laborDate, laborStart, laborEnd, laborBreakMin]);

    const openLaborDialog = useCallback(() => {
      setLaborTechId(intervenerOptions[0]?.value ?? "");
      setLaborDate(todayLocalDate());
      setLaborStart("08:00");
      setLaborEnd("10:00");
      setLaborBreakMin("");
      setLaborNotes("");
      setLaborFormError(null);
      setShowLaborDialog(true);
    }, [intervenerOptions]);

    const submitLaborDialog = useCallback(async () => {
      if (!actorId) return;
      const intervenerId = Number(laborTechId);
      const started = combineLocalDateTime(laborDate, laborStart);
      const ended = combineLocalDateTime(laborDate, laborEnd);
      if (!Number.isFinite(intervenerId) || intervenerId <= 0) {
        setLaborFormError(t("execution.error.intervenerId"));
        return;
      }
      if (!started || !ended) {
        setLaborFormError(t("execution.laborDialog.invalidTimes"));
        return;
      }
      const elapsed = elapsedMinutesBetween(started, ended);
      if (elapsed == null || elapsed <= 0) {
        setLaborFormError(t("execution.laborDialog.endAfterStart"));
        return;
      }
      const brk = Number(laborBreakMin);
      const breakMin = Number.isFinite(brk) && brk > 0 ? Math.round(brk) : 0;
      if (breakMin >= elapsed) {
        setLaborFormError(t("execution.laborDialog.breakTooLong"));
        return;
      }
      const workedMinutes = elapsed - breakMin;
      const hoursWorked = workedMinutes / 60;
      let notes = laborNotes.trim() || null;
      if (breakMin > 0) {
        const breakNote = t("execution.laborDialog.breakNote", { minutes: breakMin });
        notes = notes ? `${notes}\n${breakNote}` : breakNote;
      }
      setBusy(true);
      setLaborFormError(null);
      setError(null);
      try {
        await addLabor({
          wo_id: wo.id,
          intervener_id: intervenerId,
          started_at: started,
          ended_at: ended,
          hours_worked: hoursWorked,
          notes,
        });
        setShowLaborDialog(false);
        await loadData();
      } catch (e) {
        setLaborFormError(t("execution.error.addManualLabor"));
        console.error("submitLaborDialog", e);
      } finally {
        setBusy(false);
      }
    }, [actorId, laborTechId, laborDate, laborStart, laborEnd, laborBreakMin, laborNotes, wo.id, loadData, t]);

    // ── Parts ──────────────────────────────────────────────────────────────

    const handleRecordPartUsage = useCallback(
      async (partId: number, fallbackQty?: number) => {
        const raw = partUsage[partId];
        const value =
          raw != null && raw !== ""
            ? Number(raw)
            : fallbackQty != null
              ? fallbackQty
              : NaN;
        if (!Number.isFinite(value) || value <= 0) {
          setError(t("execution.error.invalidQuantity"));
          return;
        }
        setBusy(true);
        setError(null);
        try {
          await recordPartUsage(partId, value);
          await loadData();
        } catch (e) {
          setError(t("execution.error.recordPart"));
          console.error("handleRecordPartUsage", e);
        } finally {
          setBusy(false);
        }
      },
      [partUsage, loadData, t],
    );

    const handleNoParts = useCallback(async () => {
      setBusy(true);
      setError(null);
      try {
        await confirmNoParts(wo.id);
        await Promise.all([loadData(), refreshActiveWo()]);
      } catch (e) {
        setError(t("execution.error.confirmNoParts"));
        console.error("handleNoParts", e);
      } finally {
        setBusy(false);
      }
    }, [wo.id, loadData, refreshActiveWo, t]);

    const handleUndoNoParts = useCallback(async () => {
      setBusy(true);
      setError(null);
      try {
        await unconfirmNoParts(wo.id);
        await Promise.all([loadData(), refreshActiveWo()]);
      } catch (e) {
        setError(t("execution.error.unconfirmNoParts"));
        console.error("handleUndoNoParts", e);
      } finally {
        setBusy(false);
      }
    }, [wo.id, loadData, refreshActiveWo, t]);

    const submitAddPart = useCallback(async () => {
      const label = addPartRef.trim();
      const qty = Number(addPartQty);
      if (!label) {
        setAddPartDialogError(t("execution.parts.add.labelRequired"));
        return;
      }
      if (!Number.isFinite(qty) || qty <= 0) {
        setAddPartDialogError(t("execution.error.invalidQuantity"));
        return;
      }
      setBusy(true);
      setAddPartDialogError(null);
      setError(null);
      try {
        await addPart({
          wo_id: wo.id,
          article_ref: label,
          quantity_planned: qty,
          auto_reserve: false,
        });
        setShowAddPartDialog(false);
        setAddPartRef("");
        setAddPartQty("1");
        await loadData();
      } catch (e) {
        setAddPartDialogError(t("execution.parts.add.error"));
        console.error("submitAddPart", e);
      } finally {
        setBusy(false);
      }
    }, [addPartRef, addPartQty, wo.id, loadData, t]);

    const openNotUsedDialog = useCallback((partId: number) => {
      setNotUsedPartId(partId);
      setNotUsedReasonId(null);
      setNotUsedComment("");
      setNotUsedError(null);
      void listPublishedReferenceValuesByDomainCode("WORK.PART_UNUSED_REASON")
        .then((rows) => {
          const map: Record<string, string> = {};
          for (const row of rows) {
            map[String(row.id)] = row.code;
          }
          setUnusedReasonCodesById(map);
        })
        .catch(() => setUnusedReasonCodesById({}));
    }, []);

    const submitNotUsed = useCallback(async () => {
      if (notUsedPartId == null) return;
      const reasonId = notUsedReasonId ? Number(notUsedReasonId) : NaN;
      if (!Number.isFinite(reasonId) || reasonId <= 0) {
        setNotUsedError(t("execution.notUsed.reasonRequired"));
        return;
      }
      const reasonCode = (
        notUsedReasonId ? unusedReasonCodesById[notUsedReasonId] : undefined
      )?.toLowerCase();
      if (reasonCode === "other" && !notUsedComment.trim()) {
        setNotUsedError(t("execution.notUsed.commentRequired"));
        return;
      }
      setBusy(true);
      setNotUsedError(null);
      try {
        await markPartNotUsed({
          wo_part_id: notUsedPartId,
          not_used_reason_id: reasonId,
          not_used_comment: notUsedComment.trim() || null,
          actor_id: actorId,
        });
        setNotUsedPartId(null);
        await loadData();
      } catch (e) {
        setNotUsedError(t("execution.error.markNotUsed"));
        console.error("submitNotUsed", e);
      } finally {
        setBusy(false);
      }
    }, [
      notUsedPartId,
      notUsedReasonId,
      notUsedComment,
      unusedReasonCodesById,
      actorId,
      loadData,
      t,
    ]);

    // ── Tasks ──────────────────────────────────────────────────────────────

    const handleCompleteTask = useCallback(
      async (task: WoExecTask) => {
        if (!actorId) return;
        const resultCode = taskResultCodes[task.id] ?? "ok";
        setBusy(true);
        setError(null);
        try {
          await completeTask(task.id, actorId, resultCode);
          await loadData();
        } catch (e) {
          setError(t("execution.error.completeTask"));
          console.error("handleCompleteTask", e);
        } finally {
          setBusy(false);
        }
      },
      [actorId, taskResultCodes, loadData, t],
    );

    // ── Downtime ───────────────────────────────────────────────────────────

    const handleOpenDowntime = useCallback(async () => {
      if (!actorId) return;
      setBusy(true);
      setError(null);
      try {
        await openDowntime(
          wo.id,
          downtimeType,
          actorId,
          downtimeComment.trim() || null,
          downtimeClassification || null,
        );
        setDowntimeComment("");
        setDowntimeClassification("");
        await loadData();
      } catch (e) {
        setError(t("execution.error.openDowntime"));
        console.error("handleOpenDowntime", e);
      } finally {
        setBusy(false);
      }
    }, [actorId, wo.id, downtimeType, downtimeComment, downtimeClassification, loadData, t]);

    const handleCloseDowntime = useCallback(
      async (segmentId: number) => {
        setBusy(true);
        setError(null);
        try {
          await closeDowntime(segmentId, nowIso());
          await loadData();
        } catch (e) {
          setError(t("execution.error.closeDowntime"));
          console.error("handleCloseDowntime", e);
        } finally {
          setBusy(false);
        }
      },
      [loadData, t],
    );

    const handleReleaseReservation = useCallback(
      async (reservationId: number) => {
        setBusy(true);
        setError(null);
        try {
          await releaseInventoryReservation({ reservation_id: reservationId });
          await loadData();
        } catch (e) {
          setError(t("execution.error.releaseReservation"));
          console.error("handleReleaseReservation", e);
        } finally {
          setBusy(false);
        }
      },
      [loadData, t],
    );

    // ── Tools ──────────────────────────────────────────────────────────────

    const submitAddTool = useCallback(async () => {
      if (!addToolLabel.trim()) {
        setAddToolDialogError(t("execution.tools.labelRequired"));
        return;
      }
      setBusy(true);
      setAddToolDialogError(null);
      try {
        await addWoTool({
          wo_id: wo.id,
          tool_label: addToolLabel.trim(),
          tool_code: addToolCode.trim() || null,
          origin: "execution_added",
        });
        setShowAddToolDialog(false);
        setAddToolLabel("");
        setAddToolCode("");
        await loadData();
      } catch (e) {
        setAddToolDialogError(t("execution.tools.addError"));
        console.error("submitAddTool", e);
      } finally {
        setBusy(false);
      }
    }, [addToolLabel, addToolCode, wo.id, loadData, t]);

    const handleMarkToolUsed = useCallback(
      async (toolId: number) => {
        setBusy(true);
        setError(null);
        try {
          await markWoToolUsed({ tool_id: toolId, usage_status: "used" });
          await loadData();
        } catch (e) {
          setError(t("execution.tools.updateError"));
        } finally {
          setBusy(false);
        }
      },
      [loadData, t],
    );

    const handleMarkToolNotUsed = useCallback(
      async (toolId: number) => {
        setBusy(true);
        setError(null);
        try {
          await markWoToolNotUsed({ tool_id: toolId, usage_status: "not_used" });
          await loadData();
        } catch (e) {
          setError(t("execution.tools.updateError"));
        } finally {
          setBusy(false);
        }
      },
      [loadData, t],
    );

    // ── Derived values ─────────────────────────────────────────────────────

    const openDowntimeSegments = useMemo(
      () => downtimeSegments.filter((row) => !row.ended_at),
      [downtimeSegments],
    );

    const downtimeTimeline = useMemo(
      () =>
        [...downtimeSegments].sort((a, b) => {
          const ta = Date.parse(a.started_at ?? "") || 0;
          const tb = Date.parse(b.started_at ?? "") || 0;
          return ta - tb;
        }),
      [downtimeSegments],
    );

    const laborTotalMinutes = useMemo(
      () =>
        laborEntries.reduce((sum, e) => {
          if (e.hours_worked != null) return sum + hoursToMinutes(e.hours_worked);
          const elapsed = elapsedMinutesBetween(e.started_at, e.ended_at);
          return sum + (elapsed ?? 0);
        }, 0),
      [laborEntries],
    );

    const plannedParts = useMemo(
      () => parts.filter((p) => !p.origin || p.origin === "planned"),
      [parts],
    );

    const actuallyUsedParts = useMemo(
      () =>
        parts.filter(
          (p) =>
            p.origin === "execution_added" ||
            (p.consumption_status === "used" || (p.quantity_used ?? 0) > 0),
        ),
      [parts],
    );

    const downtimeLostMinutes = useMemo(
      () =>
        downtimeSegments.reduce((sum, seg) => {
          const elapsed = elapsedMinutesBetween(seg.started_at, seg.ended_at ?? nowIso());
          return sum + (elapsed ?? 0);
        }, 0),
      [downtimeSegments],
    );

    const sortedEvents = useMemo(
      () =>
        [...executionEvents].sort(
          (a, b) => Date.parse(b.occurred_at) - Date.parse(a.occurred_at),
        ),
      [executionEvents],
    );

    // ── Render ─────────────────────────────────────────────────────────────

    return (
      <div className="space-y-8">
        {error && (
          <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
            {error}
          </div>
        )}

        {!loggingEnabled && (
          <p className="text-sm text-muted-foreground">{t("execution.loggingOnlyInProgress")}</p>
        )}

        {/* Delay form (pause / hold) */}
        {delayIntent && (
          <div className="space-y-3 rounded-md border border-amber-200 bg-amber-50/40 p-3">
            <h3 className="text-sm font-semibold">
              {delayIntent === "pause" ? t("execution.pauseTitle") : t("execution.holdTitle")}
            </h3>
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <Label>{t("execution.delayReason")}</Label>
                <ReferenceCombobox
                  referenceType="work.delay_reason"
                  valueMode="id"
                  value={delayReasonId}
                  onChange={setDelayReasonId}
                  allowClear={false}
                  placeholder={t("execution.selectReason")}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("execution.commentOptional")}</Label>
                <Textarea
                  rows={2}
                  value={delayComment}
                  onChange={(e) => setDelayComment(e.target.value)}
                  placeholder={t("execution.addContext")}
                />
              </div>
            </div>
            {delayError && <div className="text-sm text-red-700">{delayError}</div>}
            <div className="flex items-center gap-2">
              <Button onClick={() => void submitDelayAction()} disabled={busy || !actorId}>
                {t("execution.submit")}
              </Button>
              <Button variant="outline" onClick={() => setDelayIntent(null)}>
                {t("execution.cancel")}
              </Button>
            </div>
          </div>
        )}

        {/* ── 1. Labor ───────────────────────────────────────────────────── */}
        <section className="space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <h3 className="text-base font-semibold tracking-tight">{t("execution.sectionLabor")}</h3>
              <p className="text-sm text-muted-foreground">
                {t("execution.summaryLabor", {
                  count: laborEntries.length,
                  total: formatDurationMinutes(laborTotalMinutes, "auto", durationLabels),
                })}
              </p>
            </div>
            <Button size="sm" onClick={openLaborDialog} disabled={controlsDisabled || !actorId}>
              {t("execution.addLabor")}
            </Button>
          </div>

          {/* Plan vs Actual for labor */}
          {(planAdherence?.planned_hours != null || laborTotalMinutes > 0) && (
            <div className="rounded-md border bg-muted/30 p-3">
              <LaborMetricRow
                labelPlanned={t("execution.labor.planned")}
                labelActual={t("execution.labor.actual")}
                labelVariance={t("execution.labor.variance")}
                labelEfficiency={t("execution.labor.efficiency")}
                planned={planAdherence?.planned_hours ?? null}
                actual={laborTotalMinutes}
                durationLabels={durationLabels}
              />
            </div>
          )}

          {laborEntries.length === 0 ? (
            <div className="rounded-md border border-dashed px-4 py-6 text-center">
              <p className="text-sm text-muted-foreground">{t("execution.emptyLabor")}</p>
              <Button
                className="mt-3"
                size="sm"
                variant="outline"
                onClick={openLaborDialog}
                disabled={controlsDisabled || !actorId}
              >
                {t("execution.addLabor")}
              </Button>
            </div>
          ) : (
            <div className="space-y-2">
              {laborEntries.map((entry) => {
                const duration =
                  entry.hours_worked != null
                    ? formatHoursWorked(entry.hours_worked, "auto", durationLabels)
                    : formatDurationMinutes(
                        elapsedMinutesBetween(entry.started_at, entry.ended_at) ?? 0,
                        "auto",
                        durationLabels,
                      );
                return (
                  <div
                    key={entry.id}
                    className="grid gap-2 border-b border-border/60 py-2 text-sm last:border-0 md:grid-cols-[1.2fr_1fr_1fr_100px]"
                  >
                    <div className="font-medium">{formatPersonLabel(entry.intervener_display_name)}</div>
                    <div className="text-muted-foreground">
                      {formatDateTime(entry.started_at, i18n.language)}
                    </div>
                    <div className="text-muted-foreground">
                      {formatDateTime(entry.ended_at, i18n.language)}
                    </div>
                    <div className="font-medium tabular-nums">{duration}</div>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        {/* ── 2. Parts — Planned pane + Actually used pane ──────────────── */}
        <section className="space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <h3 className="text-sm font-semibold text-muted-foreground">
                {t("execution.sectionParts")}
              </h3>
              <p className="text-sm text-muted-foreground">
                {plannedParts.length === 0
                  ? t("execution.parts.summaryNonePlanned")
                  : t("execution.parts.summaryPlanned", {
                      pending: plannedParts.filter(
                        (p) =>
                          p.consumption_status !== "used" &&
                          p.consumption_status !== "not_used" &&
                          !(p.quantity_used != null && p.quantity_used > 0),
                      ).length,
                      total: plannedParts.length,
                    })}
              </p>
            </div>
          </div>

          {/* Planned pane */}
          <div className="space-y-2">
            <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {t("execution.parts.plannedTitle")}
            </p>

            {plannedParts.length === 0 ? (
              <div className="rounded-md border border-dashed px-4 py-4 text-center text-sm text-muted-foreground">
                {t("execution.parts.nonePlanned")}
              </div>
            ) : (
              <div className="space-y-2">
                {plannedParts.map((part) => {
                  const isUsed =
                    part.consumption_status === "used" || (part.quantity_used ?? 0) > 0;
                  const isNotUsed = part.consumption_status === "not_used";
                  return (
                    <div
                      key={part.id}
                      className="grid gap-2 rounded-md border bg-background px-3 py-2 text-sm md:grid-cols-[2fr_90px_130px_auto]"
                    >
                      <div>
                        <span className="font-medium">
                          {formatOrDash(part.article_ref ?? part.article_label)}
                        </span>
                        <OriginBadge origin={part.origin} t={t} />
                        {isNotUsed && part.not_used_reason_label && (
                          <p className="mt-0.5 text-xs text-muted-foreground">
                            {t("execution.notUsed.reason")}: {part.not_used_reason_label}
                            {part.not_used_comment && ` — ${part.not_used_comment}`}
                          </p>
                        )}
                      </div>
                      <div className="text-muted-foreground tabular-nums">
                        ×{part.quantity_planned}
                      </div>
                      {isUsed || isNotUsed ? (
                        <div className="flex items-center gap-1">
                          <Badge variant={isUsed ? "default" : "secondary"} className="text-[10px]">
                            {isUsed
                              ? t("execution.parts.badge.used", {
                                  qty: part.quantity_used ?? 0,
                                })
                              : t("execution.parts.badge.notUsed")}
                          </Badge>
                        </div>
                      ) : (
                        <div className="flex items-center gap-1">
                          <Input
                            type="number"
                            min="0"
                            step="0.01"
                            value={partUsage[part.id] ?? String(part.quantity_planned)}
                            onChange={(e) =>
                              setPartUsage((prev) => ({ ...prev, [part.id]: e.target.value }))
                            }
                            disabled={controlsDisabled}
                            placeholder={t("execution.usedQty")}
                            className="h-7 w-24"
                          />
                        </div>
                      )}
                      <div className="flex items-center gap-1">
                        {!isUsed && !isNotUsed && (
                          <>
                            <Button
                              size="sm"
                              className="h-7 px-2 text-xs"
                              onClick={() =>
                                void handleRecordPartUsage(part.id, part.quantity_planned)
                              }
                              disabled={controlsDisabled}
                            >
                              {t("execution.parts.markUsed")}
                            </Button>
                            <Button
                              size="sm"
                              className="h-7 px-2 text-xs"
                              variant="outline"
                              onClick={() => openNotUsedDialog(part.id)}
                              disabled={controlsDisabled}
                            >
                              {t("execution.notUsed.cta")}
                            </Button>
                          </>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* Actually used pane */}
          <div className="space-y-2 pt-1">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                {t("execution.parts.actualTitle")}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  onClick={() => {
                    setAddPartDialogError(null);
                    setAddPartRef("");
                    setAddPartQty("1");
                    setShowAddPartDialog(true);
                  }}
                  disabled={controlsDisabled}
                >
                  {t("execution.parts.add.cta")}
                </Button>
                {plannedParts.length === 0 && !wo.parts_actuals_confirmed && (
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-7 text-xs"
                    onClick={() => void handleNoParts()}
                    disabled={controlsDisabled || actuallyUsedParts.length > 0}
                  >
                    {t("execution.noPartsUsed")}
                  </Button>
                )}
              </div>
            </div>

            {wo.parts_actuals_confirmed &&
            plannedParts.length === 0 &&
            actuallyUsedParts.length === 0 ? (
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-green-200 bg-green-50/50 px-4 py-3 text-sm text-green-800">
                <span className="font-medium">{t("execution.parts.noneConfirmed")}</span>
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs"
                  onClick={() => void handleUndoNoParts()}
                  disabled={controlsDisabled}
                >
                  {t("execution.parts.undoNoneConfirmed")}
                </Button>
              </div>
            ) : actuallyUsedParts.length === 0 ? (
              <div className="rounded-md border border-dashed px-4 py-4 text-center text-sm text-muted-foreground">
                {t("execution.parts.actualEmpty")}
              </div>
            ) : (
              <div className="space-y-1">
                {actuallyUsedParts.map((part) => (
                  <div
                    key={part.id}
                    className="flex flex-wrap items-center gap-2 rounded-md border bg-background px-3 py-2 text-sm"
                  >
                    <span className="flex-1 font-medium">
                      {formatOrDash(part.article_ref ?? part.article_label)}
                    </span>
                    <span className="text-muted-foreground tabular-nums">
                      {t("execution.parts.usedQtyLabel", { qty: part.quantity_used ?? 0 })}
                    </span>
                    <OriginBadge origin={part.origin} t={t} />
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Reservations */}
          {reservations.length > 0 && (
            <div className="space-y-2 pt-1">
              <p className="text-xs font-medium text-muted-foreground">
                {t("execution.reservations")}
              </p>
              {reservations.map((reservation) => (
                <div
                  key={reservation.id}
                  className="flex flex-wrap items-center justify-between gap-2 text-sm"
                >
                  <span>
                    {formatAssetLabel(reservation.article_code, reservation.article_name)} ·{" "}
                    {reservation.quantity_reserved}
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void handleReleaseReservation(reservation.id)}
                    disabled={controlsDisabled}
                  >
                    {t("execution.releaseReservation")}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* ── 3. Tasks ───────────────────────────────────────────────────── */}
        <section className="space-y-3">
          <div className="flex items-start justify-between gap-2">
            <div>
              <h3 className="text-sm font-semibold text-muted-foreground">
                {t("execution.sectionTasks")}
              </h3>
              <p className="text-sm text-muted-foreground">
                {t("execution.summaryTasks", {
                  done: tasks.filter((task) => Boolean(task.completed_at)).length,
                  total: tasks.length,
                })}
              </p>
            </div>
          </div>

          {tasks.length === 0 ? (
            <div className="rounded-md border border-dashed px-4 py-5 text-center text-sm text-muted-foreground">
              {t("execution.emptyTasks")}
            </div>
          ) : (
            <div className="space-y-2">
              {tasks.map((task) => (
                <div
                  key={task.id}
                  className="grid gap-2 border-b border-border/60 py-2 text-sm last:border-0 md:grid-cols-[2fr_160px_auto]"
                >
                  <div>
                    <span className="font-medium">{task.task_description}</span>
                    <OriginBadge origin={task.origin} t={t} />
                    {task.completed_at ? (
                      <span className="ml-2 text-xs text-muted-foreground">
                        {task.result_code === "cancelled"
                          ? t("execution.taskCancelled")
                          : t("execution.taskDone")}
                      </span>
                    ) : null}
                  </div>
                  <Select
                    value={taskResultCodes[task.id] ?? "ok"}
                    onValueChange={(v) =>
                      setTaskResultCodes((prev) => ({
                        ...prev,
                        [task.id]: v as TaskResultCode,
                      }))
                    }
                    disabled={controlsDisabled || Boolean(task.completed_at)}
                  >
                    <SelectTrigger className="h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {TASK_RESULT_OPTIONS.map((code) => (
                        <SelectItem key={code} value={code}>
                          {t(`execution.taskResult.${code}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  {!task.completed_at && (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void handleCompleteTask(task)}
                      disabled={controlsDisabled || !actorId}
                    >
                      {taskResultCodes[task.id] === "cancelled"
                        ? t("execution.cancelTask")
                        : t("execution.complete")}
                    </Button>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>

        {/* ── 4. Downtime ────────────────────────────────────────────────── */}
        <section className="space-y-3">
          <div className="flex flex-wrap items-start justify-between gap-2">
            <div>
              <h3 className="text-sm font-semibold">{t("execution.sectionDowntime")}</h3>
              <p className="text-sm text-muted-foreground">
                {t("execution.summaryDowntime", {
                  open: openDowntimeSegments.length,
                  lost: formatDurationMinutes(downtimeLostMinutes, "clock", durationLabels),
                })}
              </p>
            </div>
          </div>

          {/* Planned vs Actual downtime */}
          {(wo.planned_downtime_hours != null || downtimeLostMinutes > 0) && (
            <div className="grid grid-cols-2 gap-2 rounded-md border bg-muted/30 p-3 text-sm sm:grid-cols-3">
              <div className="space-y-0.5">
                <p className="text-xs text-muted-foreground">{t("execution.downtime.planned")}</p>
                <p className="font-medium tabular-nums">
                  {wo.planned_downtime_hours != null
                    ? formatDurationMinutes(hoursToMinutes(wo.planned_downtime_hours), "auto", durationLabels)
                    : "—"}
                </p>
              </div>
              <div className="space-y-0.5">
                <p className="text-xs text-muted-foreground">{t("execution.downtime.actual")}</p>
                <p className="font-medium tabular-nums">
                  {formatDurationMinutes(downtimeLostMinutes, "auto", durationLabels)}
                </p>
              </div>
              {planAdherence?.primary_downtime_cause && (
                <div className="space-y-0.5">
                  <p className="text-xs text-muted-foreground">{t("execution.downtime.cause")}</p>
                  <p className="font-medium">{planAdherence.primary_downtime_cause}</p>
                </div>
              )}
            </div>
          )}

          <div className="flex flex-wrap items-end gap-2">
            <div className="space-y-1">
              <Label className="text-xs">{t("execution.downtimeType")}</Label>
              <Select
                value={downtimeType}
                onValueChange={(v) => setDowntimeType(v as DowntimeType)}
                disabled={controlsDisabled || openDowntimeSegments.length > 0}
              >
                <SelectTrigger className="w-[140px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="full">{t("execution.downtime_full")}</SelectItem>
                  <SelectItem value="partial">{t("execution.downtime_partial")}</SelectItem>
                  <SelectItem value="standby">{t("execution.downtime_standby")}</SelectItem>
                  <SelectItem value="quality_loss">{t("execution.downtime_quality_loss")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label className="text-xs">{t("execution.downtimeClassification")}</Label>
              <Select
                value={downtimeClassification || "__none__"}
                onValueChange={(v) => setDowntimeClassification(v === "__none__" ? "" : v)}
                disabled={controlsDisabled || openDowntimeSegments.length > 0}
              >
                <SelectTrigger className="w-[180px]">
                  <SelectValue placeholder={t("execution.downtimeClassificationNone")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("execution.downtimeClassificationNone")}</SelectItem>
                  <SelectItem value="mechanical">{t("execution.downtimeClass.mechanical")}</SelectItem>
                  <SelectItem value="electrical">{t("execution.downtimeClass.electrical")}</SelectItem>
                  <SelectItem value="waiting_spare">{t("execution.downtimeClass.waiting_spare")}</SelectItem>
                  <SelectItem value="waiting_approval">{t("execution.downtimeClass.waiting_approval")}</SelectItem>
                  <SelectItem value="operator_unavailable">{t("execution.downtimeClass.operator_unavailable")}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="min-w-[180px] flex-1 space-y-1">
              <Label className="text-xs">{t("execution.commentOptional")}</Label>
              <Input
                value={downtimeComment}
                onChange={(e) => setDowntimeComment(e.target.value)}
                disabled={controlsDisabled || openDowntimeSegments.length > 0}
                placeholder={t("execution.addContext")}
              />
            </div>
            <Button
              size="sm"
              onClick={() => void handleOpenDowntime()}
              disabled={controlsDisabled || !actorId || openDowntimeSegments.length > 0}
            >
              {t("execution.openDowntime")}
            </Button>
          </div>

          {downtimeTimeline.length === 0 ? (
            <div className="rounded-md border border-dashed px-4 py-5 text-center text-sm text-muted-foreground">
              {t("execution.emptyDowntime")}
            </div>
          ) : (
            <ol className="relative ml-2 space-y-0 border-l border-border">
              {downtimeTimeline.map((seg) => {
                const duration = formatDurationMinutes(
                  elapsedMinutesBetween(seg.started_at, seg.ended_at ?? nowIso()) ?? 0,
                  "clock",
                  durationLabels,
                );
                return (
                  <li key={seg.id} className="relative py-3 pl-4">
                    <span className="absolute -left-1.5 top-4 h-3 w-3 rounded-full border-2 border-background bg-muted-foreground/60" />
                    <div className="flex flex-wrap items-start justify-between gap-2 text-sm">
                      <div>
                        <p className="font-medium">{t(`execution.downtime_${seg.downtime_type}`)}</p>
                        <p className="text-muted-foreground">
                          {formatDateTime(seg.started_at, i18n.language)}
                          {" → "}
                          {seg.ended_at
                            ? formatDateTime(seg.ended_at, i18n.language)
                            : t("execution.downtimeOpen")}
                          {" · "}
                          {duration}
                        </p>
                        {seg.comment ? (
                          <p className="mt-1 text-muted-foreground">{seg.comment}</p>
                        ) : null}
                        {seg.classification_code ? (
                          <p className="mt-1 text-xs text-muted-foreground">
                            {t(`execution.downtimeClass.${seg.classification_code}`, {
                              defaultValue: seg.classification_code,
                            })}
                          </p>
                        ) : null}
                      </div>
                      {!seg.ended_at && (
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => void handleCloseDowntime(seg.id)}
                          disabled={controlsDisabled}
                        >
                          {t("execution.closeDowntime")}
                        </Button>
                      )}
                    </div>
                  </li>
                );
              })}
            </ol>
          )}
        </section>

        {/* ── 5. Tools ───────────────────────────────────────────────────── */}
        <section className="space-y-3">
          <div className="flex items-start justify-between gap-2">
            <h3 className="text-sm font-semibold text-muted-foreground">
              {t("execution.sectionTools")}
            </h3>
            <Button
              size="sm"
              variant="outline"
              className="h-7 text-xs"
              onClick={() => {
                setAddToolLabel("");
                setAddToolCode("");
                setAddToolDialogError(null);
                setShowAddToolDialog(true);
              }}
              disabled={controlsDisabled}
            >
              {t("execution.tools.add")}
            </Button>
          </div>

          {tools.length === 0 ? (
            <div className="rounded-md border border-dashed px-4 py-4 text-center text-sm text-muted-foreground">
              {t("execution.tools.empty")}
            </div>
          ) : (
            <div className="space-y-1">
              {tools.map((tool) => (
                <div
                  key={tool.id}
                  className="flex flex-wrap items-center gap-2 rounded-md border bg-background px-3 py-2 text-sm"
                >
                  <span className="flex-1 font-medium">{tool.tool_label}</span>
                  {tool.tool_code && (
                    <span className="text-xs text-muted-foreground">{tool.tool_code}</span>
                  )}
                  <Badge
                    variant={
                      tool.usage_status === "used"
                        ? "default"
                        : tool.usage_status === "not_used"
                          ? "secondary"
                          : "outline"
                    }
                    className="text-[10px]"
                  >
                    {t(`execution.tools.status.${tool.usage_status}`, {
                      defaultValue: tool.usage_status,
                    })}
                  </Badge>
                  <Badge
                    variant={tool.origin === "execution_added" ? "secondary" : "outline"}
                    className="text-[10px]"
                  >
                    {tool.origin === "execution_added"
                      ? t("execution.badge.executionAdded")
                      : t("execution.badge.planned")}
                  </Badge>
                  {tool.usage_status === "planned" && (
                    <>
                      <Button
                        size="sm"
                        variant="outline"
                        className="h-6 px-2 text-xs"
                        onClick={() => void handleMarkToolUsed(tool.id)}
                        disabled={controlsDisabled}
                      >
                        {t("execution.tools.markUsed")}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 px-2 text-xs"
                        onClick={() => void handleMarkToolNotUsed(tool.id)}
                        disabled={controlsDisabled}
                      >
                        {t("execution.tools.markNotUsed")}
                      </Button>
                    </>
                  )}
                </div>
              ))}
            </div>
          )}
        </section>

        {/* ── 6. Execution Log ───────────────────────────────────────────── */}
        <section className="space-y-2">
          <h3 className="text-sm font-semibold text-muted-foreground">
            {t("execution.executionLog.title")}
          </h3>
          {sortedEvents.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t("execution.executionLog.empty")}</p>
          ) : (
            <ol className="relative ml-2 space-y-0 border-l border-border">
              {sortedEvents.map((ev) => {
                let params: Record<string, string> = {};
                try {
                  if (ev.summary_params_json) {
                    params = JSON.parse(ev.summary_params_json) as Record<string, string>;
                  }
                } catch {
                  // non-fatal
                }
                return (
                  <li key={ev.id} className="relative py-2 pl-4 text-sm">
                    <span className="absolute -left-1 top-3.5 h-2 w-2 rounded-full bg-muted-foreground/40" />
                    <p className="text-muted-foreground">
                      <span className="font-medium text-foreground">
                        {t(executionLogI18nKey(ev.summary_key), {
                          ...params,
                          defaultValue: ev.summary_key,
                        })}
                      </span>
                      {ev.actor_display_name && ` · ${ev.actor_display_name}`}
                    </p>
                    <p className="text-xs text-muted-foreground/70">
                      {formatDateTime(ev.occurred_at, i18n.language)}
                    </p>
                  </li>
                );
              })}
            </ol>
          )}
        </section>

        {/* ── 7. Validation summary ──────────────────────────────────────── */}
        <section className="space-y-2 border-t pt-4">
          <h3 className="text-sm font-semibold">{t("execution.sectionValidation")}</h3>
          <p className="text-sm text-muted-foreground">{t("execution.validationHint")}</p>
          <ul className="space-y-1 text-sm">
            <li>
              {t("execution.sectionLabor")}:{" "}
              {formatDurationMinutes(laborTotalMinutes, "auto", durationLabels)}
            </li>
            <li>
              {t("execution.sectionParts")}:{" "}
              {parts.filter((p) => (p.quantity_used ?? 0) > 0).length}/{parts.length}
            </li>
            <li>
              {t("execution.sectionTasks")}:{" "}
              {tasks.filter((task) => Boolean(task.completed_at)).length}/{tasks.length}
            </li>
            <li>
              {t("execution.sectionDowntime")}:{" "}
              {formatDurationMinutes(downtimeLostMinutes, "clock", durationLabels)}
            </li>
          </ul>
        </section>

        {/* ── Dialogs ────────────────────────────────────────────────────── */}

        {/* Labor dialog */}
        <Dialog open={showLaborDialog} onOpenChange={setShowLaborDialog}>
          <DialogContent className="max-w-md">
            <DialogHeader>
              <DialogTitle>{t("execution.addLabor")}</DialogTitle>
              <DialogDescription>{t("execution.laborDialog.hint")}</DialogDescription>
            </DialogHeader>
            <div className="grid gap-3">
              <div className="space-y-1">
                <Label>{t("execution.laborDialog.technician")}</Label>
                <Select
                  value={laborTechId || "__none"}
                  onValueChange={(v) => setLaborTechId(v === "__none" ? "" : v)}
                >
                  <SelectTrigger>
                    <SelectValue placeholder={t("execution.intervenerId")} />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none">—</SelectItem>
                    {intervenerOptions.map((opt) => (
                      <SelectItem key={opt.value} value={opt.value}>
                        {opt.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>{t("execution.laborDialog.date")}</Label>
                <Input type="date" value={laborDate} onChange={(e) => setLaborDate(e.target.value)} />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1">
                  <Label>{t("execution.laborDialog.startTime")}</Label>
                  <Input
                    type="time"
                    value={laborStart}
                    onChange={(e) => setLaborStart(e.target.value)}
                  />
                </div>
                <div className="space-y-1">
                  <Label>{t("execution.laborDialog.endTime")}</Label>
                  <Input
                    type="time"
                    value={laborEnd}
                    onChange={(e) => setLaborEnd(e.target.value)}
                  />
                </div>
              </div>
              <div className="space-y-1">
                <Label>{t("execution.laborDialog.breakOptional")}</Label>
                <Input
                  type="number"
                  min="0"
                  step="1"
                  value={laborBreakMin}
                  onChange={(e) => setLaborBreakMin(e.target.value)}
                  placeholder="0"
                />
              </div>
              <div className="space-y-1">
                <Label>{t("execution.laborDialog.comments")}</Label>
                <Textarea rows={2} value={laborNotes} onChange={(e) => setLaborNotes(e.target.value)} />
              </div>
              <p className="text-sm text-muted-foreground">
                {t("execution.laborDialog.workedPreview")}:{" "}
                <span className="font-medium text-foreground">
                  {laborPreviewMinutes != null
                    ? formatDurationMinutes(laborPreviewMinutes, "auto", durationLabels)
                    : "—"}
                </span>
              </p>
              {laborFormError && <p className="text-sm text-red-700">{laborFormError}</p>}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setShowLaborDialog(false)}>
                {t("execution.cancel")}
              </Button>
              <Button onClick={() => void submitLaborDialog()} disabled={busy || !actorId}>
                {t("execution.laborDialog.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Not-used dialog */}
        <Dialog open={notUsedPartId != null} onOpenChange={(open) => !open && setNotUsedPartId(null)}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>{t("execution.notUsed.title")}</DialogTitle>
              <DialogDescription>{t("execution.notUsed.hint")}</DialogDescription>
            </DialogHeader>
            <div className="space-y-3">
              <div className="space-y-1">
                <Label>{t("execution.notUsed.reason")}</Label>
                <ReferenceCombobox
                  referenceType="work.part_unused_reason"
                  valueMode="id"
                  value={notUsedReasonId}
                  onChange={setNotUsedReasonId}
                  allowClear={false}
                  placeholder={t("execution.selectReason")}
                />
              </div>
              <div className="space-y-1">
                <Label>
                  {t("execution.notUsed.comment")}
                  {(notUsedReasonId
                    ? unusedReasonCodesById[notUsedReasonId]?.toLowerCase()
                    : undefined) === "other"
                    ? ` (${t("execution.notUsed.commentRequiredShort")})`
                    : ""}
                </Label>
                <Textarea
                  rows={2}
                  value={notUsedComment}
                  onChange={(e) => setNotUsedComment(e.target.value)}
                  placeholder={t("execution.addContext")}
                />
              </div>
              {notUsedError && <p className="text-sm text-red-700">{notUsedError}</p>}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setNotUsedPartId(null)}>
                {t("execution.cancel")}
              </Button>
              <Button onClick={() => void submitNotUsed()} disabled={busy}>
                {t("execution.notUsed.confirm")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Add part during execution */}
        <Dialog open={showAddPartDialog} onOpenChange={setShowAddPartDialog}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>{t("execution.parts.add.title")}</DialogTitle>
              <DialogDescription>{t("execution.parts.add.hint")}</DialogDescription>
            </DialogHeader>
            <div className="space-y-3">
              <div className="space-y-1">
                <Label>{t("execution.parts.add.label")}</Label>
                <Input
                  value={addPartRef}
                  onChange={(e) => setAddPartRef(e.target.value)}
                  placeholder={t("execution.parts.add.labelPlaceholder")}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("execution.parts.add.qty")}</Label>
                <Input
                  type="number"
                  min="0.01"
                  step="0.01"
                  value={addPartQty}
                  onChange={(e) => setAddPartQty(e.target.value)}
                />
              </div>
              {addPartDialogError && (
                <p className="text-sm text-red-700">{addPartDialogError}</p>
              )}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setShowAddPartDialog(false)}>
                {t("execution.cancel")}
              </Button>
              <Button onClick={() => void submitAddPart()} disabled={busy}>
                {t("execution.parts.add.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Add tool dialog */}
        <Dialog open={showAddToolDialog} onOpenChange={setShowAddToolDialog}>
          <DialogContent className="max-w-sm">
            <DialogHeader>
              <DialogTitle>{t("execution.tools.addTitle")}</DialogTitle>
            </DialogHeader>
            <div className="space-y-3">
              <div className="space-y-1">
                <Label>{t("execution.tools.label")}</Label>
                <Input
                  value={addToolLabel}
                  onChange={(e) => setAddToolLabel(e.target.value)}
                  placeholder={t("execution.tools.labelPlaceholder")}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("execution.tools.code")}</Label>
                <Input
                  value={addToolCode}
                  onChange={(e) => setAddToolCode(e.target.value)}
                  placeholder={t("execution.tools.codePlaceholder")}
                />
              </div>
              {addToolDialogError && (
                <p className="text-sm text-red-700">{addToolDialogError}</p>
              )}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setShowAddToolDialog(false)}>
                {t("execution.cancel")}
              </Button>
              <Button onClick={() => void submitAddTool()} disabled={busy}>
                {t("execution.tools.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    );
  },
);
