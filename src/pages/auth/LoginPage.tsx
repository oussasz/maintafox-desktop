import { ArrowRight, Eye, EyeOff, Lock, ShieldAlert, ShieldCheck, User } from "lucide-react";
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
  const [showPassword, setShowPassword] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await session.login({ username: username.trim(), password });
      navigate("/", { replace: true });
    } catch {
      // Error captured in session.error
    }
  };

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-[#f4f6f9]">
      <div
        className="w-full max-w-[420px] rounded-2xl bg-white px-10 py-12 shadow-[0_4px_32px_0_rgba(0,0,0,0.08)]"
        style={{ animation: "login-fade-in 0.35s ease-out both" }}
      >
        {/* Logo + wordmark */}
        <div className="mb-8 flex items-center gap-3">
          <img src="/icons/maintafox.svg" alt="" className="h-9 w-9" />
          <span className="text-xl font-bold text-[#002a63]">Maintafox</span>
        </div>

        <h2 className="text-2xl font-bold text-gray-900">{t("login.welcome")}</h2>
        <p className="mt-1 text-sm text-gray-500">{t("login.subtitle")}</p>

        {/* Form */}
        <form onSubmit={handleSubmit} className="mt-8 space-y-5">
          {/* Username */}
          <div>
            <label
              htmlFor="login-username"
              className="mb-1.5 block text-sm font-medium text-gray-700"
            >
              {t("login.form.username.label")}
            </label>
            <div className="relative">
              <User className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
              <input
                id="login-username"
                type="text"
                autoComplete="username"
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder={t("login.form.username.placeholder")}
                className="w-full rounded-lg border border-gray-200 bg-gray-50 py-2.5 pl-10 pr-3
                           text-sm text-gray-900 placeholder:text-gray-400
                           focus:border-[#003d8f] focus:bg-white focus:outline-none
                           focus:ring-2 focus:ring-[#003d8f]/20 transition-all"
                disabled={session.isLoading}
              />
            </div>
          </div>

          {/* Password */}
          <div>
            <label
              htmlFor="login-password"
              className="mb-1.5 block text-sm font-medium text-gray-700"
            >
              {t("login.form.password.label")}
            </label>
            <div className="relative">
              <Lock className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
              <input
                id="login-password"
                type={showPassword ? "text" : "password"}
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("login.form.password.placeholder")}
                className="w-full rounded-lg border border-gray-200 bg-gray-50 py-2.5 pl-10 pr-10
                           text-sm text-gray-900 placeholder:text-gray-400
                           focus:border-[#003d8f] focus:bg-white focus:outline-none
                           focus:ring-2 focus:ring-[#003d8f]/20 transition-all"
                disabled={session.isLoading}
              />
              <button
                type="button"
                onClick={() => setShowPassword((v) => !v)}
                aria-label={
                  showPassword ? t("login.form.hidePassword") : t("login.form.showPassword")
                }
                tabIndex={-1}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-400
                           hover:text-gray-600 transition-colors"
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>
          </div>

          {/* Error */}
          {session.error && session.errorCode === "ACCOUNT_LOCKED" ? (
            <div
              role="alert"
              className="flex items-start gap-2 rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-700"
            >
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{session.error}</span>
            </div>
          ) : session.error ? (
            <div
              role="alert"
              className="rounded-lg border border-red-100 bg-red-50 px-4 py-3 text-sm text-red-600"
            >
              {session.error}
            </div>
          ) : null}

          {/* Submit */}
          <button
            type="submit"
            disabled={session.isLoading}
            className="flex w-full items-center justify-center gap-2 rounded-lg
                       bg-[#003d8f] py-2.5 text-sm font-semibold text-white
                       transition-colors hover:bg-[#002a63]
                       disabled:pointer-events-none disabled:opacity-50"
          >
            {session.isLoading ? (
              t("login.form.submitting")
            ) : (
              <>
                {t("login.form.submit")}
                <ArrowRight className="h-4 w-4" />
              </>
            )}
          </button>
        </form>

        {/* Security footer */}
        <div className="mt-6 flex items-center justify-center gap-1.5 text-xs text-gray-400">
          <ShieldCheck className="h-3.5 w-3.5" />
          <span>{t("login.security.label")}</span>
        </div>

        {/* Locale switcher */}
        <div className="mt-4 flex justify-center gap-2">
          {supportedLocales.map((loc) => (
            <button
              key={loc}
              onClick={() => {
                void i18n.changeLanguage(loc);
                useLocaleStore.setState({ activeLocale: loc });
              }}
              className={cn(
                "rounded-full px-3 py-1 text-xs font-medium transition-colors",
                loc === activeLocale
                  ? "bg-[#003d8f] text-white"
                  : "text-gray-400 hover:bg-gray-100 hover:text-gray-600",
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
