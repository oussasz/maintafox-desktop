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

import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PartOptionalChip } from "@/components/common/PartNecessityChoice";
import { WoPermitGateCard } from "@/components/permits/WoPermitGateCard";
import { QualificationCheckNotice } from "@/components/personnel/qualification/QualificationCheckNotice";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { Timeline } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DateField } from "@/components/ui/date-field";
import { DecimalField } from "@/components/ui/decimal-field";
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
import { TimeInput } from "@/components/ui/time-input";
import { isRecordingStatus } from "@/components/wo/wo-recording-status";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
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
import {
  listInventoryReservations,
  releaseInventoryReservation,
} from "@/services/inventory-service";
import {
  type PersonQualificationCheck,
  checkPersonForWorkOrder,
} from "@/services/qualification-gate-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import {
  type TaskResultCode,
  type WoDowntimeSegment,
  type WoExecutionEvent,
  type WoIntervener,
  type WoPlanAdherence,
  type WoTool,
  addPersonnelLabor,
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
  reopenTask,
  unconfirmNoParts,
} from "@/services/wo-execution-service";
import { holdWo, listWos, pauseWo, startWo } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { formatDateTime } from "@/utils/format-date";
import type {
  DowntimeType,
  StockReservation,
  WoExecPart,
  WoExecTask,
  WorkOrder,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { WoAddExecutionPartDialog } from "./WoAddExecutionPartDialog";
import { mergeLaborPeople, useWoLaborPeople } from "./use-wo-labor-people";
import { woActionFailureText } from "./wo-action-error";

/** Typed usage (kept as text in state) for the field; the planned quantity until the user types. */
function usageValue(typed: string | undefined, planned: number): number | null {
  if (typed === undefined) return planned;
  return typed.trim() === "" ? null : Number(typed);
}

export interface WoExecutionControlsHandle {
  openHoldForm: () => void;
  start: () => Promise<void>;
}

interface WoExecutionControlsProps {
  wo: WorkOrder;
  canEdit: boolean;
}

type DelayIntent = "pause" | "hold";

const LOGGING_EDITABLE_STATUSES = new Set(["in_progress"]);
const TASK_RESULT_OPTIONS: TaskResultCode[] = ["ok", "nok", "na", "deferred", "cancelled"];
/** A mandatory task closes only with OK, N/A, or NOK with a finding or a follow-up WO (D3). */
const MANDATORY_TASK_RESULT_OPTIONS: TaskResultCode[] = ["ok", "nok", "na"];

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
  const variance = planned != null && actual > 0 ? actual - hoursToMinutes(planned) : null;
  const efficiency =
    planned != null && actual > 0 ? Math.round((hoursToMinutes(planned) / actual) * 100) : null;

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
                ? "font-medium tabular-nums text-tone-warning-fg"
                : "font-medium tabular-nums text-tone-success-fg"
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
                ? "font-medium tabular-nums text-tone-success-fg"
                : "font-medium tabular-nums text-tone-warning-fg"
          }
        >
          {efficiency != null ? `${efficiency} %` : "—"}
        </p>
      </div>
    </div>
  );
}

/** Small badge showing part/task origin. */
function OriginBadge({
  origin,
  t,
}: {
  origin?: string | null | undefined;
  t: (k: string) => string;
}) {
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
    const { formatCurrency } = useFormatters();
    const { info } = useSession();
    const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);

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
    // NOK evidence (D3) and "Rouvrir la tâche" (comment required).
    const [taskFindingNotes, setTaskFindingNotes] = useState<Record<number, string>>({});
    const [taskFollowUpCodes, setTaskFollowUpCodes] = useState<Record<number, string>>({});
    const [reopenTaskId, setReopenTaskId] = useState<number | null>(null);
    const [reopenComment, setReopenComment] = useState("");

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

    // Add tool dialog
    const [showAddToolDialog, setShowAddToolDialog] = useState(false);
    const [addToolLabel, setAddToolLabel] = useState("");
    const [addToolCode, setAddToolCode] = useState("");
    const [addToolDialogError, setAddToolDialogError] = useState<string | null>(null);

    // Downtime
    const [downtimeType, setDowntimeType] = useState<DowntimeType>("full");
    const [downtimeComment, setDowntimeComment] = useState("");
    const [downtimeClassification, setDowntimeClassification] = useState<string>("");
    const [unusedReasonCodesById, setUnusedReasonCodesById] = useState<Record<string, string>>({});

    const [busy, setBusy] = useState(false);
    const [error, setError] = useState<string | null>(null);

    // ── Intervener options ──────────────────────────────────────────────────

    // Personnel register: everyone active, with or without a login (contractor technicians);
    // people already booked stay selectable.
    const people = useWoLaborPeople();
    const intervenerOptions = useMemo(() => {
      const booked = laborEntries
        .filter(
          (entry) =>
            entry.personnel_id != null && formatPersonLabel(entry.intervener_display_name) !== "—",
        )
        .map((entry) => ({
          value: String(entry.personnel_id),
          label: formatPersonLabel(entry.intervener_display_name),
        }));
      return mergeLaborPeople(people.options, booked);
    }, [people.options, laborEntries]);

    // Does the chosen person hold what this OT requires? (refused at booking otherwise)
    const [laborOvertime, setLaborOvertime] = useState(false);
    const [laborCheck, setLaborCheck] = useState<PersonQualificationCheck | null>(null);
    useEffect(() => {
      if (!showLaborDialog || !laborTechId) {
        setLaborCheck(null);
        return;
      }
      let cancelled = false;
      checkPersonForWorkOrder(Number(laborTechId), wo.id)
        .then((check) => {
          if (!cancelled) setLaborCheck(check);
        })
        .catch(() => {
          if (!cancelled) setLaborCheck(null);
        });
      return () => {
        cancelled = true;
      };
    }, [showLaborDialog, laborTechId, wo.id]);

    // ── Load data ──────────────────────────────────────────────────────────

    const loadData = useCallback(async () => {
      try {
        const [
          laborRows,
          partRows,
          taskRows,
          downtimeRows,
          reservationRows,
          toolRows,
          eventRows,
          adherenceRow,
        ] = await Promise.all([
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
        setError(woActionFailureText(t("execution.error.loadData"), e));
      }
    }, [wo.id, t]);

    useEffect(() => {
      void loadData();
    }, [loadData]);

    useEffect(() => {
      setStatusCode(wo.status_code ?? "draft");
      setRowVersion(wo.row_version);
    }, [wo.status_code, wo.row_version]);

    const { can } = usePermissions();
    const actorId = info?.user_id ?? null;
    const loggingEnabled = LOGGING_EDITABLE_STATUSES.has(statusCode);
    const controlsDisabled = !canEdit || !loggingEnabled || busy;
    // Task results, NOK evidence, task reopening, tool use and labour are recorded in progress and on
    // hold (backend `TASK_RECORDING_STATUSES`, CMMS Advisor O3); recording during a hold never
    // resumes the work order.
    const recordingEnabled = isRecordingStatus(statusCode);
    const recordingDisabled = !canEdit || !recordingEnabled || busy;

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
        setError(woActionFailureText(t("execution.error.startWo"), e));
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
      }),
      [openDelayForm, handleStart],
    );

    const submitDelayAction = useCallback(async () => {
      if (!actorId || !delayIntent) return;
      const reasonId = delayReasonId ? Number(delayReasonId) : Number.NaN;
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
        setError(woActionFailureText(t("execution.error.submitDelay"), e));
      } finally {
        setBusy(false);
      }
    }, [
      actorId,
      delayIntent,
      delayReasonId,
      delayComment,
      wo.id,
      rowVersion,
      handleRefreshState,
      t,
    ]);

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
      setLaborOvertime(false);
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
        await addPersonnelLabor({
          wo_id: wo.id,
          personnel_id: intervenerId,
          is_overtime: laborOvertime,
          started_at: started,
          ended_at: ended,
          hours_worked: hoursWorked,
          notes,
        });
        setShowLaborDialog(false);
        await loadData();
      } catch (e) {
        setLaborFormError(woActionFailureText(t("execution.error.addManualLabor"), e));
      } finally {
        setBusy(false);
      }
    }, [
      actorId,
      laborTechId,
      laborDate,
      laborStart,
      laborEnd,
      laborBreakMin,
      laborNotes,
      laborOvertime,
      wo.id,
      loadData,
      t,
    ]);

    // ── Parts ──────────────────────────────────────────────────────────────

    const handleRecordPartUsage = useCallback(
      async (partId: number, fallbackQty?: number) => {
        const raw = partUsage[partId];
        const value =
          raw != null && raw !== "" ? Number(raw) : fallbackQty != null ? fallbackQty : Number.NaN;
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
          setError(woActionFailureText(t("execution.error.recordPart"), e));
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
        setError(woActionFailureText(t("execution.error.confirmNoParts"), e));
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
        setError(woActionFailureText(t("execution.error.unconfirmNoParts"), e));
      } finally {
        setBusy(false);
      }
    }, [wo.id, loadData, refreshActiveWo, t]);

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
      const reasonId = notUsedReasonId ? Number(notUsedReasonId) : Number.NaN;
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
        setNotUsedError(woActionFailureText(t("execution.error.markNotUsed"), e));
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
        const findingNote = (taskFindingNotes[task.id] ?? "").trim();
        const followUpCode = (taskFollowUpCodes[task.id] ?? "").trim();
        setBusy(true);
        setError(null);
        try {
          let followUpWorkOrderId: number | null = null;
          if (resultCode === "nok" && followUpCode !== "") {
            const page = await listWos({ search: followUpCode, limit: 20, offset: 0 });
            const match = page.items.find(
              (item) => item.code.toLowerCase() === followUpCode.toLowerCase(),
            );
            if (!match) {
              setError(t("execution.taskFollowUpNotFound", { code: followUpCode }));
              return;
            }
            followUpWorkOrderId = match.id;
          }
          await completeTask(task.id, actorId, resultCode, null, {
            findingNote: resultCode === "nok" && findingNote !== "" ? findingNote : null,
            followUpWorkOrderId,
          });
          await loadData();
        } catch (e) {
          setError(woActionFailureText(t("execution.error.completeTask"), e));
        } finally {
          setBusy(false);
        }
      },
      [actorId, taskResultCodes, taskFindingNotes, taskFollowUpCodes, loadData, t],
    );

    const handleReopenTask = useCallback(
      async (task: WoExecTask) => {
        const comment = reopenComment.trim();
        if (comment === "") return;
        setBusy(true);
        setError(null);
        try {
          await reopenTask(task.id, comment);
          setReopenTaskId(null);
          setReopenComment("");
          await loadData();
        } catch (e) {
          setError(woActionFailureText(t("execution.error.reopenTask"), e));
        } finally {
          setBusy(false);
        }
      },
      [reopenComment, loadData, t],
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
        setError(woActionFailureText(t("execution.error.openDowntime"), e));
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
          setError(woActionFailureText(t("execution.error.closeDowntime"), e));
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
          setError(woActionFailureText(t("execution.error.releaseReservation"), e));
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
        setAddToolDialogError(woActionFailureText(t("execution.tools.addError"), e));
      } finally {
        setBusy(false);
      }
    }, [addToolLabel, addToolCode, wo.id, loadData, t]);

    const handleMarkToolUsed = useCallback(
      async (toolId: number) => {
        setBusy(true);
        setError(null);
        try {
          await markWoToolUsed(toolId);
          await loadData();
        } catch (e) {
          setError(woActionFailureText(t("execution.tools.updateError"), e));
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
          await markWoToolNotUsed(toolId);
          await loadData();
        } catch (e) {
          setError(woActionFailureText(t("execution.tools.updateError"), e));
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

    /** Lines cancelled while the work order was prepared: history only, out of every count. */
    const activeParts = useMemo(
      () => parts.filter((p) => p.consumption_status !== "cancelled"),
      [parts],
    );
    const cancelledParts = useMemo(
      () => parts.filter((p) => p.consumption_status === "cancelled"),
      [parts],
    );

    const plannedParts = useMemo(
      () => activeParts.filter((p) => !p.origin || p.origin === "planned"),
      [activeParts],
    );

    const actuallyUsedParts = useMemo(
      () =>
        parts.filter(
          (p) =>
            p.origin === "execution_added" ||
            p.consumption_status === "used" ||
            (p.quantity_used ?? 0) > 0,
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
        [...executionEvents].sort((a, b) => Date.parse(b.occurred_at) - Date.parse(a.occurred_at)),
      [executionEvents],
    );

    // ── Render ─────────────────────────────────────────────────────────────

    return (
      <div className="space-y-8">
        {error && (
          <div className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg">
            {error}
          </div>
        )}

        {!loggingEnabled && (
          <p className="text-sm text-muted-foreground">{t("execution.loggingOnlyInProgress")}</p>
        )}

        {wo.requires_permit && !["closed", "cancelled"].includes(statusCode) ? (
          <WoPermitGateCard
            workOrderId={wo.id}
            workOrderSiteId={wo.entity_id}
            refreshKey={rowVersion}
          />
        ) : null}

        {/* Delay form (pause / hold) */}
        {delayIntent && (
          <div className="space-y-3 rounded-md border border-tone-warning-border bg-tone-warning-bg p-3">
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
            {delayError && <div className="text-sm text-tone-danger-fg">{delayError}</div>}
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
              <h3 className="text-base font-semibold tracking-tight">
                {t("execution.sectionLabor")}
              </h3>
              <p className="text-sm text-muted-foreground">
                {t("execution.summaryLabor", {
                  count: laborEntries.length,
                  total: formatDurationMinutes(laborTotalMinutes, "auto", durationLabels),
                })}
              </p>
            </div>
            <Button size="sm" onClick={openLaborDialog} disabled={recordingDisabled || !actorId}>
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
                disabled={recordingDisabled || !actorId}
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
                const rateMissing = entry.hourly_rate == null || entry.rate_source === "missing";
                const lineCost =
                  entry.hours_worked != null && entry.hourly_rate != null
                    ? entry.hours_worked * entry.hourly_rate
                    : null;
                return (
                  <div
                    key={entry.id}
                    className="grid gap-2 border-b border-border/60 py-2 text-sm last:border-0 md:grid-cols-[1.2fr_1fr_1fr_90px_1fr]"
                  >
                    <div className="font-medium">
                      {formatPersonLabel(entry.intervener_display_name)}
                    </div>
                    <div className="text-muted-foreground">
                      {formatDateTime(entry.started_at, i18n.language)}
                    </div>
                    <div className="text-muted-foreground">
                      {formatDateTime(entry.ended_at, i18n.language)}
                    </div>
                    <div className="font-medium tabular-nums">{duration}</div>
                    <div className="text-xs text-muted-foreground">
                      {rateMissing ? (
                        <Badge variant="outline" className="text-[10px]">
                          {t("execution.rateMissing")}
                        </Badge>
                      ) : (
                        <>
                          {formatCurrency(entry.hourly_rate ?? 0)}
                          {lineCost != null ? (
                            <span className="ml-1">· {formatCurrency(lineCost)}</span>
                          ) : null}
                        </>
                      )}
                    </div>
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
                        {part.is_required === false ? <PartOptionalChip className="ml-1" /> : null}
                        {isNotUsed && part.not_used_reason_label && (
                          <p className="mt-0.5 text-xs text-muted-foreground">
                            {t("execution.notUsed.reason")}: {part.not_used_reason_label}
                            {part.not_used_comment && ` — ${part.not_used_comment}`}
                          </p>
                        )}
                      </div>
                      <div className="text-muted-foreground tabular-nums">
                        ×{part.quantity_planned}
                        {part.unit_symbol ? ` ${part.unit_symbol}` : ""}
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
                          <DecimalField
                            decimals={3}
                            min={0}
                            value={usageValue(partUsage[part.id], part.quantity_planned)}
                            onValueChange={(v) =>
                              setPartUsage((prev) => ({
                                ...prev,
                                [part.id]: v == null ? "" : String(v),
                              }))
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

          {cancelledParts.length > 0 && (
            <ul className="space-y-1" aria-label={t("execution.parts.cancelledTitle")}>
              {cancelledParts.map((part) => (
                <li
                  key={part.id}
                  className="flex flex-wrap items-center gap-2 rounded-md border border-dashed bg-muted/40 px-3 py-1.5 text-sm text-muted-foreground"
                >
                  <span className="flex-1 line-through">
                    {formatOrDash(part.article_ref ?? part.article_label)} ×{part.quantity_planned}
                    {part.unit_symbol ? ` ${part.unit_symbol}` : ""}
                  </span>
                  <Badge variant="outline" className="text-[10px]">
                    {t("planning.parts.cancelledStatus")}
                  </Badge>
                </li>
              ))}
            </ul>
          )}

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
                  onClick={() => setShowAddPartDialog(true)}
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
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-tone-success-border bg-tone-success-bg px-4 py-3 text-sm text-tone-success-fg">
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
                      {part.unit_symbol ? ` ${part.unit_symbol}` : ""}
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
                  {/* Releasing stock needs the stock management right (backend: inv.manage). */}
                  {can(P.INV_MANAGE) ? (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void handleReleaseReservation(reservation.id)}
                      disabled={controlsDisabled}
                    >
                      {t("execution.releaseReservation")}
                    </Button>
                  ) : null}
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
                    {task.is_mandatory ? (
                      <Badge variant="outline" className="ml-2 text-xs">
                        {t("execution.taskMandatory")}
                      </Badge>
                    ) : null}
                    {task.completed_at ? (
                      <span className="ml-2 text-xs text-muted-foreground">
                        {task.result_code === "cancelled"
                          ? t("execution.taskCancelled")
                          : t("execution.taskDone")}
                        {task.result_code
                          ? ` · ${t(`execution.taskResult.${task.result_code}`)}`
                          : ""}
                      </span>
                    ) : null}
                    {task.completed_at && task.finding_note ? (
                      <p className="text-xs text-muted-foreground">
                        {t("execution.taskFindingNote")} : {task.finding_note}
                      </p>
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
                    disabled={recordingDisabled || Boolean(task.completed_at)}
                  >
                    <SelectTrigger className="h-8">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {(task.is_mandatory
                        ? MANDATORY_TASK_RESULT_OPTIONS
                        : TASK_RESULT_OPTIONS
                      ).map((code) => (
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
                      disabled={recordingDisabled || !actorId}
                    >
                      {taskResultCodes[task.id] === "cancelled"
                        ? t("execution.cancelTask")
                        : t("execution.complete")}
                    </Button>
                  )}
                  {task.completed_at && reopenTaskId !== task.id && (
                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() => {
                        setReopenTaskId(task.id);
                        setReopenComment("");
                      }}
                      disabled={recordingDisabled}
                    >
                      {t("execution.reopenTask")}
                    </Button>
                  )}
                  {!task.completed_at && (taskResultCodes[task.id] ?? "ok") === "nok" && (
                    <div className="grid gap-2 md:col-span-3 md:grid-cols-2">
                      <Textarea
                        rows={2}
                        placeholder={t("execution.taskFindingNotePlaceholder")}
                        aria-label={t("execution.taskFindingNote")}
                        value={taskFindingNotes[task.id] ?? ""}
                        onChange={(e) =>
                          setTaskFindingNotes((prev) => ({ ...prev, [task.id]: e.target.value }))
                        }
                        disabled={recordingDisabled}
                      />
                      <Input
                        placeholder={t("execution.taskFollowUpPlaceholder")}
                        aria-label={t("execution.taskFollowUp")}
                        value={taskFollowUpCodes[task.id] ?? ""}
                        onChange={(e) =>
                          setTaskFollowUpCodes((prev) => ({ ...prev, [task.id]: e.target.value }))
                        }
                        disabled={recordingDisabled}
                      />
                      {task.is_mandatory ? (
                        <p className="text-xs text-muted-foreground md:col-span-2">
                          {t("execution.taskNokEvidenceHint")}
                        </p>
                      ) : null}
                    </div>
                  )}
                  {reopenTaskId === task.id && (
                    <div className="flex flex-wrap items-center gap-2 md:col-span-3">
                      <Input
                        className="min-w-64 flex-1"
                        placeholder={t("execution.reopenTaskCommentPlaceholder")}
                        aria-label={t("execution.reopenTaskComment")}
                        value={reopenComment}
                        onChange={(e) => setReopenComment(e.target.value)}
                        disabled={busy}
                      />
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void handleReopenTask(task)}
                        disabled={recordingDisabled || reopenComment.trim() === ""}
                      >
                        {t("execution.reopenTaskConfirm")}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => setReopenTaskId(null)}
                        disabled={busy}
                      >
                        {t("execution.reopenTaskCancel")}
                      </Button>
                    </div>
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
                    ? formatDurationMinutes(
                        hoursToMinutes(wo.planned_downtime_hours),
                        "auto",
                        durationLabels,
                      )
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
                  <SelectItem value="quality_loss">
                    {t("execution.downtime_quality_loss")}
                  </SelectItem>
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
                  <SelectItem value="__none__">
                    {t("execution.downtimeClassificationNone")}
                  </SelectItem>
                  <SelectItem value="mechanical">
                    {t("execution.downtimeClass.mechanical")}
                  </SelectItem>
                  <SelectItem value="electrical">
                    {t("execution.downtimeClass.electrical")}
                  </SelectItem>
                  <SelectItem value="waiting_spare">
                    {t("execution.downtimeClass.waiting_spare")}
                  </SelectItem>
                  <SelectItem value="waiting_approval">
                    {t("execution.downtimeClass.waiting_approval")}
                  </SelectItem>
                  <SelectItem value="operator_unavailable">
                    {t("execution.downtimeClass.operator_unavailable")}
                  </SelectItem>
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

          <Timeline
            items={downtimeTimeline}
            locale={i18n.language}
            aria-label={t("execution.sectionDowntime")}
            empty={
              <div className="rounded-md border border-dashed px-4 py-5 text-center text-sm text-muted-foreground">
                {t("execution.emptyDowntime")}
              </div>
            }
            getEntry={(seg) => {
              const duration = formatDurationMinutes(
                elapsedMinutesBetween(seg.started_at, seg.ended_at ?? nowIso()) ?? 0,
                "clock",
                durationLabels,
              );
              return {
                id: String(seg.id),
                title: t(`execution.downtime_${seg.downtime_type}`),
                timestamp: seg.started_at,
                subtitle: (
                  <>
                    {formatDateTime(seg.started_at, i18n.language)}
                    {" → "}
                    {seg.ended_at
                      ? formatDateTime(seg.ended_at, i18n.language)
                      : t("execution.downtimeOpen")}
                    {" · "}
                    {duration}
                  </>
                ),
                ...(seg.comment || seg.classification_code
                  ? {
                      description: (
                        <>
                          {seg.comment ? (
                            <p className="text-muted-foreground">{seg.comment}</p>
                          ) : null}
                          {seg.classification_code ? (
                            <p className="text-xs text-muted-foreground">
                              {t(`execution.downtimeClass.${seg.classification_code}`, {
                                defaultValue: seg.classification_code,
                              })}
                            </p>
                          ) : null}
                        </>
                      ),
                    }
                  : {}),
                action: !seg.ended_at ? (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => void handleCloseDowntime(seg.id)}
                    disabled={controlsDisabled}
                  >
                    {t("execution.closeDowntime")}
                  </Button>
                ) : null,
              };
            }}
          />
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
              disabled={recordingDisabled}
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
                        disabled={recordingDisabled}
                      >
                        {t("execution.tools.markUsed")}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 px-2 text-xs"
                        onClick={() => void handleMarkToolNotUsed(tool.id)}
                        disabled={recordingDisabled}
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
          <Timeline
            items={sortedEvents}
            locale={i18n.language}
            aria-label={t("execution.executionLog.title")}
            empty={t("execution.executionLog.empty")}
            getEntry={(ev) => {
              let params: Record<string, string> = {};
              try {
                if (ev.summary_params_json) {
                  params = JSON.parse(ev.summary_params_json) as Record<string, string>;
                }
              } catch {
                // non-fatal
              }
              return {
                id: String(ev.id),
                title: t(executionLogI18nKey(ev.summary_key), {
                  ...params,
                  defaultValue: ev.summary_key,
                }),
                timestamp: ev.occurred_at,
                ...(ev.actor_display_name ? { actor: ev.actor_display_name } : {}),
              };
            }}
          />
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
              {activeParts.filter((p) => (p.quantity_used ?? 0) > 0).length}/{activeParts.length}
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
              <DialogDescription>
                {t("execution.laborDialog.hint")} {t("execution.laborDialog.rateHint")}
              </DialogDescription>
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
                {people.failed ? (
                  <p className="text-xs text-text-muted">{t("woPeople.loadFailed")}</p>
                ) : !people.loading && people.options.length === 0 ? (
                  <p className="text-xs text-text-muted">{t("woPeople.none")}</p>
                ) : null}
              </div>
              {laborCheck &&
              (laborCheck.gaps.length > 0 ||
                laborCheck.warnings.length > 0 ||
                laborCheck.override_info != null) ? (
                <QualificationCheckNotice
                  check={laborCheck}
                  overrideContext={{ context_kind: "work_order", work_order_id: wo.id }}
                  onOverridden={setLaborCheck}
                />
              ) : null}
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  className="h-4 w-4"
                  checked={laborOvertime}
                  onChange={(e) => setLaborOvertime(e.target.checked)}
                />
                {t("execution.laborDialog.overtime")}
              </label>
              <div className="space-y-1">
                <Label>{t("execution.laborDialog.date")}</Label>
                <DateField value={laborDate} onChange={(v) => setLaborDate(v)} />
              </div>
              <div className="grid grid-cols-2 gap-3">
                <div className="space-y-1">
                  <Label>{t("execution.laborDialog.startTime")}</Label>
                  <TimeInput value={laborStart} onChange={(v) => setLaborStart(v)} />
                </div>
                <div className="space-y-1">
                  <Label>{t("execution.laborDialog.endTime")}</Label>
                  <TimeInput value={laborEnd} onChange={(v) => setLaborEnd(v)} />
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
                <Textarea
                  rows={2}
                  value={laborNotes}
                  onChange={(e) => setLaborNotes(e.target.value)}
                />
              </div>
              <p className="text-sm text-muted-foreground">
                {t("execution.laborDialog.workedPreview")}:{" "}
                <span className="font-medium text-foreground">
                  {laborPreviewMinutes != null
                    ? formatDurationMinutes(laborPreviewMinutes, "auto", durationLabels)
                    : "—"}
                </span>
              </p>
              {laborFormError && <p className="text-sm text-tone-danger-fg">{laborFormError}</p>}
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setShowLaborDialog(false)}>
                {t("execution.cancel")}
              </Button>
              <Button
                onClick={() => void submitLaborDialog()}
                disabled={busy || !actorId || (laborCheck != null && !laborCheck.qualified)}
              >
                {t("execution.laborDialog.save")}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Not-used dialog */}
        <Dialog
          open={notUsedPartId != null}
          onOpenChange={(open) => !open && setNotUsedPartId(null)}
        >
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
              {notUsedError && <p className="text-sm text-tone-danger-fg">{notUsedError}</p>}
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

        {/* Add part during execution (stock article or explicit non-stock item) */}
        <WoAddExecutionPartDialog
          woId={wo.id}
          open={showAddPartDialog}
          onOpenChange={setShowAddPartDialog}
          onAdded={loadData}
        />

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
                <p className="text-sm text-tone-danger-fg">{addToolDialogError}</p>
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
