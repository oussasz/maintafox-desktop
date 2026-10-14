/**
 * ConfirmDialog — the app's confirmation step for destructive or irreversible actions
 * (delete, deactivate, cancel, archive). Replaces `window.confirm`.
 *
 * The confirm button names the action ("Deactivate article"), never "OK". While the action runs
 * the buttons are disabled; the dialog closes only when `onConfirm` resolves. A rejected
 * `onConfirm` keeps the dialog open (the global toast reports the failure).
 */
import { useState } from "react";
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

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title?: string;
  /** What will happen, in one or two plain sentences. */
  description: string;
  /** Verb naming the action, e.g. "Deactivate article". */
  confirmLabel: string;
  destructive?: boolean;
  onConfirm: () => Promise<void> | void;
}

export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  destructive = false,
  onConfirm,
}: ConfirmDialogProps) {
  const { t } = useTranslation("common");
  const [pending, setPending] = useState(false);

  const run = async () => {
    setPending(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } catch {
      // The global IPC toast reports the failure; keep the dialog open so the user can retry.
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !pending && onOpenChange(next)}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{title ?? t("ds.confirm.title")}</DialogTitle>
          <DialogDescription className="text-sm text-text-secondary">
            {description}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={pending}
          >
            {t("ds.confirm.cancel")}
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            className="h-11"
            onClick={() => void run()}
            disabled={pending}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
