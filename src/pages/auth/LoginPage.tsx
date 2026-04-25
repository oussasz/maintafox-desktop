import { ArrowRight, Eye, EyeOff, Info, Lock, ShieldAlert, ShieldCheck, User } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { useProductLicenseGateRefresh } from "@/contexts/product-license-gate-context";
import { mfAlert, mfAuth, mfInput } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { i18n } from "@/i18n";
import { cn } from "@/lib/utils";
import {
  clearProductLicenseBrowserState,
  getActivationLicenseMetadata,
  getProductLicenseOnboardingState,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  resetProductLicenseActivation,
  type ActivationLicenseMetadata,
} from "@/services/product-license-service";
import { useLocaleStore } from "@/stores/locale-store";

export function LoginPage() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const session = useSession();
  const { activeLocale, supportedLocales } = useLocaleStore();

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [postActivationHint, setPostActivationHint] = useState(false);
  const [showLicenseMeta, setShowLicenseMeta] = useState(false);
  const [licenseMeta, setLicenseMeta] = useState<ActivationLicenseMetadata | null>(null);
  const [activationReady, setActivationReady] = useState(false);
  const [activationCheckLoading, setActivationCheckLoading] = useState(true);
  const [resettingActivation, setResettingActivation] = useState(false);
  const [resetActivationError, setResetActivationError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const refreshProductLicense = useProductLicenseGateRefresh();

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const tenantScopeDenied = session.errorCode === "TENANT_SCOPE_VIOLATION";
  const staleSessionClaims = session.errorCode === "SESSION_CLAIM_INVALID";

  const revalidateActivation = useCallback(async () => {
    setActivationCheckLoading(true);
    try {
      const s = await getProductLicenseOnboardingState();
      const ready = Boolean(s.complete && s.tenant_id && s.tenant_id.trim().length > 0);
      setActivationReady(ready);
    } catch {
      setActivationReady(false);
    } finally {
      setActivationCheckLoading(false);
    }
  }, []);

  useEffect(() => {
    void revalidateActivation();
  }, [revalidateActivation]);

  useEffect(() => {
    void (async () => {
      const meta = await getActivationLicenseMetadata();
      setLicenseMeta(meta);
    })();
  }, []);

  useEffect(() => {
    if (typeof sessionStorage === "undefined") return;
    if (sessionStorage.getItem(POST_ACTIVATION_LOGIN_HINT_KEY) === "1") {
      sessionStorage.removeItem(POST_ACTIVATION_LOGIN_HINT_KEY);
      setPostActivationHint(true);
    }
  }, []);

  const handleResetActivation = async () => {
    setResettingActivation(true);
    setResetActivationError(null);
    try {
      await resetProductLicenseActivation();
      clearProductLicenseBrowserState();
      await session.refresh();
      if (refreshProductLicense) {
        await refreshProductLicense({ includeDiagnostics: false });
      }
      navigate("/", { replace: true });
    } catch (e) {
      const message =
        e instanceof Error
          ? e.message
          : typeof e === "object" && e !== null && "message" in e
            ? String((e as { message: unknown }).message)
            : "Could not reset activation. Try again, or restart the app.";
      setResetActivationError(message);
    } finally {
      if (mountedRef.current) {
        setResettingActivation(false);
      }
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (!activationReady || activationCheckLoading) return;
    try {
      await session.login({ username: username.trim(), password });
      navigate("/", { replace: true });
    } catch {
      // Error captured in session.error
    }
  };

  const credentialsDisabled =
    session.isLoading || activationCheckLoading || !activationReady || resettingActivation;

  return (
    <div className={mfAuth.shell}>
      <div className={mfAuth.card}>
        {/* Logo + wordmark */}
        <div className="mb-8">
          <MaintafoxWordmark size="lg" tone="auth" />
        </div>

        <h2 className="text-2xl font-bold text-text-primary">{t("login.welcome")}</h2>
        <p className="mt-1 text-sm text-text-secondary">{t("login.subtitle")}</p>
        <button
          type="button"
          onClick={() => setShowLicenseMeta((v) => !v)}
          className={cn("mt-3 inline-flex items-center gap-2", mfAuth.linkPrimary)}
        >
          <Info className="h-4 w-4" />
          {showLicenseMeta ? "Hide license details" : "View license details"}
        </button>
        {showLicenseMeta ? (
          <div className={cn("mt-3", mfAlert.info)}>
            <p>
              <span className="font-medium">License tier:</span>{" "}
              {licenseMeta?.license_tier ?? "n/a"}
            </p>
            <p>
              <span className="font-medium">Slot limit:</span> {licenseMeta?.device_limit ?? "n/a"}
            </p>
            <p>
              <span className="font-medium">Expiry date:</span> {licenseMeta?.expires_at ?? "n/a"}
            </p>
            <p>
              <span className="font-medium">Company:</span>{" "}
              {licenseMeta?.company_display_name ?? licenseMeta?.tenant_id ?? "n/a"}
            </p>
          </div>
        ) : null}

        {activationCheckLoading ? (
          <div className="mt-6 flex items-center gap-2 text-sm text-text-muted">
            <span className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-surface-border border-t-primary" />
            Vérification de l&apos;activation de l&apos;appareil…
          </div>
        ) : !activationReady ? (
          <div role="alert" className={cn("mt-6", mfAlert.warning)}>
            <p className="font-medium">Activation requise</p>
            <p className="mt-1">
              Les informations de licence sur cet appareil sont incomplètes ou corrompues.
              Réinitialisez l&apos;activation pour saisir une clé valide.
            </p>
            <button
              type="button"
              onClick={() => void handleResetActivation()}
              disabled={resettingActivation}
              className="btn-primary mt-3 w-full justify-center py-2"
            >
              {resettingActivation
                ? "Réinitialisation…"
                : "Réinitialiser l&apos;activation / utiliser une autre clé"}
            </button>
          </div>
        ) : null}

        {postActivationHint && activationReady ? (
          <div role="status" className={cn("mt-4", mfAlert.info)}>
            Please contact your Administrator for account credentials.
          </div>
        ) : null}

        {/* Form */}
        <form onSubmit={handleSubmit} className="mt-8 space-y-5">
          {/* Username */}
          <div>
            <label htmlFor="login-username" className={mfInput.authLabel}>
              {t("login.form.username.label")}
            </label>
            <div className="relative">
              <User className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
              <input
                id="login-username"
                type="text"
                autoComplete="username"
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder={t("login.form.username.placeholder")}
                className={mfInput.authField}
                disabled={credentialsDisabled}
              />
            </div>
          </div>

          {/* Password */}
          <div>
            <label htmlFor="login-password" className={mfInput.authLabel}>
              {t("login.form.password.label")}
            </label>
            <div className="relative">
              <Lock className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
              <input
                id="login-password"
                type={showPassword ? "text" : "password"}
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder={t("login.form.password.placeholder")}
                className={mfInput.authFieldWithTrailing}
                disabled={credentialsDisabled}
              />
              <button
                type="button"
                onClick={() => setShowPassword((v) => !v)}
                aria-label={
                  showPassword ? t("login.form.hidePassword") : t("login.form.showPassword")
                }
                tabIndex={-1}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted transition-colors hover:text-text-secondary"
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>
          </div>

          {/* Error */}
          {session.error && session.errorCode === "ACCOUNT_LOCKED" ? (
            <div role="alert" className={cn("flex items-start gap-2", mfAlert.warning)}>
              <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{session.error}</span>
            </div>
          ) : session.error && tenantScopeDenied ? (
            <div role="alert" className={mfAlert.warning}>
              <p className="font-medium">Compte non autorisé pour le tenant activé</p>
              <p className="mt-1">{session.error}</p>
              <p className="mt-1 text-xs">
                Action: utilisez un compte autorisé pour ce tenant ou réactivez l&apos;appareil avec
                la clé du bon tenant.
              </p>
            </div>
          ) : session.error && staleSessionClaims ? (
            <div role="alert" className={mfAlert.info}>
              <p className="font-medium">Contexte de session périmé</p>
              <p className="mt-1">{session.error}</p>
              <p className="mt-1 text-xs">
                Action: reconnectez-vous pour rafraîchir les claims tenant après un changement
                d&apos;activation.
              </p>
              <button
                type="button"
                onClick={() => void handleResetActivation()}
                disabled={resettingActivation}
                className="mt-3 w-full rounded-lg border border-surface-border bg-surface-1 px-4 py-2 text-sm font-semibold text-text-primary transition-colors hover:bg-surface-2 disabled:opacity-50"
              >
                {resettingActivation
                  ? "Réinitialisation…"
                  : "Réinitialiser l&apos;activation / utiliser une autre clé"}
              </button>
            </div>
          ) : session.error ? (
            <div role="alert" className={mfAlert.danger}>
              {session.error}
            </div>
          ) : null}

          {/* Submit */}
          <Button
            type="submit"
            className="w-full gap-2 py-2.5 font-semibold"
            disabled={credentialsDisabled}
          >
            {session.isLoading ? (
              t("login.form.submitting")
            ) : (
              <>
                {t("login.form.submit")}
                <ArrowRight className="h-4 w-4" />
              </>
            )}
          </Button>
        </form>

        <div className="mt-5 border-t border-surface-border pt-5 text-center">
          <p className="mb-2 text-xs text-text-muted">
            Wrong tenant or need to enter a new license key? This clears local license data and
            returns you to product activation.
          </p>
          {resetActivationError ? (
            <p role="alert" className={cn("mb-2 text-xs", mfAlert.danger)}>
              {resetActivationError}
            </p>
          ) : null}
          <button
            type="button"
            onClick={() => void handleResetActivation()}
            disabled={resettingActivation || activationCheckLoading}
            className="text-sm font-semibold text-primary underline-offset-2 hover:underline disabled:opacity-50"
          >
            {resettingActivation
              ? "Réinitialisation…"
              : "Réinitialiser l&apos;activation / autre clé"}
          </button>
        </div>

        {/* Security footer */}
        <div className="mt-6 flex items-center justify-center gap-1.5 text-xs text-text-muted">
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
                  ? "bg-primary text-primary-foreground"
                  : "text-text-muted hover:bg-surface-3 hover:text-text-secondary",
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
