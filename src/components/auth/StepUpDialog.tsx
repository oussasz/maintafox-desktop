import { Lock } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
 * Verifies the user's password for dangerous actions that require elevated confirmation.
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
      // Don't reset attempts on re-open — keep the lockout state
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

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div
        className="w-full max-w-sm rounded-lg border border-surface-border bg-surface-0 p-6 shadow-lg"
        // biome-ignore lint/a11y/useSemanticElements: custom overlay dialog
        role="dialog"
        aria-modal="true"
      >
        <div className="flex items-center gap-3 mb-4">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-amber-100">
            <Lock className="h-5 w-5 text-amber-700" />
          </div>
          <div>
            <h3 className="text-base font-semibold text-text-primary">
              {title ?? t("stepUp.title", "Confirmation d'identité requise")}
            </h3>
            <p className="text-sm text-text-secondary">
              {description ??
                t(
                  "stepUp.message",
                  "Cette action est sensible. Saisissez votre mot de passe pour confirmer.",
                )}
            </p>
          </div>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="step-up-password" className="sr-only">
              {t("stepUp.passwordLabel", "Mot de passe")}
            </label>
            <input
              ref={inputRef}
              id="step-up-password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("stepUp.passwordLabel", "Mot de passe")}
              disabled={isLocked || loading}
              required
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary
                         disabled:opacity-50"
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
            >
              {error}
            </div>
          )}

          <p className="text-xs text-text-muted">
            {t("stepUp.windowNote", "Fenêtre de vérification : 120 secondes")}
          </p>

          <div className="flex justify-end gap-2">
            <button type="button" onClick={onCancel} className="btn-ghost text-sm">
              {t("stepUp.cancel", "Annuler")}
            </button>
            <button type="submit" disabled={isLocked || loading} className="btn-primary text-sm">
              {loading
                ? t("stepUp.submitting", "Vérification...")
                : t("stepUp.submit", "Confirmer l'identité")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
