import { Lock } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
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
import { Input } from "@/components/ui/input";
import { accountLockedText, lockoutUntil } from "@/lib/account-lockout";
import { describeAppError } from "@/lib/app-error";
import { verifyStepUp } from "@/services/rbac-service";
import { ACCOUNT_LOCKED, useSessionStore } from "@/store/session-store";
import { extractIpcErrorCode } from "@/utils/errors";

interface StepUpDialogProps {
  open: boolean;
  onVerified: () => void;
  onCancel: () => void;
  title?: string;
  description?: string;
}

/**
 * Reusable step-up re-authentication dialog.
 * Uses Radix Dialog so it properly stacks above other dialogs.
 */
export function StepUpDialog({
  open,
  onVerified,
  onCancel,
  title,
  description,
}: StepUpDialogProps) {
  const { t } = useTranslation("auth");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [lockedOut, setLockedOut] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus on open
  useEffect(() => {
    if (open) {
      setPassword("");
      setError(null);
      setLockedOut(false);
      setTimeout(() => inputRef.current?.focus(), 100);
    }
  }, [open]);

  // The attempt limit is the server's (the sign-in account lockout): no count of its own here.
  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      if (lockedOut || loading) return;

      setError(null);
      setLoading(true);
      try {
        await verifyStepUp(password);
        setPassword("");
        onVerified();
      } catch (err) {
        setPassword("");
        const code = extractIpcErrorCode(err);
        if (code === ACCOUNT_LOCKED) {
          // Too many wrong passwords: the account is locked and the server ended the session.
          // The lockout and its end, in the user's words, show here and on sign-in.
          const until = lockoutUntil(err);
          setLockedOut(true);
          setError(accountLockedText(until));
          useSessionStore.getState().applyAccountLockout(until);
          onCancel();
        } else if (code === "AUTH_ERROR") {
          setError(t("stepUp.error"));
        } else {
          setError(describeAppError(err).title);
        }
      } finally {
        setLoading(false);
      }
    },
    [password, lockedOut, loading, onVerified, onCancel, t],
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(isOpen) => {
        if (!isOpen) onCancel();
      }}
    >
      <DialogContent
        className="max-w-sm"
        onPointerDownOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-tone-warning-bg">
              <Lock className="h-5 w-5 text-tone-warning-fg" aria-hidden />
            </div>
            <div>
              <DialogTitle className="text-base">{title ?? t("stepUp.title")}</DialogTitle>
              <DialogDescription className="text-sm">
                {description ?? t("stepUp.message")}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="step-up-password" className="sr-only">
              {t("stepUp.passwordLabel")}
            </label>
            <Input
              ref={inputRef}
              id="step-up-password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("stepUp.passwordLabel")}
              disabled={lockedOut || loading}
              required
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
            >
              {error}
            </div>
          )}

          <p className="text-xs text-text-muted">{t("stepUp.windowNote")}</p>

          <DialogFooter>
            <Button type="button" variant="outline" size="sm" onClick={onCancel}>
              {t("stepUp.cancel")}
            </Button>
            <Button type="submit" size="sm" disabled={lockedOut || loading}>
              {loading ? t("stepUp.submitting") : t("stepUp.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
