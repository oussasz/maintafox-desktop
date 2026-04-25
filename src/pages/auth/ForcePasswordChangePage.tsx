import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { mfAuth } from "@/design-system/tokens";

interface ForcePasswordChangePageProps {
  onComplete: (newPassword: string) => Promise<void>;
}

export function ForcePasswordChangePage({ onComplete }: ForcePasswordChangePageProps) {
  const { t } = useTranslation("auth");
  const [newPassword, setNewPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);

    if (newPassword.length < 8) {
      setError(t("session.forcePasswordChange.newPassword") + " \u2014 min. 8");
      return;
    }

    if (newPassword !== confirmPassword) {
      setError(t("session.forcePasswordChange.confirmPassword"));
      return;
    }

    setLoading(true);
    try {
      await onComplete(newPassword);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("login.error.unknown"));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={mfAuth.shell}>
      <div className={mfAuth.cardCompact}>
        <div className="mb-8 flex justify-center">
          <MaintafoxWordmark size="lg" align="center" tone="auth" />
        </div>

        <div className="mb-6">
          <h2 className="text-lg font-semibold text-text-primary">
            {t("session.forcePasswordChange.title")}
          </h2>
          <p className="mt-1 text-sm text-text-secondary">
            {t("session.forcePasswordChange.message")}
          </p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label
              htmlFor="new-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("session.forcePasswordChange.newPassword")}
            </label>
            <input
              id="new-password"
              type="password"
              autoComplete="new-password"
              autoFocus
              required
              value={newPassword}
              onChange={(e) => setNewPassword(e.target.value)}
              className="field-input"
              disabled={loading}
            />
          </div>

          <div>
            <label
              htmlFor="confirm-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("session.forcePasswordChange.confirmPassword")}
            </label>
            <input
              id="confirm-password"
              type="password"
              autoComplete="new-password"
              required
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              className="field-input"
              disabled={loading}
            />
          </div>

          {error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm
                         text-status-danger"
            >
              {error}
            </div>
          )}

          <Button type="submit" className="w-full font-medium" disabled={loading}>
            {loading ? t("login.form.submitting") : t("session.forcePasswordChange.submit")}
          </Button>
        </form>
      </div>
    </div>
  );
}
