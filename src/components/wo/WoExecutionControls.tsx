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
import {
  addLabor,
  completeTask,
  confirmNoParts,
  holdWo,
  listDelaySegments,
  listDowntimeSegments,
  listLabor,
  listParts,
  listTasks,
  pauseWo,
  recordPartUsage,
  resumeWo,
  startWo,
  closeLabor,
  type TaskResultCode,
  type WoDelaySegment,
  type WoIntervener,
  type WoPart,
  type WoTask,
} from "@/services/wo-execution-service";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

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

function formatDateTime(value: string | null): string {
  if (!value) return "-";
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString();
}

function toErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return fallback;
}

const EXECUTION_EDITABLE_STATUSES = new Set([
  "assigned",
  "waiting_for_prerequisite",
  "in_progress",
  "paused",
]);

const TASK_RESULT_OPTIONS: TaskResultCode[] = ["ok", "nok", "na", "deferred"];

export function WoExecutionControls({ wo, canEdit }: WoExecutionControlsProps) {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const refreshActiveWo = useWoStore((s) => s.refreshActiveWo);
  const openCompletionDialog = useWoStore((s) => s.openCompletionDialog);

  const [statusCode, setStatusCode] = useState(wo.status_code ?? "draft");
  const [rowVersion, setRowVersion] = useState(wo.row_version);

  const [laborEntries, setLaborEntries] = useState<WoIntervener[]>([]);
  const [parts, setParts] = useState<WoPart[]>([]);
  const [tasks, setTasks] = useState<WoTask[]>([]);
  const [, setDelaySegments] = useState<WoDelaySegment[]>([]);

  const [reasonOptions, setReasonOptions] = useState<DelayReasonOption[]>([]);

  const [partUsage, setPartUsage] = useState<Record<number, string>>({});
  const [taskResultCodes, setTaskResultCodes] = useState<Record<number, TaskResultCode>>({});

  const [delayIntent, setDelayIntent] = useState<DelayIntent | null>(null);
  const [delayReasonId, setDelayReasonId] = useState("");
  const [delayComment, setDelayComment] = useState("");
  const [delayError, setDelayError] = useState<string | null>(null);

  const [intervenerIdInput, setIntervenerIdInput] = useState("");
  const [manualHours, setManualHours] = useState("");

  const [busy, setBusy] = useState(false);
  const [, setLoading] = useState(false);
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
    setLoading(true);
    try {
      const [laborRows, partRows, taskRows, delayRows, downtimeRows, lookupReasons] =
        await Promise.all([
          listLabor(wo.id),
          listParts(wo.id).catch(() => []),
          listTasks(wo.id).catch(() => []),
          listDelaySegments(wo.id).catch(() => []),
          listDowntimeSegments(wo.id).catch(() => []),
          getLookupValues("delay_reason_codes").catch(() => []),
        ]);

      setLaborEntries(laborRows);
      setParts(partRows);
      setTasks(taskRows);
      setDelaySegments(delayRows);

      const usageSeed: Record<number, string> = {};
      partRows.forEach((part) => {
        usageSeed[part.id] = part.quantity_used != null ? String(part.quantity_used) : "";
      });
      setPartUsage(usageSeed);

      const fromLookup: DelayReasonOption[] = lookupReasons.map((r) => ({
        id: r.id,
        label: r.label,
      }));

      const knownIds = new Set(fromLookup.map((r) => r.id));
      const fromHistory: DelayReasonOption[] = delayRows
        .filter((row) => row.delay_reason_id != null)
        .map((row) => row.delay_reason_id as number)
        .filter((id, idx, arr) => arr.indexOf(id) === idx)
        .filter((id) => !knownIds.has(id))
        .map((id) => ({ id, label: `Reason #${id}` }));

      const mergedReasons = [...fromLookup, ...fromHistory];
      const fallbackReasons = Array.from({ length: 10 }, (_, idx) => ({
        id: idx + 1,
        label: `Reason #${idx + 1}`,
      }));

      setReasonOptions(mergedReasons.length > 0 ? mergedReasons : fallbackReasons);

      if (downtimeRows.length > 0) {
        void downtimeRows;
      }
    } catch (e) {
      setError(toErrorMessage(e, "Unable to load execution data."));
    } finally {
      setLoading(false);
    }
  }, [wo.id]);

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
      setError(toErrorMessage(e, "Unable to start work order."));
    } finally {
      setBusy(false);
    }
  }, [actorId, wo.id, rowVersion, handleRefreshState]);

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
      setError(toErrorMessage(e, "Unable to resume work order."));
    } finally {
      setBusy(false);
    }
  }, [actorId, wo.id, rowVersion, handleRefreshState]);

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
      setError(toErrorMessage(e, "Unable to submit delay action."));
    } finally {
      setBusy(false);
    }
  }, [actorId, delayIntent, delayReasonId, delayComment, wo.id, rowVersion, handleRefreshState, t]);

  const handleStartLabor = useCallback(async () => {
    if (!actorId) return;
    const intervenerId = Number(intervenerIdInput);
    if (!Number.isFinite(intervenerId) || intervenerId <= 0) {
      setError("Intervener ID is required.");
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
      setError(toErrorMessage(e, "Unable to start labor entry."));
    } finally {
      setBusy(false);
    }
  }, [actorId, intervenerIdInput, wo.id, loadData]);

  const handleStopLabor = useCallback(
    async (entryId: number) => {
      if (!actorId) return;
      setBusy(true);
      setError(null);
      try {
        await closeLabor(entryId, nowIso(), actorId);
        await loadData();
      } catch (e) {
        setError(toErrorMessage(e, "Unable to close labor entry."));
      } finally {
        setBusy(false);
      }
    },
    [actorId, loadData],
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
      setError("Intervener ID and manual hours must be valid.");
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
      setError(toErrorMessage(e, "Unable to add manual labor entry."));
    } finally {
      setBusy(false);
    }
  }, [actorId, intervenerIdInput, manualHours, wo.id, loadData]);

  const handleRecordPartUsage = useCallback(
    async (partId: number) => {
      const value = Number(partUsage[partId] ?? "");
      if (!Number.isFinite(value) || value < 0) {
        setError("Quantity used must be a valid non-negative number.");
        return;
      }

      setBusy(true);
      setError(null);
      try {
        await recordPartUsage(partId, value);
        await loadData();
      } catch (e) {
        setError(toErrorMessage(e, "Unable to record part usage."));
      } finally {
        setBusy(false);
      }
    },
    [partUsage, loadData],
  );

  const handleNoParts = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await confirmNoParts(wo.id);
      await loadData();
    } catch (e) {
      setError(toErrorMessage(e, "Unable to confirm no parts used."));
    } finally {
      setBusy(false);
    }
  }, [wo.id, loadData]);

  const handleCompleteTask = useCallback(
    async (task: WoTask) => {
      if (!actorId) return;
      const resultCode = taskResultCodes[task.id] ?? "ok";
      setBusy(true);
      setError(null);
      try {
        await completeTask(task.id, actorId, resultCode);
        await loadData();
      } catch (e) {
        setError(toErrorMessage(e, "Unable to complete task."));
      } finally {
        setBusy(false);
      }
    },
    [actorId, taskResultCodes, loadData],
  );

  const openLaborEntries = useMemo(
    () => laborEntries.filter((row) => !row.ended_at),
    [laborEntries],
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
              <div className="text-sm">Start: {formatDateTime(entry.started_at)}</div>
              <div className="text-sm">End: {formatDateTime(entry.ended_at)}</div>
              <div className="text-sm">Hours: {entry.hours_worked ?? "-"}</div>
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
              <div className="text-sm">{part.article_ref || `Part #${part.id}`}</div>
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
    </div>
  );
}
