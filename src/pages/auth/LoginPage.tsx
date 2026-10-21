import { ArrowRight, Eye, EyeOff, Info, Loader2, Lock, User, UserPlus } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { AdminRecoveryCodeDialog } from "@/components/auth/AdminRecoveryCodeDialog";
import { ForgotPasswordDialog } from "@/components/auth/ForgotPasswordDialog";
import { formatLicenseDate } from "@/components/auth/activation/OrganizationLicenseCard";
import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton, AuthStatusChip } from "@/components/auth/shell/AuthPrimitives";
import { AuthShell } from "@/components/auth/shell/AuthShell";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  useProductLicenseGate,
  useProductLicenseGateReconcile,
  useProductLicenseGateRefresh,
} from "@/contexts/product-license-gate-context";
import { mfAuth, mfAuthAlert } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { i18n } from "@/i18n";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  clearProductLicenseBrowserState,
  getActivationBootstrapState,
  getActivationLicenseMetadata,
  getProductLicenseOnboardingState,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  resetProductLicenseActivation,
  type ActivationLicenseMetadata,
} from "@/services/product-license-service";

export function LoginPage() {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const session = useSession();
  const licenseGate = useProductLicenseGate();
  const refreshProductLicense = useProductLicenseGateRefresh();
  const reconcileProductLicense = useProductLicenseGateReconcile();

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [forgotOpen, setForgotOpen] = useState(false);
  const [recoveryCodeOpen, setRecoveryCodeOpen] = useState(false);
  const [postActivationHint, setPostActivationHint] = useState(false);
  const [showLicenseMeta, setShowLicenseMeta] = useState(false);
  const [licenseMeta, setLicenseMeta] = useState<ActivationLicenseMetadata | null>(null);
  const [activationReady, setActivationReady] = useState(false);
  const [activationCheckLoading, setActivationCheckLoading] = useState(true);
  /**
   * The PC is activated and has no administrator yet (`get_activation_bootstrap_state`): no named
   * administrator was ever created here, so the first-start screen is still open. Also true on a
   * PC whose only administrator is the built-in "admin" account with its password never changed.
   * The backend alone decides whether the creation is allowed (`bootstrap_initial_tenant_admin`).
   */
  const [firstAdminPending, setFirstAdminPending] = useState(false);
  const [resettingActivation, setResettingActivation] = useState(false);
  const [resetActivationError, setResetActivationError] = useState<string | null>(null);
  const [confirmResetOpen, setConfirmResetOpen] = useState(false);
  const [capsLockOn, setCapsLockOn] = useState(false);
  const usernameRef = useRef<HTMLInputElement | null>(null);
  const mountedRef = useRef(true);

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
      let pending = false;
      if (ready) {
        try {
          const bootstrap = await getActivationBootstrapState();
          pending = Boolean(bootstrap.tenant_id?.trim()) && !bootstrap.has_tenant_admin;
        } catch {
          // No answer: offer nothing; the sign-in form stays available.
        }
      }
      setFirstAdminPending(pending);
    } catch {
      setActivationReady(false);
      setFirstAdminPending(false);
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
      setResetActivationError(
        describeAppError(e).title || t("login.resetActivation.errorFallback"),
      );
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
      await reconcileProductLicense?.();
      navigate("/", { replace: true });
    } catch {
      // Error captured in session.error
    }
  };

  const credentialsDisabled =
    session.isLoading || activationCheckLoading || !activationReady || resettingActivation;

  // Put the cursor in the username field as soon as the form becomes usable.
  useEffect(() => {
    if (!activationCheckLoading && activationReady) {
      usernameRef.current?.focus();
    }
  }, [activationCheckLoading, activationReady]);

  const licenseSummary = [
    licenseMeta?.company_display_name ?? licenseMeta?.tenant_id ?? null,
    licenseMeta?.license_tier ?? null,
  ]
    .filter((part): part is string => Boolean(part && part.trim()))
    .join(" · ");

  const licenseDetailsToggle = (
    <button
      type="button"
      aria-expanded={showLicenseMeta}
      onClick={() => setShowLicenseMeta((v) => !v)}
      className={cn("inline-flex items-center gap-1.5 text-[13px]", mfAuth.linkPrimary)}
    >
      <Info className="h-3.5 w-3.5" aria-hidden />
      {showLicenseMeta ? t("login.licenseDetails.hide") : t("login.licenseDetails.show")}
    </button>
  );

  return (
    <AuthShell width="sm">
      <h1 className={mfAuth.title}>{t("login.welcome")}</h1>
      <p className={mfAuth.subtitle}>{t("login.subtitle")}</p>

      <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-1.5">
        {licenseSummary ? (
          <AuthStatusChip tone={activationReady ? "ok" : "neutral"}>
            {licenseSummary}
          </AuthStatusChip>
        ) : null}
        {licenseDetailsToggle}
      </div>

      {showLicenseMeta ? (
        <dl className={cn("mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5", mfAuthAlert.info)}>
          <dt className="font-medium">{t("login.licenseDetails.tier")}</dt>
          <dd>{licenseMeta?.license_tier ?? t("login.licenseDetails.na")}</dd>
          <dt className="font-medium">{t("login.licenseDetails.slotLimit")}</dt>
          <dd>{licenseMeta?.device_limit ?? t("login.licenseDetails.na")}</dd>
          <dt className="font-medium">{t("login.licenseDetails.expiry")}</dt>
          <dd>
            {licenseMeta?.expires_at
              ? formatLicenseDate(licenseMeta.expires_at, i18n.language || "en")
              : t("login.licenseDetails.na")}
          </dd>
          <dt className="font-medium">{t("login.licenseDetails.company")}</dt>
          <dd className="min-w-0 break-words">
            {licenseMeta?.company_display_name ??
              licenseMeta?.tenant_id ??
              t("login.licenseDetails.na")}
          </dd>
        </dl>
      ) : null}

      {activationCheckLoading ? (
        <div className="mt-6 flex items-center gap-2 text-sm text-text-muted" role="status">
          <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
          {t("activation.loading")}
        </div>
      ) : !activationReady ? (
        <div className="mt-6 space-y-3">
          <AuthAlert tone="warning" title={t("login.activationRequired.title")}>
            <p>{t("login.activationRequired.message")}</p>
          </AuthAlert>
          <AuthButton
            onClick={() => {
              licenseGate?.setPreferLoginView(false);
              navigate("/", { replace: true });
            }}
          >
            {t("login.activationRequired.cta")}
          </AuthButton>
          <AuthButton
            variant="ghost"
            onClick={() => setConfirmResetOpen(true)}
            disabled={resettingActivation}
          >
            {resettingActivation ? t("login.resetActivation.resetting") : t("activation.changeKey")}
          </AuthButton>
        </div>
      ) : null}

      {firstAdminPending && activationReady ? (
        <div className="mt-4 space-y-2" data-testid="login-first-admin">
          <AuthAlert tone="info" title={t("login.firstAdmin.title")}>
            <p>{t("login.firstAdmin.message")}</p>
          </AuthAlert>
          <AuthButton
            variant="secondary"
            onClick={() => navigate("/admin-setup")}
            disabled={resettingActivation}
          >
            <UserPlus className="h-4 w-4" aria-hidden />
            {t("login.firstAdmin.cta")}
          </AuthButton>
        </div>
      ) : null}

      {postActivationHint && activationReady ? (
        <AuthAlert tone="success" className="mt-4">
          {t("login.postActivationHint")}
        </AuthAlert>
      ) : null}

      <form onSubmit={handleSubmit} className="mt-6 space-y-4">
        <AuthField
          ref={usernameRef}
          id="login-username"
          label={t("login.form.username.label")}
          icon={<User />}
          type="text"
          autoComplete="username"
          required
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder={t("login.form.username.placeholder")}
          disabled={credentialsDisabled}
        />

        <AuthField
          id="login-password"
          label={t("login.form.password.label")}
          icon={<Lock />}
          type={showPassword ? "text" : "password"}
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          onKeyUp={(e) => setCapsLockOn(e.getModifierState("CapsLock"))}
          onBlur={() => setCapsLockOn(false)}
          placeholder={t("login.form.password.placeholder")}
          disabled={credentialsDisabled}
          {...(capsLockOn ? { hint: t("login.form.capsLock") } : {})}
          hintTone="warning"
          trailing={
            <button
              type="button"
              onClick={() => setShowPassword((v) => !v)}
              aria-label={
                showPassword ? t("login.form.hidePassword") : t("login.form.showPassword")
              }
              aria-pressed={showPassword}
              className="flex h-9 w-9 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-[var(--mfa-blue-tint)] hover:text-text-primary"
            >
              {showPassword ? (
                <EyeOff className="h-4 w-4" aria-hidden />
              ) : (
                <Eye className="h-4 w-4" aria-hidden />
              )}
            </button>
          }
        />

        {session.error && session.errorCode === "ACCOUNT_LOCKED" ? (
          <AuthAlert tone="warning">{session.error}</AuthAlert>
        ) : session.error && tenantScopeDenied ? (
          <AuthAlert tone="warning" title={t("login.tenantScopeDenied.title")}>
            <p>{session.error}</p>
            <p className="text-xs text-text-secondary">{t("login.tenantScopeDenied.action")}</p>
          </AuthAlert>
        ) : session.error && staleSessionClaims ? (
          <AuthAlert tone="info" title={t("login.staleSession.title")}>
            <p>{session.error}</p>
            <p className="text-xs text-text-secondary">{t("login.staleSession.action")}</p>
            <AuthButton
              variant="secondary"
              className="mt-2"
              onClick={() => setConfirmResetOpen(true)}
              disabled={resettingActivation}
            >
              {resettingActivation
                ? t("login.staleSession.resetting")
                : t("login.staleSession.resetCta")}
            </AuthButton>
          </AuthAlert>
        ) : session.error ? (
          <AuthAlert tone="danger">{session.error}</AuthAlert>
        ) : null}

        <AuthButton type="submit" loading={session.isLoading} disabled={credentialsDisabled}>
          {session.isLoading ? (
            t("login.form.submitting")
          ) : (
            <>
              {t("login.form.submit")}
              <ArrowRight className="h-4 w-4" aria-hidden />
            </>
          )}
        </AuthButton>

        <div className="text-center">
          <button
            type="button"
            onClick={() => setForgotOpen(true)}
            disabled={credentialsDisabled}
            className={mfAuth.linkPrimary}
          >
            {t("login.forgotPassword.link")}
          </button>
        </div>
      </form>

      <div className="mt-3 text-center">
        <button
          type="button"
          onClick={() => setRecoveryCodeOpen(true)}
          disabled={activationCheckLoading}
          className={cn("text-xs", mfAuth.linkPrimary)}
          data-testid="login-recovery-code-link"
        >
          {t("login.recoveryCode.link")}
        </button>
      </div>

      <ForgotPasswordDialog open={forgotOpen} onOpenChange={setForgotOpen} />
      <AdminRecoveryCodeDialog open={recoveryCodeOpen} onOpenChange={setRecoveryCodeOpen} />
      <ConfirmDialog
        open={confirmResetOpen}
        onOpenChange={setConfirmResetOpen}
        title={t("login.resetActivation.confirmTitle")}
        description={t("login.resetActivation.confirmBody")}
        confirmLabel={t("login.resetActivation.confirmAction")}
        destructive
        onConfirm={handleResetActivation}
      />

      <div className="mt-6 border-t border-surface-border pt-5 text-center">
        <p className="mb-2 text-xs text-text-muted">{t("login.resetActivation.hint")}</p>
        {resetActivationError ? (
          <AuthAlert tone="danger" className="mb-2 text-left">
            {resetActivationError}
          </AuthAlert>
        ) : null}
        <button
          type="button"
          onClick={() => setConfirmResetOpen(true)}
          disabled={resettingActivation || activationCheckLoading}
          className={mfAuth.linkPrimary}
        >
          {resettingActivation
            ? t("login.resetActivation.resetting")
            : t("login.resetActivation.cta")}
        </button>
      </div>
    </AuthShell>
  );
}
