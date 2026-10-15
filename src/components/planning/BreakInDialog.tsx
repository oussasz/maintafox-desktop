import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  fromDateTimeLocal,
  PLANNING_BREAK_IN_REASONS,
  toDateTimeLocal,
  type PlanningBreakInReason,
} from "@/components/planning/planning-model";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
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
import { formatOrDash, formatPersonLabel } from "@/lib/display";
import type {
  CreateScheduleBreakInInput,
  ScheduleBreakIn,
  ScheduleCommitment,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type ApproverOption = { id: number; label: string };
type PersonnelOption = { id: number; label: string };

type BreakInDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commitments: ScheduleCommitment[];
  breakIns: ScheduleBreakIn[];
  teamId: number | null;
  approvers: ApproverOption[];
  personnel: PersonnelOption[];
  saving: boolean;
  onSubmit: (input: CreateScheduleBreakInInput) => Promise<void>;
};

export function BreakInDialog({
  open,
  onOpenChange,
  commitments,
  breakIns,
  teamId,
  approvers,
  personnel,
  saving,
  onSubmit,
}: BreakInDialogProps) {
  const { t } = useTranslation("planning");
  const [commitmentId, setCommitmentId] = useState<number | null>(null);
  const [reason, setReason] = useState<PlanningBreakInReason>("emergency");
  const [approverUserId, setApproverUserId] = useState<number | null>(null);
  const [newStart, setNewStart] = useState("");
  const [newEnd, setNewEnd] = useState("");
  const [newPersonnelId, setNewPersonnelId] = useState<number | null>(null);
  const [overrideReason, setOverrideReason] = useState("");
  const [dangerousOverrideReason, setDangerousOverrideReason] = useState("");

  const selected = commitments.find((c) => c.id === commitmentId) ?? null;

  useEffect(() => {
    if (!selected || !open) return;
    setNewStart(toDateTimeLocal(selected.committed_start));
    setNewEnd(toDateTimeLocal(selected.committed_end));
    setNewPersonnelId(selected.assigned_personnel_id);
  }, [selected, open]);

  async function handleSubmit() {
    if (!selected) return;
    await onSubmit({
      schedule_commitment_id: selected.id,
      expected_commitment_row_version: selected.row_version,
      break_in_reason: reason,
      approved_by_user_id: approverUserId,
      new_slot_start: fromDateTimeLocal(newStart),
      new_slot_end: fromDateTimeLocal(newEnd),
      new_assigned_team_id: teamId,
      new_assigned_personnel_id: newPersonnelId,
      bypass_availability: false,
      override_reason: overrideReason.trim() || null,
      dangerous_override_reason: dangerousOverrideReason.trim() || null,
    });
    setOverrideReason("");
    setDangerousOverrideReason("");
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("breakIns.title")}</DialogTitle>
        </DialogHeader>

        <div className="grid gap-3 py-2">
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("breakIns.commitment")}</Label>
              <Select
                value={commitmentId != null ? String(commitmentId) : "__none__"}
                onValueChange={(v) => setCommitmentId(v === "__none__" ? null : Number(v))}
              >
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={t("breakIns.selectCommitment")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("breakIns.selectCommitment")}</SelectItem>
                  {commitments.map((commitment) => (
                    <SelectItem key={commitment.id} value={String(commitment.id)}>
                      {formatOrDash(commitment.source_code) || commitment.source_type}
                      {commitment.source_title ? ` — ${commitment.source_title}` : ""}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label>{t("breakIns.reason")}</Label>
              <Select value={reason} onValueChange={(v) => setReason(v as PlanningBreakInReason)}>
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PLANNING_BREAK_IN_REASONS.map((code) => (
                    <SelectItem key={code} value={code}>
                      {t(`breakIns.reasons.${code}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("breakIns.start")}</Label>
              <Input
                className="h-8 text-sm"
                type="datetime-local"
                value={newStart}
                onChange={(e) => setNewStart(e.target.value)}
              />
            </div>
            <div>
              <Label>{t("breakIns.end")}</Label>
              <Input
                className="h-8 text-sm"
                type="datetime-local"
                value={newEnd}
                onChange={(e) => setNewEnd(e.target.value)}
              />
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <Label>{t("breakIns.approver")}</Label>
              <Select
                value={approverUserId != null ? String(approverUserId) : "__none__"}
                onValueChange={(v) => setApproverUserId(v === "__none__" ? null : Number(v))}
              >
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue placeholder={t("breakIns.selectApprover")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("breakIns.selectApprover")}</SelectItem>
                  {approvers.map((user) => (
                    <SelectItem key={user.id} value={String(user.id)}>
                      {user.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label>{t("breakIns.newAssignee")}</Label>
              <Select
                value={newPersonnelId != null ? String(newPersonnelId) : "__unassigned__"}
                onValueChange={(v) => setNewPersonnelId(v === "__unassigned__" ? null : Number(v))}
              >
                <SelectTrigger className="h-8 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="__unassigned__">{t("commitment.unassigned")}</SelectItem>
                  {personnel.map((person) => (
                    <SelectItem key={person.id} value={String(person.id)}>
                      {person.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          <div>
            <Label>{t("breakIns.overrideReason")}</Label>
            <Textarea
              className="min-h-[64px] text-sm"
              value={overrideReason}
              onChange={(e) => setOverrideReason(e.target.value)}
            />
          </div>
          <div>
            <Label>{t("breakIns.dangerousOverrideReason")}</Label>
            <Textarea
              className="min-h-[64px] text-sm"
              value={dangerousOverrideReason}
              onChange={(e) => setDangerousOverrideReason(e.target.value)}
            />
          </div>

          <div className="max-h-36 space-y-1 overflow-y-auto text-xs">
            {breakIns.map((entry) => (
              <div
                key={entry.id}
                className="rounded-lg border border-surface-border p-2 text-text-secondary"
              >
                <span className="font-medium text-text-primary">
                  {formatOrDash(entry.commitment_source_code)}
                </span>{" "}
                {t(`breakIns.reasons.${entry.break_in_reason}`, {
                  defaultValue: entry.break_in_reason,
                })}
                {" · "}
                {new Date(entry.old_slot_start).toLocaleString()}
                {" → "}
                {new Date(entry.new_slot_start).toLocaleString()}
                {entry.old_assignee_name || entry.new_assignee_name
                  ? ` · ${formatPersonLabel(entry.old_assignee_name)} → ${formatPersonLabel(entry.new_assignee_name)}`
                  : null}
                {entry.cost_impact_delta != null
                  ? ` · Δ ${entry.cost_impact_delta.toFixed(2)}`
                  : null}
              </div>
            ))}
            {breakIns.length === 0 && <p className="text-text-muted">{t("breakIns.empty")}</p>}
          </div>
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("dialogs.cancel")}
          </Button>
          <PermissionGate permission={P.PLAN_CONFIRM}>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={!selected || saving}
              onClick={() => void handleSubmit()}
            >
              {t("breakIns.create")}
            </Button>
          </PermissionGate>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
