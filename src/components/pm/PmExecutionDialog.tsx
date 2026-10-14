import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import type { PmOccurrenceRow } from "@/services/pm-service";
import type { ExecutePmOccurrenceInput } from "@shared/ipc-types";

import { formatPmAsset } from "./pm-cells";
import { pmUnreportedErrorText } from "./pm-errors";
import { PM_EXECUTION_RESULTS, PM_FINDING_SEVERITIES, PM_REASON_CODES } from "./pm-labels";
import { allowedExecutionResults } from "./pm-model";

interface PmExecutionDialogProps {
  open: boolean;
  occurrence: PmOccurrenceRow | null;
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
  const results = useMemo(
    () => (occurrence ? allowedExecutionResults(occurrence.status, PM_EXECUTION_RESULTS) : []),
    [occurrence],
  );
  const [result, setResult] = useState("");
  const [reasonCode, setReasonCode] = useState("");
  const [note, setNote] = useState("");
  const [findingType, setFindingType] = useState<string | null>(null);
  const [severity, setSeverity] = useState("");
  const [findingDescription, setFindingDescription] = useState("");
  const [followUpDi, setFollowUpDi] = useState(false);
  const [followUpWo, setFollowUpWo] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setResult(results[0] ?? "");
    setReasonCode("");
    setNote("");
    setFindingType(null);
    setSeverity("");
    setFindingDescription("");
    setFollowUpDi(false);
    setFollowUpWo(false);
    setError(null);
  }, [open, results]);

  const needsReason = result === "deferred" || result === "missed";
  const needsFinding = result === "completed_with_findings";

  async function handleSubmit() {
    if (!occurrence || !result) return;
    setError(null);
    if (needsReason && !reasonCode) {
      setError(t("errors.reasonRequired"));
      return;
    }
    if (needsFinding && (!findingType || !findingDescription.trim())) {
      setError(t("errors.findingRequired"));
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
        findings:
          needsFinding && findingType
            ? [
                {
                  finding_type: findingType,
                  severity: severity || null,
                  description: findingDescription.trim(),
                  create_follow_up_di: followUpDi,
                  create_follow_up_work_order: followUpWo,
                },
              ]
            : null,
      });
      onClose();
    } catch (err) {
      setError(pmUnreportedErrorText(err));
    }
  }

  const asset = occurrence ? formatPmAsset(occurrence.asset_code, occurrence.asset_name) : null;

  return (
    <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
      <DialogContent className="max-h-[90vh] max-w-lg overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("dialogs.execution.title")}</DialogTitle>
          <DialogDescription>{t("dialogs.execution.description")}</DialogDescription>
        </DialogHeader>
        {occurrence ? (
          <div className="grid gap-3 py-2">
            <div className="flex flex-wrap items-center gap-2 text-sm">
              <span className="font-mono">{occurrence.plan_code}</span>
              {occurrence.plan_title ? <span>{occurrence.plan_title}</span> : null}
              {asset ? <span className="text-text-muted">· {asset}</span> : null}
              <LinkedEntityBadge
                entity="work_order"
                code={occurrence.linked_work_order_code}
                entityId={occurrence.linked_work_order_id}
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="pm-exec-result">{t("fields.executionResult")}</Label>
              <Select value={result} onValueChange={setResult}>
                <SelectTrigger id="pm-exec-result">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {results.map((r) => (
                    <SelectItem key={r} value={r}>
                      {t(`executionResult.${r}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            {needsReason ? (
              <div className="grid gap-1">
                <Label htmlFor="pm-exec-reason">{t("fields.reasonCode")}</Label>
                <Select value={reasonCode} onValueChange={setReasonCode}>
                  <SelectTrigger id="pm-exec-reason">
                    <SelectValue placeholder={t("errors.reasonRequired")} />
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
              <Label htmlFor="pm-exec-note">{t("fields.note")}</Label>
              <Textarea
                id="pm-exec-note"
                rows={3}
                value={note}
                onChange={(e) => setNote(e.target.value)}
              />
            </div>
            {needsFinding ? (
              <>
                <div className="grid gap-1">
                  <ReferenceCombobox
                    referenceType="di.symptom"
                    label={t("fields.findingType")}
                    value={findingType}
                    onChange={setFindingType}
                    valueMode="code"
                    allowCreate={false}
                  />
                  <p className="text-xs text-text-muted">{t("hints.findingTypeFromCatalog")}</p>
                </div>
                <div className="grid gap-1">
                  <Label htmlFor="pm-exec-severity">{t("fields.severity")}</Label>
                  <Select value={severity} onValueChange={setSeverity}>
                    <SelectTrigger id="pm-exec-severity">
                      <SelectValue placeholder={t("labels.none")} />
                    </SelectTrigger>
                    <SelectContent>
                      {PM_FINDING_SEVERITIES.map((s) => (
                        <SelectItem key={s} value={s}>
                          {t(`severity.${s}`)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="grid gap-1">
                  <Label htmlFor="pm-exec-finding">{t("fields.findingDescription")}</Label>
                  <Textarea
                    id="pm-exec-finding"
                    rows={3}
                    value={findingDescription}
                    onChange={(e) => setFindingDescription(e.target.value)}
                  />
                </div>
                <label className="flex min-h-9 items-center gap-2 text-sm">
                  <Checkbox
                    checked={followUpDi}
                    onCheckedChange={(v) => setFollowUpDi(v === true)}
                  />
                  {t("actions.createFollowUpDi")}
                </label>
                <label className="flex min-h-9 items-center gap-2 text-sm">
                  <Checkbox
                    checked={followUpWo}
                    onCheckedChange={(v) => setFollowUpWo(v === true)}
                  />
                  {t("actions.createFollowUpWo")}
                </label>
              </>
            ) : null}
            {error ? (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {error}
              </p>
            ) : null}
          </div>
        ) : null}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("actions.cancel")}
          </Button>
          <Button disabled={saving || !result} onClick={() => void handleSubmit()}>
            {t("actions.logExecution")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
