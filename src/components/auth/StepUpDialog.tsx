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
import { verifyStepUp } from "@/services/rbac-service";

interface StepUpDialogProps {
  open: boolean;
  onVerified: () => void;
  onCancel: () => void;
  title?: string;
  description?: string;
}

const MAX_ATTEMPTS = 3;
const LOCKOUT_SECONDS = 30;

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
  const [attempts, setAttempts] = useState(0);
  const [lockedUntil, setLockedUntil] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus on open
  useEffect(() => {
    if (open) {
      setPassword("");
      setError(null);
      setTimeout(() => inputRef.current?.focus(), 100);
    }
  }, [open]);

  // Lockout timer
  useEffect(() => {
    if (lockedUntil === null) return;
    const remaining = lockedUntil - Date.now();
    if (remaining <= 0) {
      setLockedUntil(null);
      return;
    }
    const timer = setTimeout(() => setLockedUntil(null), remaining);
    return () => clearTimeout(timer);
  }, [lockedUntil]);

  const isLocked = lockedUntil !== null && lockedUntil > Date.now();

  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      if (isLocked || loading) return;

      setError(null);
      setLoading(true);
      try {
        await verifyStepUp(password);
        setAttempts(0);
        onVerified();
      } catch {
        const newAttempts = attempts + 1;
        setAttempts(newAttempts);
        setPassword("");

        if (newAttempts >= MAX_ATTEMPTS) {
          setLockedUntil(Date.now() + LOCKOUT_SECONDS * 1000);
          setError(
            t("stepUp.locked", "Trop de tentatives. Réessayez dans {{seconds}} secondes.", {
              seconds: LOCKOUT_SECONDS,
            }),
          );
        } else {
          setError(t("stepUp.error", "Mot de passe incorrect. Veuillez réessayer."));
        }
      } finally {
        setLoading(false);
      }
    },
    [password, isLocked, loading, attempts, onVerified, t],
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
            <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-amber-100">
              <Lock className="h-5 w-5 text-amber-700" />
            </div>
            <div>
              <DialogTitle className="text-base">
                {title ?? t("stepUp.title", "Confirmation d'identité requise")}
              </DialogTitle>
              <DialogDescription className="text-sm">
                {description ??
                  t(
                    "stepUp.message",
                    "Cette action est sensible. Saisissez votre mot de passe pour confirmer.",
                  )}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="step-up-password" className="sr-only">
              {t("stepUp.passwordLabel", "Mot de passe")}
            </label>
            <Input
              ref={inputRef}
              id="step-up-password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("stepUp.passwordLabel", "Mot de passe")}
              disabled={isLocked || loading}
              required
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive"
            >
              {error}
            </div>
          )}

          <p className="text-xs text-muted-foreground">
            {t("stepUp.windowNote", "Fenêtre de vérification : 120 secondes")}
          </p>

          <DialogFooter>
            <Button type="button" variant="outline" size="sm" onClick={onCancel}>
              {t("stepUp.cancel", "Annuler")}
            </Button>
            <Button type="submit" size="sm" disabled={isLocked || loading}>
              {loading
                ? t("stepUp.submitting", "Vérification...")
                : t("stepUp.submit", "Confirmer l'identité")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
