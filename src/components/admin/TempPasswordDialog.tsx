import { Check, Copy } from "lucide-react";
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
import { mfAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export type TempPasswordDialogProps = {
  open: boolean;
  temporaryPassword: string | null;
  onClose: () => void;
};

/**
 * Show-once temporary password dialog. Parent must clear `temporaryPassword`
 * when `onClose` runs so the plaintext does not linger in React state.
 */
export function TempPasswordDialog({ open, temporaryPassword, onClose }: TempPasswordDialogProps) {
  const { t } = useTranslation("admin");
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);

  const handleCopy = async () => {
    if (!temporaryPassword) return;
    try {
      await navigator.clipboard.writeText(temporaryPassword);
      setCopyFailed(false);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      // Said, not swallowed: the user can still select the text and copy it by hand.
      setCopied(false);
      setCopyFailed(true);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          setCopied(false);
          onClose();
        }
      }}
    >
      <DialogContent
        className="max-w-md"
        data-testid="temp-password-dialog"
        // Shown once: Escape or a click outside must not throw the password away unseen.
        onEscapeKeyDown={(e) => e.preventDefault()}
        onPointerDownOutside={(e) => e.preventDefault()}
        onInteractOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{t("users.resetPassword.showOnceTitle")}</DialogTitle>
          <DialogDescription>{t("users.resetPassword.showOnceDescription")}</DialogDescription>
        </DialogHeader>

        {temporaryPassword ? (
          <div className="space-y-3 py-2">
            <p role="status" className={cn("text-sm", mfAlert.warning)}>
              {t("users.resetPassword.showOnceWarning")}
            </p>
            <div className="flex items-center gap-2 rounded-lg border border-surface-border bg-surface-1 px-3 py-2">
              <code
                className="flex-1 select-all break-all font-mono text-sm text-text-primary"
                data-testid="temp-password-value"
              >
                {temporaryPassword}
              </code>
              <Button
                type="button"
                variant="outline"
                className="h-10 w-10 p-0"
                onClick={() => void handleCopy()}
                aria-label={t("users.resetPassword.copy")}
                title={t("users.resetPassword.copy")}
              >
                {copied ? (
                  <Check className="h-4 w-4" aria-hidden />
                ) : (
                  <Copy className="h-4 w-4" aria-hidden />
                )}
              </Button>
            </div>
            <p role="status" className="min-h-4 text-xs text-text-muted">
              {copied ? t("users.resetPassword.copied") : null}
            </p>
            {copyFailed ? (
              <p role="alert" className="text-xs text-tone-warning-fg">
                {t("users.resetPassword.copyFailed")}
              </p>
            ) : null}
          </div>
        ) : null}

        <DialogFooter>
          <Button
            type="button"
            className="h-11"
            onClick={() => {
              setCopied(false);
              setCopyFailed(false);
              onClose();
            }}
          >
            {t("users.resetPassword.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
