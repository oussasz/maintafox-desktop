/**
 * WoCompletionDialog.tsx
 *
 * Two-step completion confirmation:
 *   Step 1 — Completion Summary (plan vs actual adherence snapshot)
 *   Step 2 — Completion form + readiness checklist
 */

import { CheckCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { WoCompletionGatesChecklist } from "@/components/wo/WoCompletionGatesChecklist";
import { useSession } from "@/hooks/use-session";
import {
  formatDurationMinutes,
  hoursToMinutes,
} from "@/lib/display";
import { getPlanAdherence, type WoPlanAdherence } from "@/services/wo-execution-service";
import { evaluateWoCompletionGates } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import type { WoCompletionGate, WorkOrder } from "@shared/ipc-types";

function toDatetimeLocal(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function CompletionSummary({
  adherence,
}: {
  adherence: WoPlanAdherence | null;
}) {
  const { t } = useTranslation("ot");
  const durationLabels = {
    hours: t("execution.durationHours"),
    minutes: t("execution.durationMinutes"),
  };

  if (!adherence) {
    return (
      <p className="text-sm text-muted-foreground">{t("completion.summary.loading")}</p>
    );
  }

  const efficiency = adherence.efficiency_pct;
  const costVar = adherence.cost_variance_pct;
  const dtVar = adherence.downtime_variance_hours;

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t("completion.summary.partsTitle")}
        </p>
        <p className="text-sm tabular-nums">
          {t("completion.summary.partsLine", {
            planned: adherence.parts_planned,
            used: adherence.parts_used,
            unused: adherence.parts_unused,
            extra: adherence.parts_extra,
          })}
        </p>
      </div>

      <Separator />

      <div className="space-y-1">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t("completion.summary.tasksTitle")}
        </p>
        <p className="text-sm tabular-nums">
          {t("completion.summary.tasksLine", {
            planned: adherence.tasks_planned,
            done: adherence.tasks_done,
            added: adherence.tasks_added,
            cancelled: adherence.tasks_cancelled,
          })}
        </p>
      </div>

      <Separator />

      <div className="space-y-1">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t("completion.summary.laborTitle")}
        </p>
        <div className="grid grid-cols-3 gap-2 text-sm">
          <div>
            <p className="text-xs text-muted-foreground">{t("completion.summary.planned")}</p>
            <p className="font-medium tabular-nums">
              {adherence.planned_hours != null
                ? formatDurationMinutes(
                    hoursToMinutes(adherence.planned_hours),
                    "auto",
                    durationLabels,
                  )
                : "—"}
            </p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t("completion.summary.actual")}</p>
            <p className="font-medium tabular-nums">
              {adherence.actual_hours != null
                ? formatDurationMinutes(
                    hoursToMinutes(adherence.actual_hours),
                    "auto",
                    durationLabels,
                  )
                : "—"}
            </p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">{t("completion.summary.efficiency")}</p>
            <p className="font-medium tabular-nums">
              {efficiency != null ? `${Math.round(efficiency)} %` : "—"}
            </p>
          </div>
        </div>
      </div>

      <Separator />

      <div className="space-y-1">
        <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {t("completion.summary.costTitle")}
        </p>
        <p className="text-sm font-medium tabular-nums">
          {costVar != null ? `${costVar > 0 ? "+" : ""}${Math.round(costVar)} %` : "—"}
        </p>
      </div>

      {(adherence.planned_downtime_hours != null || adherence.actual_downtime_hours > 0) && (
        <>
          <Separator />
          <div className="space-y-1">
            <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {t("completion.summary.downtimeTitle")}
            </p>
            <p className="text-sm tabular-nums">
              {dtVar != null
                ? `${dtVar > 0 ? "+" : ""}${formatDurationMinutes(
                    hoursToMinutes(Math.abs(dtVar)),
                    "auto",
                    durationLabels,
                  )}`
                : formatDurationMinutes(
                    hoursToMinutes(adherence.actual_downtime_hours),
                    "auto",
                    durationLabels,
                  )}
            </p>
          </div>
        </>
      )}
    </div>
  );
}

interface WoCompletionDialogProps {
  wo: WorkOrder;
}

type DialogStep = "summary" | "form";

export function WoCompletionDialog({ wo }: WoCompletionDialogProps) {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const showCompletionDialog = useWoStore((s) => s.showCompletionDialog);
  const closeCompletionDialog = useWoStore((s) => s.closeCompletionDialog);
  const completeWorkOrder = useWoStore((s) => s.completeWorkOrder);
  const completionErrors = useWoStore((s) => s.completionErrors);

  const [step, setStep] = useState<DialogStep>("summary");
  const [adherence, setAdherence] = useState<WoPlanAdherence | null>(null);
  const [adherenceLoading, setAdherenceLoading] = useState(false);
  const [gates, setGates] = useState<WoCompletionGate[]>([]);
  const [gatesLoading, setGatesLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [actualEnd, setActualEnd] = useState(() => toDatetimeLocal(new Date()));
  const [hoursWorked, setHoursWorked] = useState("");
  const [conclusion, setConclusion] = useState("");

  const loadGates = useCallback(async () => {
    setGatesLoading(true);
    try {
      setGates(await evaluateWoCompletionGates(wo.id));
    } catch {
      setGates([]);
    } finally {
      setGatesLoading(false);
    }
  }, [wo.id]);

  useEffect(() => {
    if (!showCompletionDialog) return;
    setStep("summary");
    setActualEnd(toDatetimeLocal(new Date()));
    setHoursWorked("");
    setConclusion("");
    setAdherenceLoading(true);
    void getPlanAdherence(wo.id)
      .then(setAdherence)
      .catch(() => setAdherence(null))
      .finally(() => setAdherenceLoading(false));
    void loadGates();
  }, [showCompletionDialog, wo.id, loadGates]);

  useEffect(() => {
    if (completionErrors.length > 0) {
      setStep("form");
      void loadGates();
    }
  }, [completionErrors, loadGates]);

  const handleSubmit = useCallback(async () => {
    const actorId = info?.user_id;
    if (actorId == null) return;
    const hours = hoursWorked.trim() ? Number(hoursWorked) : null;
    setSubmitting(true);
    try {
      await completeWorkOrder({
        wo_id: wo.id,
        actor_id: actorId,
        expected_row_version: wo.row_version,
        actual_end: new Date(actualEnd).toISOString(),
        actual_duration_hours: hours != null && Number.isFinite(hours) ? hours : null,
        conclusion: conclusion.trim() || null,
      });
      await loadGates();
    } finally {
      setSubmitting(false);
    }
  }, [
    actualEnd,
    completeWorkOrder,
    conclusion,
    hoursWorked,
    info?.user_id,
    loadGates,
    wo.id,
    wo.row_version,
  ]);

  const blockingCount = gates.filter((g) => g.required && !g.passed).length;

  return (
    <Dialog
      open={showCompletionDialog}
      onOpenChange={(open) => {
        if (!open) closeCompletionDialog();
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <CheckCircle className="h-5 w-5 text-primary" />
            {step === "summary" ? t("completion.summary.title") : t("completion.title")}
          </DialogTitle>
          <DialogDescription>
            {step === "summary"
              ? t("completion.summary.description")
              : t("completion.description")}
          </DialogDescription>
        </DialogHeader>

        {step === "summary" && (
          <div className="space-y-3 py-2">
            {adherenceLoading ? (
              <p className="text-sm text-muted-foreground">{t("completion.summary.loading")}</p>
            ) : (
              <CompletionSummary adherence={adherence} />
            )}
            {!gatesLoading && gates.length > 0 && (
              <WoCompletionGatesChecklist
                gates={gates}
                title={t("completion.gates.title")}
              />
            )}
          </div>
        )}

        {step === "form" && (
          <div className="space-y-4 py-2">
            {!gatesLoading && (
              <WoCompletionGatesChecklist
                gates={gates}
                title={t("completion.gates.title")}
              />
            )}

            <div className="space-y-1">
              <Label htmlFor="wo-complete-end">{t("completion.endDate")}</Label>
              <Input
                id="wo-complete-end"
                type="datetime-local"
                value={actualEnd}
                onChange={(e) => setActualEnd(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="wo-complete-hours">{t("completion.hoursWorked")}</Label>
              <Input
                id="wo-complete-hours"
                type="number"
                min={0}
                step={0.25}
                value={hoursWorked}
                onChange={(e) => setHoursWorked(e.target.value)}
                placeholder={t("completion.hoursPlaceholder")}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="wo-complete-report">{t("completion.report")}</Label>
              <Textarea
                id="wo-complete-report"
                rows={3}
                value={conclusion}
                onChange={(e) => setConclusion(e.target.value)}
                placeholder={t("completion.reportPlaceholder")}
              />
            </div>
          </div>
        )}

        <div className="flex justify-end gap-2 pt-2">
          {step === "summary" ? (
            <>
              <Button variant="outline" onClick={closeCompletionDialog}>
                {t("execution.cancel")}
              </Button>
              <Button
                onClick={() => {
                  setStep("form");
                  void loadGates();
                }}
                disabled={adherenceLoading}
              >
                {t("completion.summary.confirm")}
              </Button>
            </>
          ) : (
            <>
              <Button variant="ghost" size="sm" onClick={() => setStep("summary")}>
                {t("completion.summary.back")}
              </Button>
              <Button variant="outline" onClick={closeCompletionDialog}>
                {t("execution.cancel")}
              </Button>
              <Button
                onClick={() => void handleSubmit()}
                disabled={submitting || blockingCount > 0}
              >
                {t("completion.submit")}
              </Button>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
