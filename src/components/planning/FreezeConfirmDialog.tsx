import { useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
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
import { P } from "@shared/rbac/permissions.generated";

type FreezeConfirmDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  periodStartLabel: string;
  periodEndLabel: string;
  saving: boolean;
  onConfirm: (reason: string) => Promise<void>;
};

export function FreezeConfirmDialog({
  open,
  onOpenChange,
  periodStartLabel,
  periodEndLabel,
  saving,
  onConfirm,
}: FreezeConfirmDialogProps) {
  const { t } = useTranslation("planning");
  const [reason, setReason] = useState("");

  async function handleConfirm() {
    const trimmed = reason.trim();
    if (!trimmed) return;
    await onConfirm(trimmed);
    setReason("");
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("freeze.title")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3 py-2">
          <p className="text-sm text-text-secondary">
            {t("freeze.summary", { start: periodStartLabel, end: periodEndLabel })}
          </p>
          <div>
            <Label>{t("freeze.reason")}</Label>
            <Textarea
              className="min-h-[80px] text-sm"
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder={t("freeze.reasonPlaceholder")}
            />
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
              disabled={!reason.trim() || saving}
              onClick={() => void handleConfirm()}
            >
              {t("actions.freezePeriod")}
            </Button>
          </PermissionGate>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
