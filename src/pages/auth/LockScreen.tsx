import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";

interface LockScreenProps {
  displayName: string | null;
  onUnlock: (password: string) => Promise<void>;
  onLogout: () => void;
}

export function LockScreen({ displayName, onUnlock, onLogout }: LockScreenProps) {
  const { t } = useTranslation("auth");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      await onUnlock(password);
    } catch (err) {
      setError(err instanceof Error ? err.message : t("session.idleLocked.unlockAction"));
      setPassword("");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm text-center">
        {/* User avatar */}
        <div
          className="mx-auto mb-4 flex h-16 w-16 items-center justify-center
                     rounded-full bg-primary text-2xl font-bold text-white"
        >
          {displayName ? displayName.charAt(0).toUpperCase() : "?"}
        </div>

        <h2 className="text-lg font-semibold text-text-primary">{t("session.idleLocked.title")}</h2>
        <p className="mt-1 text-sm text-text-secondary">{t("session.idleLocked.message")}</p>

        {displayName && <p className="mt-2 text-sm font-medium text-text-primary">{displayName}</p>}

        <form onSubmit={handleSubmit} className="mt-6 space-y-4">
          <div>
            <label htmlFor="lock-password" className="sr-only">
              {t("session.idleLocked.unlockPrompt")}
            </label>
            <input
              id="lock-password"
              type="password"
              autoComplete="current-password"
              autoFocus
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("session.idleLocked.unlockPrompt")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary text-center
                         placeholder:text-text-muted
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
            {loading ? t("session.idleLocked.unlocking") : t("session.idleLocked.unlockAction")}
          </button>
        </form>

        {/* Sign out link */}
        <button
          onClick={onLogout}
          className="mt-4 text-xs text-text-muted hover:text-text-secondary
                     transition-colors"
        >
          {t("logout.label")}
        </button>
      </div>
    </div>
  );
}
