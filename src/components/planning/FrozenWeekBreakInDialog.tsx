/**
 * New work into a frozen week (planning step D): the commit is refused with PLANNING_WEEK_FROZEN
 * unless it enters as a recorded break-in — a reason from the plant's list
 * (PLANNING.BREAK_IN_REASONS) and, for an urgent reason, an approver or a dangerous-override
 * reason. The backend checks all of it again; this dialog only collects it.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  breakInReasonLabel,
  isUrgentBreakInReason,
  useBreakInReasons,
} from "@/components/planning/break-in-reasons";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { ListAccessNote } from "@/components/ui/list-access-note";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useCanReadList } from "@/lib/list-access";
import type { CommitBreakIn } from "@shared/ipc-types";

type ApproverOption = { id: number; label: string };

type FrozenWeekBreakInDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** First day of the frozen week (`YYYY-MM-DD`), from the refusal. */
  weekStart: string | null;
  approvers: ApproverOption[];
  onConfirm: (breakIn: CommitBreakIn) => void;
};

export function FrozenWeekBreakInDialog({
  open,
  onOpenChange,
  weekStart,
  approvers,
  onConfirm,
}: FrozenWeekBreakInDialogProps) {
  const { t, i18n } = useTranslation("planning");
  // Approvers are user accounts: listed only with the user administration right.
  const canReadUsers = useCanReadList("users");
  const reasons = useBreakInReasons(open);
  const [reason, setReason] = useState("");
  const [approverUserId, setApproverUserId] = useState<number | null>(null);
  const [dangerousOverrideReason, setDangerousOverrideReason] = useState("");

  useEffect(() => {
    if (!open) {
      setReason("");
      setApproverUserId(null);
      setDangerousOverrideReason("");
    }
  }, [open]);

  const urgent = isUrgentBreakInReason(reasons.find((r) => r.code === reason));
  const evidenceMissing = urgent && approverUserId == null && dangerousOverrideReason.trim() === "";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("frozenWeek.title")}</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-text-secondary">
          {t("frozenWeek.hint", { week: weekStart ?? "—" })}
        </p>
        <div className="space-y-3">
          <div>
            <Label>{t("breakIns.reason")}</Label>
            <Select value={reason} onValueChange={setReason}>
              <SelectTrigger className="h-8 text-sm" aria-label={t("breakIns.reason")}>
                <SelectValue placeholder={t("frozenWeek.selectReason")} />
              </SelectTrigger>
              <SelectContent>
                {reasons.map((value) => (
                  <SelectItem key={value.code} value={value.code}>
                    {breakInReasonLabel(value.code, reasons, i18n.language)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {urgent ? (
              <p className="mt-1 text-xs text-text-warning">{t("breakIns.urgentHint")}</p>
            ) : null}
          </div>
          <div>
            <Label>{t("breakIns.approver")}</Label>
            <Select
              disabled={!canReadUsers}
              value={approverUserId != null ? String(approverUserId) : "__none__"}
              onValueChange={(v) => setApproverUserId(v === "__none__" ? null : Number(v))}
            >
              <SelectTrigger className="h-8 text-sm" aria-label={t("breakIns.approver")}>
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
            {!canReadUsers ? <ListAccessNote list="users" className="mt-1" /> : null}
          </div>
          <div>
            <Label htmlFor="frozen-week-dangerous-reason">
              {t("breakIns.dangerousOverrideReason")}
            </Label>
            <Textarea
              id="frozen-week-dangerous-reason"
              className="min-h-[64px] text-sm"
              value={dangerousOverrideReason}
              onChange={(e) => setDangerousOverrideReason(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("dialogs.cancel")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={reason === "" || evidenceMissing}
            onClick={() =>
              onConfirm({
                reason,
                approved_by_user_id: approverUserId,
                dangerous_override_reason: dangerousOverrideReason.trim() || null,
              })
            }
          >
            {t("frozenWeek.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
