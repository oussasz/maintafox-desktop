/**
 * WoExecutionControls.tsx
 *
 * Execution sub-panel for WoDetailDialog "Execution" tab.
 * Shows: labor entries with start/stop timers, task checklist,
 * and parts usage tracking.
 *
 * Phase 2 – Sub-phase 05 – File 02 – Sprint S4.
 */

import { CheckSquare, Clock, Package, Pause, Play, Plus, Square, UserCog } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { addLabor, closeLabor, completeTask, listLabor, listTasks } from "@/services/wo-service";
import type { WoLaborEntry, WoTask, WorkOrder } from "@shared/ipc-types";

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatHours(hours: number | null): string {
  if (hours == null) return "—";
  return `${hours.toFixed(1)}h`;
}

function formatDatetime(iso: string | null): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString("fr-FR", {
      day: "2-digit",
      month: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

// ── Component ───────────────────────────────────────────────────────────────

interface WoExecutionControlsProps {
  wo: WorkOrder;
}

export function WoExecutionControls({ wo }: WoExecutionControlsProps) {
  const { t } = useTranslation("ot");

  const [laborEntries, setLaborEntries] = useState<WoLaborEntry[]>([]);
  const [tasks, setTasks] = useState<WoTask[]>([]);
  const [laborLoading, setLaborLoading] = useState(false);
  const [tasksLoading, setTasksLoading] = useState(false);

  // Manual labor fields
  const [manualHours, setManualHours] = useState("");

  // ── Load labor + tasks ──────────────────────────────────────────────

  const loadLabor = useCallback(async () => {
    setLaborLoading(true);
    try {
      const entries = await listLabor(wo.id);
      setLaborEntries(entries);
    } catch {
      // Silently handle — IPC may not yet be wired
    } finally {
      setLaborLoading(false);
    }
  }, [wo.id]);

  const loadTasks = useCallback(async () => {
    setTasksLoading(true);
    try {
      const items = await listTasks(wo.id);
      setTasks(items);
    } catch {
      // Silently handle
    } finally {
      setTasksLoading(false);
    }
  }, [wo.id]);

  useEffect(() => {
    void loadLabor();
    void loadTasks();
  }, [loadLabor, loadTasks]);

  // ── Labor start/stop handlers ───────────────────────────────────────

  const handleStartLabor = useCallback(
    async (intervenerId: number) => {
      await addLabor({
        work_order_id: wo.id,
        intervener_id: intervenerId,
        started_at: new Date().toISOString(),
      });
      void loadLabor();
    },
    [wo.id, loadLabor],
  );

  const handleStopLabor = useCallback(
    async (entry: WoLaborEntry) => {
      await closeLabor({
        id: entry.id,
        ended_at: new Date().toISOString(),
        hours_worked: manualHours ? Number(manualHours) : null,
      });
      setManualHours("");
      void loadLabor();
    },
    [loadLabor, manualHours],
  );

  // ── Task completion handler ─────────────────────────────────────────

  const handleCompleteTask = useCallback(
    async (taskId: number) => {
      await completeTask(taskId);
      void loadTasks();
    },
    [loadTasks],
  );

  // ── Derived ─────────────────────────────────────────────────────────

  const openLaborEntries = laborEntries.filter((e) => e.started_at && !e.ended_at);
  const closedLaborEntries = laborEntries.filter((e) => e.ended_at);
  const completedTasks = tasks.filter((t) => t.is_completed);
  const totalTasks = tasks.length;

  return (
    <div className="space-y-4">
      {/* ── Labor section ─────────────────────────────────────────── */}
      <Card>
        <CardHeader className="px-4 py-3">
          <CardTitle className="text-sm flex items-center gap-2">
            <UserCog className="h-4 w-4" />
            {t("detail.sections.labor")}
            <Badge variant="secondary" className="text-[10px]">
              {laborEntries.length}
            </Badge>
          </CardTitle>
        </CardHeader>
        <CardContent className="px-4 pb-3">
          {laborLoading ? (
            <p className="text-xs text-muted-foreground">{t("empty.noLabor")}</p>
          ) : laborEntries.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("empty.noLabor")}</p>
          ) : (
            <div className="space-y-2">
              {/* Active labor (in progress) */}
              {openLaborEntries.map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-center gap-3 rounded-md border border-amber-200 bg-amber-50 p-2.5 text-sm"
                >
                  <Play className="h-3.5 w-3.5 text-amber-600 shrink-0" />
                  <div className="flex-1 min-w-0">
                    <span className="font-medium">
                      {entry.intervener_name ?? `#${entry.intervener_id}`}
                    </span>
                    {entry.skill && (
                      <span className="text-muted-foreground text-xs ml-1.5">({entry.skill})</span>
                    )}
                    <span className="text-xs text-muted-foreground ml-2">
                      {t("execution.startedAt")}: {formatDatetime(entry.started_at)}
                    </span>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    <Input
                      type="number"
                      min={0}
                      step={0.5}
                      value={manualHours}
                      onChange={(e) => setManualHours(e.target.value)}
                      placeholder="h"
                      className="h-7 w-16 text-xs"
                    />
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 gap-1 text-xs border-red-300 text-red-700 hover:bg-red-50"
                      onClick={() => void handleStopLabor(entry)}
                    >
                      <Pause className="h-3 w-3" />
                      {t("execution.stop")}
                    </Button>
                  </div>
                </div>
              ))}

              {/* Closed labor entries */}
              {closedLaborEntries.map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-center gap-3 rounded-md border p-2.5 text-sm"
                >
                  <Clock className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                  <div className="flex-1 min-w-0">
                    <span className="font-medium">
                      {entry.intervener_name ?? `#${entry.intervener_id}`}
                    </span>
                    {entry.skill && (
                      <span className="text-muted-foreground text-xs ml-1.5">({entry.skill})</span>
                    )}
                  </div>
                  <div className="flex items-center gap-3 text-xs text-muted-foreground shrink-0">
                    <span>
                      {formatDatetime(entry.started_at)} → {formatDatetime(entry.ended_at)}
                    </span>
                    <Badge variant="secondary" className="text-[10px]">
                      {formatHours(entry.hours_worked)}
                    </Badge>
                    {entry.hourly_rate != null && (
                      <span className="tabular-nums">{entry.hourly_rate}€/h</span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Quick add labor button */}
          {wo.status_code === "in_progress" && (
            <div className="pt-2">
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-xs"
                onClick={() => void handleStartLabor(wo.primary_responsible_id ?? 0)}
              >
                <Plus className="h-3 w-3" />
                {t("action.addLabor")}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── Tasks section ─────────────────────────────────────────── */}
      <Card>
        <CardHeader className="px-4 py-3">
          <CardTitle className="text-sm flex items-center gap-2">
            <CheckSquare className="h-4 w-4" />
            {t("execution.tasks")}
            {totalTasks > 0 && (
              <Badge variant="secondary" className="text-[10px]">
                {completedTasks.length}/{totalTasks}
              </Badge>
            )}
          </CardTitle>
        </CardHeader>
        <CardContent className="px-4 pb-3">
          {tasksLoading ? (
            <p className="text-xs text-muted-foreground">…</p>
          ) : tasks.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("execution.noTasks")}</p>
          ) : (
            <div className="space-y-1.5">
              {tasks.map((task) => (
                <div key={task.id} className="flex items-center gap-2 text-sm">
                  <button
                    type="button"
                    className="shrink-0"
                    disabled={task.is_completed || wo.status_code !== "in_progress"}
                    onClick={() => void handleCompleteTask(task.id)}
                  >
                    {task.is_completed ? (
                      <CheckSquare className="h-4 w-4 text-green-600" />
                    ) : (
                      <Square className="h-4 w-4 text-muted-foreground hover:text-foreground" />
                    )}
                  </button>
                  <span
                    className={`flex-1 ${task.is_completed ? "line-through text-muted-foreground" : ""}`}
                  >
                    <span className="text-xs text-muted-foreground mr-1.5">{task.sequence}.</span>
                    {task.description}
                    {task.is_mandatory && (
                      <Badge
                        variant="outline"
                        className="text-[9px] ml-1.5 border-red-200 text-red-600"
                      >
                        {t("execution.mandatory")}
                      </Badge>
                    )}
                  </span>
                  {task.completed_at && (
                    <span className="text-[10px] text-muted-foreground shrink-0">
                      {formatDatetime(task.completed_at)}
                    </span>
                  )}
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── Parts usage section (summary) ─────────────────────────── */}
      <Card>
        <CardHeader className="px-4 py-3">
          <CardTitle className="text-sm flex items-center gap-2">
            <Package className="h-4 w-4" />
            {t("detail.sections.parts")}
          </CardTitle>
        </CardHeader>
        <CardContent className="px-4 pb-3">
          <p className="text-xs text-muted-foreground">{t("empty.noParts")}</p>
          {wo.status_code === "in_progress" && (
            <div className="pt-2">
              <Button variant="outline" size="sm" className="gap-1.5 text-xs">
                <Plus className="h-3 w-3" />
                {t("action.addPart")}
              </Button>
            </div>
          )}
        </CardContent>
      </Card>

      <Separator />

      {/* ── Actual times (read-only summary) ──────────────────────── */}
      <div className="grid grid-cols-2 gap-4 text-sm">
        <div>
          <span className="text-muted-foreground text-xs">{t("detail.fields.actualStart")}:</span>{" "}
          <span className="font-medium text-xs">
            {wo.actual_start ? formatDatetime(wo.actual_start) : "—"}
          </span>
        </div>
        <div>
          <span className="text-muted-foreground text-xs">{t("detail.fields.actualEnd")}:</span>{" "}
          <span className="font-medium text-xs">
            {wo.actual_end ? formatDatetime(wo.actual_end) : "—"}
          </span>
        </div>
        <div>
          <span className="text-muted-foreground text-xs">
            {t("detail.fields.estimatedHours")}:
          </span>{" "}
          <span className="font-medium text-xs">{formatHours(wo.expected_duration_hours)}</span>
        </div>
        <div>
          <span className="text-muted-foreground text-xs">{t("detail.fields.actualHours")}:</span>{" "}
          <span className="font-medium text-xs">{formatHours(wo.actual_duration_hours)}</span>
        </div>
      </div>
    </div>
  );
}
