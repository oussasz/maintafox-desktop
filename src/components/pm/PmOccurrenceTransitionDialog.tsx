import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import type { PmOccurrence, TransitionPmOccurrenceInput } from "@shared/ipc-types";

import { PM_REASON_CODES, allowedOccurrenceTargets } from "./pm-labels";

interface PmOccurrenceTransitionDialogProps {
  open: boolean;
  occurrence: PmOccurrence | null;
  saving?: boolean;
  onClose: () => void;
  onSubmit: (input: TransitionPmOccurrenceInput) => Promise<void>;
}

export function PmOccurrenceTransitionDialog({
  open,
  occurrence,
  saving,
  onClose,
  onSubmit,
}: PmOccurrenceTransitionDialogProps) {
  const { t } = useTranslation("pm");
  const targets = useMemo(
    () => (occurrence ? allowedOccurrenceTargets(occurrence.status) : []),
    [occurrence],
  );
  const [nextStatus, setNextStatus] = useState("");
  const [reasonCode, setReasonCode] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !occurrence) return;
    setNextStatus(targets[0] ?? "");
    setReasonCode("");
    setNote("");
    setError(null);
  }, [open, occurrence, targets]);

  const needsReason = ["deferred", "missed", "cancelled"].includes(nextStatus);
  const generateWo = nextStatus === "ready_for_scheduling";

  async function handleSubmit() {
    if (!occurrence || !nextStatus) return;
    setError(null);
    if (needsReason && !reasonCode) {
      setError(t("fields.reasonCode"));
      return;
    }
    try {
      await onSubmit({
        occurrence_id: occurrence.id,
        expected_row_version: occurrence.row_version,
        next_status: nextStatus,
        reason_code: needsReason ? reasonCode : null,
        note: note.trim() || null,
        generate_work_order: generateWo,
        work_order_type_id: null,
        actor_id: null,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("dialogs.transitionOccurrence.title")}</DialogTitle>
          <DialogDescription>{t("dialogs.transitionOccurrence.description")}</DialogDescription>
        </DialogHeader>
        {occurrence ? (
          <div className="grid gap-3 py-2">
            <p className="text-sm">
              <span className="font-mono">{occurrence.plan_code}</span>
              {" · "}
              {t(`occurrenceStatus.${occurrence.status}`, { defaultValue: occurrence.status })}
            </p>
            <div className="grid gap-1">
              <Label>{t("fields.status")}</Label>
              <Select value={nextStatus} onValueChange={setNextStatus}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {targets.map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`occurrenceStatus.${s}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {generateWo ? (
                <p className="text-xs text-muted-foreground">{t("hints.readyCreatesWo")}</p>
              ) : null}
            </div>
            {needsReason ? (
              <div className="grid gap-1">
                <Label>{t("fields.reasonCode")}</Label>
                <Select value={reasonCode} onValueChange={setReasonCode}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PM_REASON_CODES.map((r) => (
                      <SelectItem key={r} value={r}>
                        {t(`reasons.${r}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            ) : null}
            <div className="grid gap-1">
              <Label>{t("fields.note")}</Label>
              <Textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
            </div>
            {error ? <p className="text-sm text-destructive">{error}</p> : null}
          </div>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button
            disabled={saving || !nextStatus || targets.length === 0}
            onClick={() => void handleSubmit()}
          >
            {t("actions.transition")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
