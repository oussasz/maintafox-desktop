/**
 * WoCancelDialog.tsx
 *
 * Confirm cancel for a work order. Requires a non-empty cancel reason.
 */

import { AlertTriangle, XCircle } from "lucide-react";
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
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { useWoStore } from "@/stores/wo-store";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrder } from "@shared/ipc-types";

interface WoCancelDialogProps {
  wo: WorkOrder | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function WoCancelDialog({ wo, open, onOpenChange }: WoCancelDialogProps) {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const cancelWorkOrder = useWoStore((s) => s.cancelWorkOrder);
  const saving = useWoStore((s) => s.saving);

  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setReason("");
      setError(null);
    }
  }, [open, wo?.id]);

  const handleSubmit = useCallback(async () => {
    if (!wo || !info?.user_id) return;
    const trimmed = reason.trim();
    if (!trimmed) {
      setError(t("cancel.reasonRequired"));
      return;
    }
    setError(null);
    try {
      await cancelWorkOrder({
        id: wo.id,
        actor_id: info.user_id,
        expected_row_version: wo.row_version,
        cancel_reason: trimmed,
      });
      onOpenChange(false);
    } catch (err) {
      setError(toErrorMessage(err));
    }
  }, [wo, info?.user_id, reason, cancelWorkOrder, onOpenChange, t]);

  if (!wo) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <XCircle className="h-5 w-5 text-destructive" />
            {t("cancel.title")}
          </DialogTitle>
          <DialogDescription>
            <span className="font-mono text-xs">{wo.code}</span>
            {" — "}
            {wo.title}
          </DialogDescription>
        </DialogHeader>

        <Separator />

        <div className="space-y-3 py-2">
          <p className="text-sm text-muted-foreground">{t("cancel.warning")}</p>
          <div className="space-y-1.5">
            <Label className="text-xs">{t("cancel.reason")}</Label>
            <Textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
              placeholder={t("cancel.reasonPlaceholder")}
              rows={4}
              className="text-sm resize-none"
            />
          </div>
          {error && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/5 p-2.5 text-sm text-destructive">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}
        </div>

        <Separator />

        <div className="flex items-center justify-end gap-2 pt-1">
          <Button variant="outline" size="sm" onClick={() => onOpenChange(false)}>
            {t("form.cancel")}
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => void handleSubmit()}
            disabled={saving || !reason.trim()}
            className="gap-1.5"
          >
            <XCircle className="h-3.5 w-3.5" />
            {t("cancel.submit")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
