import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import type { ExecutePmOccurrenceInput, PmOccurrence } from "@shared/ipc-types";

import { PM_EXECUTION_RESULTS, PM_REASON_CODES } from "./pm-labels";

interface PmExecutionDialogProps {
  open: boolean;
  occurrence: PmOccurrence | null;
  saving?: boolean;
  onClose: () => void;
  onSubmit: (input: ExecutePmOccurrenceInput) => Promise<void>;
}

export function PmExecutionDialog({
  open,
  occurrence,
  saving,
  onClose,
  onSubmit,
}: PmExecutionDialogProps) {
  const { t } = useTranslation("pm");
  const [result, setResult] = useState("completed_no_findings");
  const [reasonCode, setReasonCode] = useState("");
  const [note, setNote] = useState("");
  const [findingType, setFindingType] = useState("");
  const [severity, setSeverity] = useState("medium");
  const [findingDescription, setFindingDescription] = useState("");
  const [followUpDi, setFollowUpDi] = useState(false);
  const [followUpWo, setFollowUpWo] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setResult("completed_no_findings");
    setReasonCode("");
    setNote("");
    setFindingType("");
    setSeverity("medium");
    setFindingDescription("");
    setFollowUpDi(false);
    setFollowUpWo(false);
    setError(null);
  }, [open, occurrence?.id]);

  const needsReason = result === "deferred" || result === "missed";
  const needsFinding = result === "completed_with_findings";

  async function handleSubmit() {
    if (!occurrence) return;
    setError(null);
    if (needsReason && !reasonCode) {
      setError(t("fields.reasonCode"));
      return;
    }
    if (needsFinding && (!findingType.trim() || !findingDescription.trim())) {
      setError(t("fields.findingDescription"));
      return;
    }
    try {
      await onSubmit({
        occurrence_id: occurrence.id,
        expected_occurrence_row_version: occurrence.row_version,
        execution_result: result,
        note: note.trim() || null,
        actor_id: null,
        work_order_id: occurrence.linked_work_order_id,
        defer_reason_code: result === "deferred" ? reasonCode : null,
        miss_reason_code: result === "missed" ? reasonCode : null,
        findings: needsFinding
          ? [
              {
                finding_type: findingType.trim(),
                severity,
                description: findingDescription.trim(),
                create_follow_up_di: followUpDi,
                create_follow_up_work_order: followUpWo,
              },
            ]
          : null,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("dialogs.execution.title")}</DialogTitle>
          <DialogDescription>{t("dialogs.execution.description")}</DialogDescription>
        </DialogHeader>
        {occurrence ? (
          <div className="grid gap-3 py-2">
            <p className="text-sm">
              <span className="font-mono">{occurrence.plan_code}</span>
              {occurrence.linked_work_order_code ? ` · ${occurrence.linked_work_order_code}` : ""}
            </p>
            <div className="grid gap-1">
              <Label>{t("fields.executionResult")}</Label>
              <Select value={result} onValueChange={setResult}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PM_EXECUTION_RESULTS.map((r) => (
                    <SelectItem key={r} value={r}>
                      {t(`executionResult.${r}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
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
            {needsFinding ? (
              <>
                <div className="grid gap-1">
                  <Label>{t("fields.findingType")}</Label>
                  <Input value={findingType} onChange={(e) => setFindingType(e.target.value)} />
                </div>
                <div className="grid gap-1">
                  <Label>{t("fields.severity")}</Label>
                  <Select value={severity} onValueChange={setSeverity}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {["low", "medium", "high", "critical"].map((s) => (
                        <SelectItem key={s} value={s}>
                          {s}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="grid gap-1">
                  <Label>{t("fields.findingDescription")}</Label>
                  <Textarea
                    rows={3}
                    value={findingDescription}
                    onChange={(e) => setFindingDescription(e.target.value)}
                  />
                </div>
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={followUpDi}
                    onCheckedChange={(v) => setFollowUpDi(v === true)}
                  />
                  {t("actions.createFollowUpDi")}
                </label>
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    checked={followUpWo}
                    onCheckedChange={(v) => setFollowUpWo(v === true)}
                  />
                  {t("actions.createFollowUpWo")}
                </label>
              </>
            ) : null}
            {error ? <p className="text-sm text-destructive">{error}</p> : null}
          </div>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button disabled={saving} onClick={() => void handleSubmit()}>
            {t("actions.logExecution")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
