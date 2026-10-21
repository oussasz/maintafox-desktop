import { ArrowRight, AtSign, Eye, EyeOff, Info, Lock, User, UserRound } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { formatLicenseDate } from "@/components/auth/activation/OrganizationLicenseCard";
import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton } from "@/components/auth/shell/AuthPrimitives";
import { AuthShell } from "@/components/auth/shell/AuthShell";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useProductLicenseGateRefresh } from "@/contexts/product-license-gate-context";
import { mfAuth, mfAuthAlert } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { appErrorText, describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type ActivationLicenseMetadata,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  bootstrapInitialTenantAdmin,
  clearProductLicenseBrowserState,
  getActivationLicenseMetadata,
  resetProductLicenseActivation,
} from "@/services/product-license-service";

/**
 * First-run wizard when the device is activated but no tenant-scoped Administrator exists.
 *
 * Same auth shell as the login screen (FR/EN, design tokens). The calls, their payloads and the
 * routing after success are unchanged; "reset activation" now asks for confirmation first.
 */
export function SetupInitialAdminPage() {
  const { t, i18n } = useTranslation("auth");
  const navigate = useNavigate();
  const session = useSession();
  const [username, setUsername] = useState("");
  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [showLicenseMeta, setShowLicenseMeta] = useState(false);
  const [licenseMeta, setLicenseMeta] = useState<ActivationLicenseMetadata | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [resettingActivation, setResettingActivation] = useState(false);
  const [resetActivationError, setResetActivationError] = useState<string | null>(null);
  const [confirmResetOpen, setConfirmResetOpen] = useState(false);
  const mountedRef = useRef(true);
  const refreshProductLicense = useProductLicenseGateRefresh();

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
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

  useEffect(() => {
    void (async () => {
      const meta = await getActivationLicenseMetadata();
      setLicenseMeta(meta);
    })();
  }, []);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      await bootstrapInitialTenantAdmin({
        username: username.trim(),
        email: email.trim(),
        password,
        ...(displayName.trim() !== "" ? { display_name: displayName.trim() } : {}),
      });
      sessionStorage.setItem(POST_ACTIVATION_LOGIN_HINT_KEY, "1");
      navigate("/login", { replace: true });
    } catch (err) {
      setError(appErrorText(err));
    } finally {
      setSubmitting(false);
    }
  };

  const na = t("login.licenseDetails.na");

  return (
    <AuthShell width="sm">
      <h1 className={mfAuth.title}>{t("setupAdmin.title")}</h1>
      <p className={mfAuth.subtitle}>{t("setupAdmin.subtitle")}</p>

      <button
        type="button"
        aria-expanded={showLicenseMeta}
        onClick={() => setShowLicenseMeta((v) => !v)}
        className={cn("mt-3 inline-flex items-center gap-1.5 text-[13px]", mfAuth.linkPrimary)}
      >
        <Info className="h-3.5 w-3.5" aria-hidden />
        {showLicenseMeta ? t("login.licenseDetails.hide") : t("login.licenseDetails.show")}
      </button>
      {showLicenseMeta ? (
        <dl className={cn("mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5", mfAuthAlert.info)}>
          <dt className="font-medium">{t("login.licenseDetails.tier")}</dt>
          <dd>{licenseMeta?.license_tier ?? na}</dd>
          <dt className="font-medium">{t("login.licenseDetails.slotLimit")}</dt>
          <dd>{licenseMeta?.device_limit ?? na}</dd>
          <dt className="font-medium">{t("login.licenseDetails.expiry")}</dt>
          <dd>
            {licenseMeta?.expires_at
              ? formatLicenseDate(licenseMeta.expires_at, i18n.language || "en")
              : na}
          </dd>
          <dt className="font-medium">{t("login.licenseDetails.company")}</dt>
          <dd className="min-w-0 break-words">
            {licenseMeta?.company_display_name ?? licenseMeta?.tenant_id ?? na}
          </dd>
        </dl>
      ) : null}

      <form className="mt-6 space-y-4" onSubmit={(e) => void handleSubmit(e)}>
        <AuthField
          id="bootstrap-username"
          label={t("setupAdmin.username.label")}
          icon={<User />}
          autoComplete="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
          minLength={2}
          hint={t("setupAdmin.username.hint")}
        />
        <AuthField
          id="bootstrap-display"
          label={t("setupAdmin.displayName.label")}
          icon={<UserRound />}
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          placeholder={t("setupAdmin.displayName.placeholder")}
        />
        <AuthField
          id="bootstrap-email"
          label={t("setupAdmin.email.label")}
          icon={<AtSign />}
          type="email"
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          placeholder={t("setupAdmin.email.placeholder")}
          required
        />
        <AuthField
          id="bootstrap-password"
          label={t("setupAdmin.password.label")}
          icon={<Lock />}
          type={showPassword ? "text" : "password"}
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
          minLength={8}
          hint={t("setupAdmin.password.hint")}
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
        {error ? (
          <AuthAlert tone="danger" role="alert">
            {error}
          </AuthAlert>
        ) : null}
        <AuthButton type="submit" loading={submitting} disabled={submitting || resettingActivation}>
          {submitting ? (
            t("setupAdmin.submitting")
          ) : (
            <>
              {t("setupAdmin.submit")}
              <ArrowRight className="h-4 w-4" aria-hidden />
            </>
          )}
        </AuthButton>
      </form>

      <div className="mt-6 border-t border-surface-border pt-5 text-center">
        <p className="mb-2 text-xs text-text-muted">{t("setupAdmin.resetHint")}</p>
        {resetActivationError ? (
          <AuthAlert tone="danger" className="mb-2 text-left" role="alert">
            {resetActivationError}
          </AuthAlert>
        ) : null}
        <button
          type="button"
          onClick={() => setConfirmResetOpen(true)}
          disabled={resettingActivation || submitting}
          className={mfAuth.linkPrimary}
        >
          {resettingActivation
            ? t("login.resetActivation.resetting")
            : t("login.resetActivation.cta")}
        </button>
      </div>

      <ConfirmDialog
        open={confirmResetOpen}
        onOpenChange={setConfirmResetOpen}
        title={t("login.resetActivation.confirmTitle")}
        description={t("login.resetActivation.confirmBody")}
        confirmLabel={t("login.resetActivation.confirmAction")}
        destructive
        onConfirm={handleResetActivation}
      />
    </AuthShell>
  );
}
