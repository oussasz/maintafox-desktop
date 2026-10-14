/**
 * Confirmation for archive actions that the backend records with a mandatory reason
 * (restore request, legal hold, purge). Same behaviour as `ConfirmDialog` — named action,
 * buttons disabled while running, closes only when the action resolves, stays open on failure
 * (the global IPC toast reports it) — plus the reason field `ConfirmDialog` has no slot for.
 */
import { type ReactNode, useEffect, useId, useState } from "react";
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
import { Textarea } from "@/components/ui/textarea";

export interface ArchiveReasonDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description: string;
  confirmLabel: string;
  destructive?: boolean;
  /** Extra content above the reason (e.g. the purge eligibility checks). */
  children?: ReactNode;
  /** Disables the confirm button with this visible reason. */
  blockedReason?: string | null;
  onConfirm: (reason: string) => Promise<void>;
}

export function ArchiveReasonDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  destructive = false,
  children,
  blockedReason,
  onConfirm,
}: ArchiveReasonDialogProps) {
  const { t } = useTranslation("archive");
  const [reason, setReason] = useState("");
  const [pending, setPending] = useState(false);
  const fieldId = useId();

  useEffect(() => {
    if (open) setReason("");
  }, [open]);

  const run = async () => {
    setPending(true);
    try {
      await onConfirm(reason.trim());
      onOpenChange(false);
    } catch {
      // Reported by the global toast (or the user cancelled the password check).
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription className="text-sm text-text-secondary">
            {description}
          </DialogDescription>
        </DialogHeader>
        {children}
        <div className="space-y-1">
          <label htmlFor={fieldId} className="text-sm font-medium text-text-primary">
            {t("dialog.reason")}
          </label>
          <Textarea
            id={fieldId}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder={t("dialog.reasonPlaceholder")}
            disabled={pending}
          />
        </div>
        {blockedReason ? <p className="text-sm text-text-danger">{blockedReason}</p> : null}
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("dialog.cancel")}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            className="h-11"
            onClick={() => void run()}
            disabled={pending || !reason.trim() || Boolean(blockedReason)}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
