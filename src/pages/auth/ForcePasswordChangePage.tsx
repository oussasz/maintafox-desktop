import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

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
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm">
        <div className="mb-6 text-center">
          <h1 className="text-2xl font-bold text-primary">Maintafox</h1>
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
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
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
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
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

          <button
            type="submit"
            disabled={loading}
            className="btn-primary w-full py-2 text-sm font-medium"
          >
            {loading ? t("login.form.submitting") : t("session.forcePasswordChange.submit")}
          </button>
        </form>
      </div>
    </div>
  );
}
