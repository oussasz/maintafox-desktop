import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";

type OverrideReasonDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title?: string;
  description?: string;
  /** The assignee refusals being overridden (`PLANNING_ASSIGNEE_*`), shown before confirming. */
  issues?: string[];
  /** Context shown above the reason (e.g. the technician's days). */
  children?: ReactNode;
  onConfirm: (reason: string) => void;
};

export function OverrideReasonDialog({
  open,
  onOpenChange,
  title,
  description,
  issues = [],
  children,
  onConfirm,
}: OverrideReasonDialogProps) {
  const { t } = useTranslation("planning");
  const [reason, setReason] = useState("");

  function handleConfirm() {
    const trimmed = reason.trim();
    if (!trimmed) return;
    onConfirm(trimmed);
    setReason("");
    onOpenChange(false);
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setReason("");
        onOpenChange(next);
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>
            {title ?? (issues.length > 0 ? t("override.assigneeTitle") : t("override.title"))}
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-3 py-2">
          <p className="text-sm text-text-secondary">
            {description ??
              (issues.length > 0 ? t("override.assigneeDescription") : t("override.description"))}
          </p>
          {issues.length > 0 ? (
            <ul className="list-disc space-y-0.5 pl-5 text-xs text-tone-attention-fg" role="status">
              {issues.map((code) => (
                <li key={code}>{t(`assignee.issues.${code}`)}</li>
              ))}
            </ul>
          ) : null}
          {children}
          <div>
            <Label>{t("override.reason")}</Label>
            <Textarea
              className="min-h-[80px] text-sm"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder={t("gantt.overrideReasonPrompt")}
            />
          </div>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("dialogs.cancel")}
          </Button>
          <Button type="button" size="sm" disabled={!reason.trim()} onClick={handleConfirm}>
            {t("override.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
