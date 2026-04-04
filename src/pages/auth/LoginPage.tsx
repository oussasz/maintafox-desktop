import { type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { useSession } from "@/hooks/use-session";
import { i18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { useLocaleStore } from "@/stores/locale-store";

export function LoginPage() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const session = useSession();
  const { activeLocale, supportedLocales } = useLocaleStore();

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await session.login({ username: username.trim(), password });
      navigate("/", { replace: true });
    } catch {
      // Error is captured in session.error by the hook
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface-0 px-4">
      <div className="w-full max-w-sm">
        {/* Brand header */}
        <div className="mb-8 text-center">
          <h1 className="text-2xl font-bold text-primary">Maintafox</h1>
          <p className="mt-1 text-sm text-text-secondary">{t("login.subtitle")}</p>
        </div>

        {/* Login form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          {/* Username */}
          <div>
            <label
              htmlFor="login-username"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("login.form.username.label")}
            </label>
            <input
              id="login-username"
              type="text"
              autoComplete="username"
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={t("login.form.username.placeholder")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={session.isLoading}
            />
          </div>

          {/* Password */}
          <div>
            <label
              htmlFor="login-password"
              className="mb-1 block text-sm font-medium text-text-primary"
            >
              {t("login.form.password.label")}
            </label>
            <input
              id="login-password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={t("login.form.password.placeholder")}
              className="w-full rounded-md border border-surface-border bg-surface-1
                         px-3 py-2 text-sm text-text-primary
                         placeholder:text-text-muted
                         focus:border-primary focus:outline-none focus:ring-1
                         focus:ring-primary"
              disabled={session.isLoading}
            />
          </div>

          {/* Error display */}
          {session.error && (
            <div
              role="alert"
              className="rounded-md bg-status-danger/10 px-3 py-2 text-sm
                         text-status-danger"
            >
              {session.error}
            </div>
          )}

          {/* Submit */}
          <button
            type="submit"
            disabled={session.isLoading}
            className="btn-primary w-full py-2 text-sm font-medium"
          >
            {session.isLoading ? t("login.form.submitting") : t("login.form.submit")}
          </button>
        </form>

        {/* Locale switcher */}
        <div className="mt-6 flex justify-center gap-2">
          {supportedLocales.map((loc) => (
            <button
              key={loc}
              onClick={() => {
                // Switch language client-side only — no session exists yet
                // to persist via IPC. After login, initialize() re-reads backend.
                void i18n.changeLanguage(loc);
                useLocaleStore.setState({ activeLocale: loc });
              }}
              className={cn(
                "rounded px-3 py-1 text-xs font-medium transition-colors",
                loc === activeLocale
                  ? "bg-primary text-white"
                  : "text-text-secondary hover:bg-surface-2",
              )}
            >
              {loc.toUpperCase()}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
