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

  const handleCopy = async () => {
    if (!temporaryPassword) return;
    try {
      await navigator.clipboard.writeText(temporaryPassword);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
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
      <DialogContent className="max-w-md" data-testid="temp-password-dialog">
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
                className="flex-1 break-all font-mono text-sm text-text-primary"
                data-testid="temp-password-value"
              >
                {temporaryPassword}
              </code>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleCopy()}
                aria-label={t("users.resetPassword.copy")}
              >
                {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
              </Button>
            </div>
            {copied ? (
              <p className="text-xs text-text-muted">{t("users.resetPassword.copied")}</p>
            ) : null}
          </div>
        ) : null}

        <DialogFooter>
          <Button
            type="button"
            onClick={() => {
              setCopied(false);
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
