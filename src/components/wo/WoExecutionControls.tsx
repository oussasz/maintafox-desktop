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
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { getLookupValues } from "@/services/lookup-service";
import { listInventoryReservations, releaseInventoryReservation } from "@/services/inventory-service";
import {
  addLabor,
  closeDowntime,
  completeTask,
  confirmNoParts,
  listDowntimeSegments,
  listLabor,
  listParts,
  listTasks,
  openDowntime,
  recordPartUsage,
  closeLabor,
  type TaskResultCode,
  type WoDowntimeSegment,
  type WoIntervener,
} from "@/services/wo-execution-service";
import { holdWo, pauseWo, resumeWo, startWo } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { formatDateTime } from "@/utils/format-date";
import type { DowntimeType, WoExecPart, WoExecTask } from "@shared/ipc-types";
import type { StockReservation, WorkOrder } from "@shared/ipc-types";

interface WoExecutionControlsProps {
  wo: WorkOrder;
  canEdit: boolean;
}

type DelayIntent = "pause" | "hold";

interface DelayReasonOption {
  id: number;
  label: string;
}

function nowIso(): string {
  return new Date().toISOString();
}

const EXECUTION_EDITABLE_STATUSES = new Set([
  "assigned",
  "waiting_for_prerequisite",
  "in_progress",
  "paused",
]);

const TASK_RESULT_OPTIONS: TaskResultCode[] = ["ok", "nok", "na", "deferred"];

export function WoExecutionControls({ wo, canEdit }: WoExecutionControlsProps) {
  const { t, i18n } = useTranslation("ot");
  const { info } = useSession();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const openCompletionDialog = useWoStore((s) => s.openCompletionDialog);

  const [statusCode, setStatusCode] = useState(wo.status_code ?? "draft");
  const [rowVersion, setRowVersion] = useState(wo.row_version);

  const [laborEntries, setLaborEntries] = useState<WoIntervener[]>([]);
  const [parts, setParts] = useState<WoExecPart[]>([]);
  const [tasks, setTasks] = useState<WoExecTask[]>([]);
  const [downtimeSegments, setDowntimeSegments] = useState<WoDowntimeSegment[]>([]);
  const [reservations, setReservations] = useState<StockReservation[]>([]);

  const [reasonOptions, setReasonOptions] = useState<DelayReasonOption[]>([]);

  const [partUsage, setPartUsage] = useState<Record<number, string>>({});
  const [taskResultCodes, setTaskResultCodes] = useState<Record<number, TaskResultCode>>({});

  const [delayIntent, setDelayIntent] = useState<DelayIntent | null>(null);
  const [delayReasonId, setDelayReasonId] = useState("");
  const [delayComment, setDelayComment] = useState("");
  const [delayError, setDelayError] = useState<string | null>(null);

  const [intervenerIdInput, setIntervenerIdInput] = useState("");
  const [manualHours, setManualHours] = useState("");

  const [downtimeType, setDowntimeType] = useState<DowntimeType>("full");
  const [downtimeComment, setDowntimeComment] = useState("");

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setStatusCode(wo.status_code ?? "draft");
    setRowVersion(wo.row_version);
  }, [wo]);

  useEffect(() => {
    if (!intervenerIdInput && (wo.primary_responsible_id || info?.user_id)) {
      setIntervenerIdInput(String(wo.primary_responsible_id ?? info?.user_id ?? ""));
    }
  }, [intervenerIdInput, wo.primary_responsible_id, info?.user_id]);

  const loadData = useCallback(async () => {
    try {
      const [laborRows, partRows, taskRows, downtimeRows, lookupReasons] = await Promise.all([
        listLabor(wo.id),
        listParts(wo.id).catch(() => []),
        listTasks(wo.id).catch(() => []),
        listDowntimeSegments(wo.id).catch(() => []),
        getLookupValues("delay_reason_codes").catch(() => []),
      ]);
      const woReservations = await listInventoryReservations({
        source_id: wo.id,
        include_inactive: true,
      }).catch(() => []);

      setLaborEntries(laborRows);
      setParts(partRows);
      setTasks(taskRows);
      setDowntimeSegments(downtimeRows);
      setReservations(woReservations);

      const usageSeed: Record<number, string> = {};
      partRows.forEach((part) => {
        usageSeed[part.id] = part.quantity_used != null ? String(part.quantity_used) : "";
      });
      setPartUsage(usageSeed);

      const fromLookup: DelayReasonOption[] = lookupReasons.map((r) => ({
        id: r.id,
        label: r.label,
      }));

      const fallbackReasons = Array.from({ length: 10 }, (_, idx) => ({
        id: idx + 1,
        label: t("execution.reasonFallback", { id: idx + 1 }),
      }));

      setReasonOptions(fromLookup.length > 0 ? fromLookup : fallbackReasons);
    } catch (e) {
      setError(t("execution.error.loadData"));
      console.error("loadData", e);
    }
  }, [wo.id, t]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  const actorId = info?.user_id ?? null;
  const controlsDisabled = !canEdit || !EXECUTION_EDITABLE_STATUSES.has(statusCode) || busy;

  const handleRefreshState = useCallback(
    async (next: WorkOrder) => {
      setRowVersion(next.row_version);
      setStatusCode(next.status_code ?? statusCode);
      await refreshActiveWo();
      await loadData();
    },
    [refreshActiveWo, loadData, statusCode],
  );

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
    setDelayReasonId("");
    setDelayComment("");
    setDelayError(null);
  }, []);

  const submitDelayAction = useCallback(async () => {
    if (!actorId || !delayIntent) return;

    if (!delayReasonId) {
      setDelayError(t("execution.delayReasonRequired"));
      return;
    }

    setBusy(true);
    setError(null);
    setDelayError(null);
    try {
      const input = {
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: rowVersion,
        delay_reason_id: Number(delayReasonId),
        comment: delayComment.trim() ? delayComment.trim() : null,
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

  const handleStartLabor = useCallback(async () => {
    if (!actorId) return;
    const intervenerId = Number(intervenerIdInput);
    if (!Number.isFinite(intervenerId) || intervenerId <= 0) {
      setError(t("execution.error.intervenerId"));
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await addLabor({
        wo_id: wo.id,
        intervener_id: intervenerId,
        started_at: nowIso(),
      });
      await loadData();
    } catch (e) {
      setError(t("execution.error.startLabor"));
      console.error("handleStartLabor", e);
    } finally {
      setBusy(false);
    }
  }, [actorId, intervenerIdInput, wo.id, loadData, t]);

  const handleStopLabor = useCallback(
    async (entryId: number) => {
      if (!actorId) return;
      setBusy(true);
      setError(null);
      try {
        await closeLabor(entryId, nowIso(), actorId);
        await loadData();
      } catch (e) {
        setError(t("execution.error.closeLabor"));
        console.error("handleStopLabor", e);
      } finally {
        setBusy(false);
      }
    },
    [actorId, loadData, t],
  );

  const handleManualLabor = useCallback(async () => {
    if (!actorId) return;
    const intervenerId = Number(intervenerIdInput);
    const hours = Number(manualHours);

    if (
      !Number.isFinite(intervenerId) ||
      intervenerId <= 0 ||
      !Number.isFinite(hours) ||
      hours <= 0
    ) {
      setError(t("execution.error.invalidManualEntry"));
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await addLabor({
        wo_id: wo.id,
        intervener_id: intervenerId,
        hours_worked: hours,
      });
      setManualHours("");
      await loadData();
    } catch (e) {
      setError(t("execution.error.addManualLabor"));
      console.error("handleManualLabor", e);
    } finally {
      setBusy(false);
    }
  }, [actorId, intervenerIdInput, manualHours, wo.id, loadData, t]);

  const handleRecordPartUsage = useCallback(
    async (partId: number) => {
      const value = Number(partUsage[partId] ?? "");
      if (!Number.isFinite(value) || value < 0) {
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
      await loadData();
    } catch (e) {
      setError(t("execution.error.confirmNoParts"));
      console.error("handleNoParts", e);
    } finally {
      setBusy(false);
    }
  }, [wo.id, loadData, t]);

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

  const openLaborEntries = useMemo(
    () => laborEntries.filter((row) => !row.ended_at),
    [laborEntries],
  );

  const handleOpenDowntime = useCallback(async () => {
    if (!actorId) return;
    setBusy(true);
    setError(null);
    try {
      await openDowntime(wo.id, downtimeType, actorId, downtimeComment.trim() || null);
      setDowntimeComment("");
      await loadData();
    } catch (e) {
      setError(t("execution.error.openDowntime"));
      console.error("handleOpenDowntime", e);
    } finally {
      setBusy(false);
    }
  }, [actorId, wo.id, downtimeType, downtimeComment, loadData, t]);

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

  const openDowntimeSegments = useMemo(
    () => downtimeSegments.filter((row) => !row.ended_at),
    [downtimeSegments],
  );

  const handleReleaseReservation = useCallback(
    async (reservationId: number) => {
      setBusy(true);
      setError(null);
      try {
        await releaseInventoryReservation({ reservation_id: reservationId });
        await loadData();
      } catch (e) {
        setError("Failed to release reservation.");
        console.error("handleReleaseReservation", e);
      } finally {
        setBusy(false);
      }
    },
    [loadData],
  );

  return (
    <div className="space-y-4">
      {error && (
        <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
          {error}
        </div>
      )}

      <section className="space-y-2">
        <h3 className="text-sm font-semibold">{t("execution.controls")}</h3>
        <div className="flex flex-wrap items-center gap-2">
          {(statusCode === "assigned" || statusCode === "waiting_for_prerequisite") && (
            <>
              <Button onClick={() => void handleStart()} disabled={controlsDisabled || !actorId}>
                {t("execution.start")}
              </Button>
              <Button
                variant="outline"
                onClick={() => openDelayForm("hold")}
                disabled={controlsDisabled || !actorId}
              >
                {t("execution.hold")}
              </Button>
            </>
          )}

          {statusCode === "in_progress" && (
            <>
              <Button
                variant="outline"
                onClick={() => openDelayForm("pause")}
                disabled={controlsDisabled || !actorId}
              >
                {t("execution.pause")}
              </Button>
              <Button onClick={openCompletionDialog} disabled={controlsDisabled || !actorId}>
                {t("execution.completeMech")}
              </Button>
            </>
          )}

          {statusCode === "paused" && (
            <Button onClick={() => void handleResume()} disabled={controlsDisabled || !actorId}>
              {t("execution.resume")}
            </Button>
          )}

          {statusCode === "mechanically_complete" && (
            <span className="text-sm text-muted-foreground">{t("execution.noActions")}</span>
          )}
        </div>

        {delayIntent && (
          <div className="rounded-md border p-3">
            <div className="mb-2 text-sm font-medium">
              {delayIntent === "pause" ? t("execution.pauseTitle") : t("execution.holdTitle")}
            </div>
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <Label>{t("execution.delayReason")}</Label>
                <Select value={delayReasonId} onValueChange={setDelayReasonId}>
                  <SelectTrigger>
                    <SelectValue placeholder={t("execution.selectReason")} />
                  </SelectTrigger>
                  <SelectContent>
                    {reasonOptions.map((reason) => (
                      <SelectItem key={reason.id} value={String(reason.id)}>
                        {reason.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
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
            {delayError && <div className="mt-2 text-sm text-red-700">{delayError}</div>}
            <div className="mt-3 flex items-center gap-2">
              <Button onClick={() => void submitDelayAction()} disabled={busy || !actorId}>
                {t("execution.submit")}
              </Button>
              <Button variant="outline" onClick={() => setDelayIntent(null)}>
                {t("execution.cancel")}
              </Button>
            </div>
          </div>
        )}
      </section>

      <details open>
        <summary className="cursor-pointer text-sm font-semibold">
          {t("execution.laborEntries")}
        </summary>
        <div className="mt-3 space-y-3 rounded-md border p-3">
          <div className="grid gap-2 md:grid-cols-[180px_140px_auto_auto]">
            <Input
              type="number"
              placeholder={t("execution.intervenerId")}
              value={intervenerIdInput}
              onChange={(e) => setIntervenerIdInput(e.target.value)}
              disabled={controlsDisabled}
            />
            <Input
              type="number"
              placeholder={t("execution.manualHours")}
              value={manualHours}
              onChange={(e) => setManualHours(e.target.value)}
              disabled={controlsDisabled}
            />
            <Button
              variant="outline"
              onClick={() => void handleStartLabor()}
              disabled={controlsDisabled || !actorId}
            >
              {t("execution.startLabor")}
            </Button>
            <Button
              variant="outline"
              onClick={() => void handleManualLabor()}
              disabled={controlsDisabled || !actorId}
            >
              {t("execution.addManualHours")}
            </Button>
          </div>

          {openLaborEntries.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("execution.noOpenLabor")}</div>
          )}

          {laborEntries.map((entry) => (
            <div
              key={entry.id}
              className="grid gap-2 rounded-md border p-2 md:grid-cols-[120px_1fr_1fr_120px_auto]"
            >
              <div className="text-sm font-medium">#{entry.intervener_id}</div>
              <div className="text-sm">
                {t("execution.laborStart")}: {formatDateTime(entry.started_at, i18n.language)}
              </div>
              <div className="text-sm">
                {t("execution.laborEnd")}: {formatDateTime(entry.ended_at, i18n.language)}
              </div>
              <div className="text-sm">
                {t("execution.laborHours")}: {entry.hours_worked ?? "-"}
              </div>
              {!entry.ended_at && (
                <Button
                  size="sm"
                  onClick={() => void handleStopLabor(entry.id)}
                  disabled={controlsDisabled || !actorId}
                >
                  {t("execution.stop")}
                </Button>
              )}
            </div>
          ))}
        </div>
      </details>

      <details open>
        <summary className="cursor-pointer text-sm font-semibold">
          {t("execution.partsUsed")}
        </summary>
        <div className="mt-3 space-y-2 rounded-md border p-3">
          {parts.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("execution.noPlannedParts")}</div>
          )}
          {parts.map((part) => (
            <div
              key={part.id}
              className="grid gap-2 rounded-md border p-2 md:grid-cols-[2fr_130px_130px_auto]"
            >
              <div className="text-sm">
                {part.article_ref || (part.article_id != null ? `Article #${part.article_id}` : `Part #${part.id}`)}
              </div>
              <div className="text-sm">
                {t("execution.planned")}: {part.quantity_planned}
              </div>
              <Input
                type="number"
                min="0"
                step="0.01"
                value={partUsage[part.id] ?? ""}
                onChange={(e) => setPartUsage((prev) => ({ ...prev, [part.id]: e.target.value }))}
                disabled={controlsDisabled}
              />
              <Button
                size="sm"
                onClick={() => void handleRecordPartUsage(part.id)}
                disabled={controlsDisabled}
              >
                {t("execution.saveUsage")}
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            onClick={() => void handleNoParts()}
            disabled={controlsDisabled}
          >
            {t("execution.noPartsUsed")}
          </Button>
        </div>
      </details>

      <details open>
        <summary className="cursor-pointer text-sm font-semibold">Stock reservations</summary>
        <div className="mt-3 space-y-2 rounded-md border p-3">
          {reservations.length === 0 && (
            <div className="text-sm text-muted-foreground">
              No reservation linked to this WO yet.
            </div>
          )}
          {reservations.map((reservation) => (
            <div
              key={reservation.id}
              className="grid gap-2 rounded-md border p-2 md:grid-cols-[2fr_110px_110px_140px_auto]"
            >
              <div className="text-sm">
                <div className="font-medium">
                  {reservation.article_code} - {reservation.article_name}
                </div>
                <div className="text-xs text-muted-foreground">
                  {reservation.warehouse_code}/{reservation.location_code}
                </div>
              </div>
              <div className="text-sm">
                Reserved: {reservation.quantity_reserved}
              </div>
              <div className="text-sm">
                Issued: {reservation.quantity_issued}
              </div>
              <div className="text-sm capitalize">{reservation.status}</div>
              <Button
                size="sm"
                variant="outline"
                disabled={busy || reservation.status === "released" || !canEdit}
                onClick={() => void handleReleaseReservation(reservation.id)}
              >
                Release
              </Button>
            </div>
          ))}
        </div>
      </details>

      <details open>
        <summary className="cursor-pointer text-sm font-semibold">
          {t("execution.taskExecution")}
        </summary>
        <div className="mt-3 space-y-2 rounded-md border p-3">
          {tasks.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("execution.noTasks")}</div>
          )}
          {tasks.map((task) => {
            const incompleteMandatory = task.is_mandatory && !task.is_completed;
            return (
              <div
                key={task.id}
                className={`grid gap-2 rounded-md border p-2 md:grid-cols-[30px_1fr_130px_150px_auto] ${
                  incompleteMandatory ? "border-red-300 bg-red-50" : ""
                }`}
              >
                <input
                  type="checkbox"
                  checked={task.is_completed}
                  readOnly
                  className="mt-1 h-4 w-4"
                />
                <div className="text-sm">
                  {task.task_description}
                  {task.is_mandatory && (
                    <span className="ml-2 text-xs font-semibold text-red-700">
                      {t("execution.mandatory")}
                    </span>
                  )}
                </div>
                <div className="text-xs text-muted-foreground">
                  {t("execution.result")}: {task.result_code ?? "-"}
                </div>
                <Select
                  value={taskResultCodes[task.id] ?? "ok"}
                  onValueChange={(value) =>
                    setTaskResultCodes((prev) => ({ ...prev, [task.id]: value as TaskResultCode }))
                  }
                  disabled={controlsDisabled || task.is_completed}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {TASK_RESULT_OPTIONS.map((option) => (
                      <SelectItem key={option} value={option}>
                        {option}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => void handleCompleteTask(task)}
                  disabled={controlsDisabled || task.is_completed || !actorId}
                >
                  {t("execution.complete")}
                </Button>
              </div>
            );
          })}
        </div>
      </details>
      <details open>
        <summary className="cursor-pointer text-sm font-semibold">
          {t("execution.downtimeTracking")}
        </summary>
        <div className="mt-3 space-y-3 rounded-md border p-3">
          <div className="grid gap-2 md:grid-cols-[180px_1fr_auto]">
            <Select
              value={downtimeType}
              onValueChange={(v) => setDowntimeType(v as DowntimeType)}
              disabled={controlsDisabled}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="full">{t("execution.downtimeFull")}</SelectItem>
                <SelectItem value="partial">{t("execution.downtimePartial")}</SelectItem>
                <SelectItem value="standby">{t("execution.downtimeStandby")}</SelectItem>
                <SelectItem value="quality_loss">{t("execution.downtimeQualityLoss")}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              placeholder={t("execution.downtimeComment")}
              value={downtimeComment}
              onChange={(e) => setDowntimeComment(e.target.value)}
              disabled={controlsDisabled}
            />
            <Button
              variant="outline"
              onClick={() => void handleOpenDowntime()}
              disabled={controlsDisabled || !actorId || openDowntimeSegments.length > 0}
            >
              {t("execution.openDowntime")}
            </Button>
          </div>

          {downtimeSegments.length === 0 && (
            <div className="text-sm text-muted-foreground">{t("execution.noDowntime")}</div>
          )}

          {downtimeSegments.map((seg) => (
            <div
              key={seg.id}
              className="grid gap-2 rounded-md border p-2 md:grid-cols-[100px_1fr_1fr_1fr_auto]"
            >
              <div className="text-xs font-medium">
                {t(`execution.downtime_${seg.downtime_type}`)}
              </div>
              <div className="text-xs">
                {t("execution.laborStart")}: {formatDateTime(seg.started_at, i18n.language)}
              </div>
              <div className="text-xs">
                {t("execution.laborEnd")}: {formatDateTime(seg.ended_at, i18n.language)}
              </div>
              <div className="text-xs text-muted-foreground">{seg.comment ?? "-"}</div>
              {!seg.ended_at && (
                <Button
                  size="sm"
                  onClick={() => void handleCloseDowntime(seg.id)}
                  disabled={controlsDisabled}
                >
                  {t("execution.closeDowntime")}
                </Button>
              )}
            </div>
          ))}
        </div>
      </details>
    </div>
  );
}
