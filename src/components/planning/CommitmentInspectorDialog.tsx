import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { ConflictReasonList } from "@/components/planning/ConflictReasonList";
import { parseConflictReasons } from "@/components/planning/planning-model";
import { errorText } from "@/components/planning/weekly-schedule-model";
import { Timeline } from "@/components/timeline/Timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import type { ScheduleChangeLogEntry, ScheduleCommitment } from "@shared/ipc-types";

type CommitmentInspectorDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commitment: ScheduleCommitment | null;
  changeLog: ScheduleChangeLogEntry[];
  loadingChangeLog: boolean;
  onLoadChangeLog: (commitmentId: number) => void;
  onRequestBreakIn?: (commitmentId: number) => void;
  /** Jump to Gantt board focused on this commitment (calendar surface). */
  onOpenOnBoard?: (commitmentId: number) => void;
  /**
   * Ends a committed slot: "released" gives the slot back (the job returns to the backlog),
   * "cancelled" withdraws it. Absent without the planning confirm right.
   */
  onEndCommitment?: (
    commitment: ScheduleCommitment,
    to: "released" | "cancelled",
    reason: string,
  ) => Promise<void>;
};

function actionTone(action: string): TimelineTone {
  if (action.includes("override") || action.includes("break")) return "warning";
  if (action.includes("freeze")) return "info";
  return "muted";
}

export function CommitmentInspectorDialog({
  open,
  onOpenChange,
  commitment,
  changeLog,
  loadingChangeLog,
  onLoadChangeLog,
  onRequestBreakIn,
  onOpenOnBoard,
  onEndCommitment,
}: CommitmentInspectorDialogProps) {
  const { t, i18n } = useTranslation("planning");
  const { formatDateTime, formatCurrency } = useFormatters();
  const [ending, setEnding] = useState<"released" | "cancelled" | null>(null);
  const [endReason, setEndReason] = useState("");
  const [endBusy, setEndBusy] = useState(false);
  const [endError, setEndError] = useState<string | null>(null);

  useEffect(() => {
    setEnding(null);
    setEndReason("");
    setEndError(null);
  }, [commitment?.id, open]);

  // A frozen slot changes only through a break-in; only a committed one can end.
  const canEnd =
    commitment != null && commitment.status === "committed" && commitment.frozen_at == null;

  async function confirmEnd() {
    if (!commitment || !ending || !onEndCommitment) return;
    setEndBusy(true);
    setEndError(null);
    try {
      await onEndCommitment(commitment, ending, endReason.trim());
      setEnding(null);
      setEndReason("");
    } catch (err) {
      setEndError(errorText(err));
    } finally {
      setEndBusy(false);
    }
  }
  const conflictReasons = useMemo(
    () => parseConflictReasons(commitment?.conflict_reasons_json),
    [commitment?.conflict_reasons_json],
  );

  useEffect(() => {
    if (open && commitment) {
      onLoadChangeLog(commitment.id);
    }
  }, [open, commitment, onLoadChangeLog]);

  const entries: TimelineEntry[] = useMemo(
    () =>
      changeLog.map((entry) => {
        const badges = [];
        if (entry.reason_code) {
          badges.push({
            label: entry.reason_code,
            tone: "info" as TimelineTone,
          });
        }
        if (entry.field_changed) {
          badges.push({
            label: entry.field_changed,
            tone: "muted" as TimelineTone,
          });
        }
        return {
          id: String(entry.id),
          title: t(`audit.actions.${entry.action_type}`, {
            defaultValue: entry.action_type,
          }),
          description:
            entry.reason_note ||
            entry.reason ||
            (entry.old_value || entry.new_value
              ? `${formatOrDash(entry.old_value)} → ${formatOrDash(entry.new_value)}`
              : undefined),
          timestamp: entry.created_at,
          actor: formatPersonLabel(entry.actor_display_name),
          badges,
          tone: actionTone(entry.action_type),
          details: entry.details_json ?? undefined,
        };
      }),
    [changeLog, t],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90vh] w-full max-w-3xl flex-col gap-0 p-0">
        <DialogHeader className="px-6 pt-5 pb-3">
          <DialogTitle className="flex flex-wrap items-center gap-2">
            <span>
              {formatOrDash(commitment?.source_code) ||
                commitment?.source_type ||
                t("inspector.title")}
            </span>
            {commitment && commitment.status !== "committed" ? (
              <Badge variant="outline">{t(`commitmentStatus.${commitment.status}`)}</Badge>
            ) : null}
            {commitment?.frozen_at ? (
              <Badge variant="secondary">{t("inspector.frozen")}</Badge>
            ) : null}
            {commitment?.has_blocking_conflict ? (
              <Badge variant="destructive">{t("inspector.blockingConflict")}</Badge>
            ) : null}
          </DialogTitle>
          {commitment?.source_title ? (
            <p className="text-sm text-text-secondary">{commitment.source_title}</p>
          ) : null}
          <ConflictReasonList reasons={conflictReasons} />
          {commitment?.source_type === "work_order" ? (
            <div className="flex flex-wrap items-center gap-2 pt-1 text-xs text-text-muted">
              <span>{t("inspector.sourceWorkOrder")}</span>
              <LinkedEntityBadge
                entity="work_order"
                code={commitment.source_code}
                entityId={commitment.source_id}
                title={commitment.source_title}
              />
            </div>
          ) : null}
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 pb-4">
          {commitment ? (
            <div className="grid gap-2 text-sm sm:grid-cols-2">
              <div>
                <div className="text-[10px] uppercase tracking-wide text-text-muted">
                  {t("inspector.slot")}
                </div>
                <div className="text-text-primary">
                  {formatDateTime(commitment.committed_start)} –{" "}
                  {formatDateTime(commitment.committed_end)}
                </div>
              </div>
              <div>
                <div className="text-[10px] uppercase tracking-wide text-text-muted">
                  {t("inspector.assignee")}
                </div>
                <div className="text-text-primary">
                  {formatPersonLabel(commitment.assigned_personnel_name)}
                </div>
              </div>
              <div>
                <div className="text-[10px] uppercase tracking-wide text-text-muted">
                  {t("inspector.team")}
                </div>
                <div className="text-text-primary">
                  {formatOrDash(commitment.assigned_team_label)}
                </div>
              </div>
              <div>
                <div className="text-[10px] uppercase tracking-wide text-text-muted">
                  {t("inspector.cost")}
                </div>
                <div className="text-text-primary">
                  {commitment.estimated_labor_cost != null
                    ? formatCurrency(commitment.estimated_labor_cost)
                    : "—"}
                  {commitment.cost_variance_warning ? (
                    <Badge variant="destructive" className="ml-2">
                      {t("inspector.costVariance")}
                    </Badge>
                  ) : null}
                </div>
              </div>
              {commitment.status !== "committed" && commitment.status_reason ? (
                <div className="sm:col-span-2">
                  <div className="text-[10px] uppercase tracking-wide text-text-muted">
                    {t("inspector.statusReason")}
                  </div>
                  <div className="text-text-primary">
                    {commitment.status_reason}
                    {commitment.status_changed_at
                      ? ` (${formatDateTime(commitment.status_changed_at)})`
                      : ""}
                  </div>
                </div>
              ) : null}
              {commitment.nearest_feasible_window ? (
                <div className="sm:col-span-2">
                  <div className="text-[10px] uppercase tracking-wide text-text-muted">
                    {t("inspector.nearestWindow")}
                  </div>
                  <div className="text-text-primary">
                    {formatDateTime(commitment.nearest_feasible_window)}
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}

          {ending ? (
            <div className="space-y-2 rounded-md border border-surface-border p-3">
              <p className="text-sm text-text-secondary">{t(`lifecycle.${ending}.hint`)}</p>
              <Label htmlFor="commitment-end-reason">{t("lifecycle.reason")}</Label>
              <Textarea
                id="commitment-end-reason"
                value={endReason}
                onChange={(e) => setEndReason(e.target.value)}
              />
              {endError ? (
                <p role="alert" className="text-sm text-status-danger">
                  {endError}
                </p>
              ) : null}
              <div className="flex gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant={ending === "cancelled" ? "destructive" : "default"}
                  disabled={endBusy || endReason.trim() === ""}
                  onClick={() => void confirmEnd()}
                >
                  {t(`lifecycle.${ending}.confirm`)}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={endBusy}
                  onClick={() => setEnding(null)}
                >
                  {t("dialogs.cancel")}
                </Button>
              </div>
            </div>
          ) : null}

          <Separator />

          <div>
            <h3 className="mb-2 text-sm font-medium text-text-primary">
              {t("inspector.auditTitle")}
            </h3>
            <Timeline
              items={entries}
              loading={loadingChangeLog}
              locale={i18n.language}
              empty={t("inspector.auditEmpty")}
              aria-label={t("inspector.auditTitle")}
            />
          </div>
        </div>

        <Separator />
        <div className="flex items-center justify-between gap-2 px-6 py-3">
          <div className="flex flex-wrap items-center gap-2">
            {commitment && onRequestBreakIn ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => onRequestBreakIn(commitment.id)}
              >
                {t("breakIns.create")}
              </Button>
            ) : null}
            {canEnd && onEndCommitment && ending == null ? (
              <>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => setEnding("released")}
                >
                  {t("lifecycle.released.action")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => setEnding("cancelled")}
                >
                  {t("lifecycle.cancelled.action")}
                </Button>
              </>
            ) : null}
            {commitment && onOpenOnBoard ? (
              <Button
                type="button"
                size="sm"
                variant="secondary"
                onClick={() => onOpenOnBoard(commitment.id)}
              >
                {t("inspector.openOnBoard")}
              </Button>
            ) : null}
          </div>
          <Button type="button" size="sm" variant="outline" onClick={() => onOpenChange(false)}>
            {t("dialogs.close")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
