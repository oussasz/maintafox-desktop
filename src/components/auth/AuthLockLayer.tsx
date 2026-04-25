import { Lock, ShieldAlert } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { mfAuth, mfInput } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { cn } from "@/lib/utils";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";

/**
 * Full-screen lock surface for centralized auth/permission recovery.
 *
 * Mount under the main shell so it can cover the workspace without unmounting routes
 * (preserving in-memory form state).
 */
export function AuthLockLayer() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const location = useLocation();
  const session = useSession();
  const isOpen = useAuthInterceptorStore((s) => s.isLockOpen);
  const mode = useAuthInterceptorStore((s) => s.mode);
  const lastFailure = useAuthInterceptorStore((s) => s.lastFailure);
  const preInterrupt = useAuthInterceptorStore((s) => s.preInterruptSnapshot);
  const clearLock = useAuthInterceptorStore((s) => s.clear);

  const [password, setPassword] = useState("");
  const passwordInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!isOpen) {
      setPassword("");
    }
  }, [isOpen]);

  // If the user explicitly navigates to the full sign-in page, don't keep a global lock on top of it.
  useEffect(() => {
    if (!isOpen) return;
    if (location.pathname === "/login") {
      clearLock();
    }
  }, [clearLock, isOpen, location.pathname]);

  const title = useMemo(() => {
    if (mode === "permission") {
      return t("interceptor.permission.title");
    }
    if (mode === "session") {
      return t("interceptor.session.title");
    }
    return t("interceptor.generic.title");
  }, [mode, t]);

  const message = useMemo(() => {
    if (mode === "permission") {
      return t("interceptor.permission.message");
    }
    if (mode === "session") {
      return t("interceptor.session.message");
    }
    return t("interceptor.generic.message");
  }, [mode, t]);

  const canTryPasswordResume =
    isOpen && (mode === "session" || mode === "unknown") && !session.info?.is_authenticated;

  useEffect(() => {
    if (!canTryPasswordResume) return;
    const id = window.setTimeout(() => passwordInputRef.current?.focus(), 30);
    return () => window.clearTimeout(id);
  }, [canTryPasswordResume, isOpen]);

  const displayName = preInterrupt?.display_name ?? preInterrupt?.username ?? null;

  const onSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      if (!canTryPasswordResume) return;
      try {
        const username = preInterrupt?.username?.trim();
        if (!username) {
          // We don't have a username to re-auth inline; send the user to the full login flow.
          navigate("/login", { replace: false });
          return;
        }
        await session.login({ username, password });
        clearLock();
      } catch {
        // `useSession` already captures a friendly error string for display.
      }
    },
    [canTryPasswordResume, clearLock, navigate, password, preInterrupt?.username, session],
  );

  if (!isOpen) {
    return null;
  }

  return (
    <div
      className="fixed inset-0 z-[2147483000] flex items-center justify-center p-6 pointer-events-auto"
      role="dialog"
      aria-modal="true"
      aria-labelledby="auth-lock-title"
      onPointerDownCapture={(e) => {
        // Keep all pointer interactions trapped inside lock layer while open.
        e.stopPropagation();
      }}
    >
      <div className="absolute inset-0 bg-surface-0/90 backdrop-blur-sm pointer-events-auto" />
      <div
        className={cn("relative z-[2147483001] w-full max-w-md pointer-events-auto", mfAuth.card)}
        onPointerDown={(e) => e.stopPropagation()}
      >
        <div className="mb-6 flex justify-center">
          <MaintafoxWordmark size="sm" tone="auth" />
        </div>

        <div className="mb-4 flex items-start gap-3">
          {mode === "permission" ? (
            <ShieldAlert className="mt-0.5 h-5 w-5 text-amber-700" aria-hidden="true" />
          ) : (
            <Lock className="mt-0.5 h-5 w-5 text-primary" aria-hidden="true" />
          )}
          <div className="min-w-0">
            <h1 id="auth-lock-title" className="text-lg font-semibold text-text-primary">
              {title}
            </h1>
            <p className="mt-1 text-sm text-text-secondary">{message}</p>
            {displayName ? (
              <p className="mt-3 text-sm text-text-primary">
                <span className="text-text-secondary">{t("interceptor.userLabel")}: </span>
                <span className="font-medium">{displayName}</span>
              </p>
            ) : null}
          </div>
        </div>

        {session.error ? (
          <div
            role="alert"
            className="mb-4 rounded-md bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
          >
            {session.error}
          </div>
        ) : null}

        {canTryPasswordResume && preInterrupt?.username ? (
          <form onSubmit={onSubmit} className="space-y-4">
            <div>
              <label htmlFor="auth-lock-password" className={mfInput.authLabel}>
                {t("login.form.password.label")}
              </label>
              <input
                ref={passwordInputRef}
                id="auth-lock-password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className={cn("w-full pointer-events-auto", mfInput.authField)}
                disabled={session.isLoading}
                required
              />
            </div>
            <Button
              type="submit"
              className="w-full"
              disabled={session.isLoading || password.trim().length === 0}
            >
              {session.isLoading ? t("login.form.submitting") : t("interceptor.actions.resume")}
            </Button>
          </form>
        ) : null}

        <div className="mt-4 flex flex-col gap-2">
          <Button
            type="button"
            variant="secondary"
            className="w-full"
            onClick={() => {
              // Full login can recover more edge cases; route change is explicit and user-driven.
              clearLock();
              navigate("/login", { replace: false });
            }}
          >
            {t("interceptor.actions.openLogin")}
          </Button>

          <div className="flex items-center justify-between gap-3 text-xs text-text-muted">
            <Link to="/" className="hover:text-text-secondary" onClick={() => clearLock()}>
              {t("interceptor.actions.backToApp")}
            </Link>
            {lastFailure?.command ? (
              <span className="truncate" title={lastFailure.message}>
                {t("interceptor.hints.command")}: {lastFailure.command}
              </span>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
