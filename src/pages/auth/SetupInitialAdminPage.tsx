import {
  ArrowRight,
  AtSign,
  Eye,
  EyeOff,
  Info,
  Loader2,
  Lock,
  RefreshCw,
  User,
  UserRound,
} from "lucide-react";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
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
  type PlantJoinState,
  bootstrapInitialTenantAdmin,
  clearProductLicenseBrowserState,
  getActivationBootstrapState,
  getActivationLicenseMetadata,
  requestPlantJoin,
  resetProductLicenseActivation,
} from "@/services/product-license-service";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";

/**
 * First-run wizard when the device is activated but no tenant-scoped Administrator exists.
 *
 * Same auth shell as the login screen (FR/EN, design tokens). "Reset activation" asks for
 * confirmation first.
 *
 * The PC joins its plant first (CMMS Advisor decision 2026-10-21, `sync::plant_join`): the sync
 * worker runs one pull at once ("Joining your plant… (synchronising)"). Accounts that arrive send
 * the PC to sign-in; the creation form is shown only once a complete pull has confirmed that the
 * organisation has no account on the server. Offline, or when the pull fails, the screen asks for a
 * connection, with a retry. The state comes from the backend (`get_activation_bootstrap_state`) and
 * is read again on every sync worker status (`sync-orchestrator-store`); the backend enforces the
 * same rule on the creation itself.
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
  /** `null` until the backend has answered. */
  const [join, setJoin] = useState<PlantJoinState | null>(null);
  const [retryingJoin, setRetryingJoin] = useState(false);
  const [joinError, setJoinError] = useState<string | null>(null);
  const worker = useSyncOrchestratorStore((s) => s.worker);
  const initializeSync = useSyncOrchestratorStore((s) => s.initialize);
  const shutdownSync = useSyncOrchestratorStore((s) => s.shutdown);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const readJoinState = useCallback(async () => {
    try {
      const bootstrap = await getActivationBootstrapState();
      if (!mountedRef.current) return;
      if (bootstrap.plant_join === null) {
        // Not activated (any more): the licence gate shows the activation screen.
        navigate("/", { replace: true });
        return;
      }
      setJoin(bootstrap.plant_join);
    } catch (err) {
      if (mountedRef.current) {
        setJoinError(appErrorText(err));
        setJoin("connection_needed");
      }
    }
  }, [navigate]);

  // Join the plant: follow the sync worker, and ask it for a pull now.
  useEffect(() => {
    initializeSync();
    void requestPlantJoin()
      .catch(() => {
        // Refused once an administrator exists: the state read below sends to sign-in.
      })
      .finally(() => void readJoinState());
    return () => shutdownSync();
  }, [initializeSync, shutdownSync, readJoinState]);

  // Every worker status (a pull started, finished or failed) may change the answer.
  useEffect(() => {
    if (worker) void readJoinState();
  }, [worker, readJoinState]);

  useEffect(() => {
    if (join === "sign_in") navigate("/login", { replace: true });
  }, [join, navigate]);

  const retryJoin = async () => {
    setRetryingJoin(true);
    setJoinError(null);
    try {
      await requestPlantJoin();
      // The worker's next status (its cycle starting) is read like every other one.
      if (mountedRef.current) setJoin("joining");
    } catch (err) {
      if (mountedRef.current) setJoinError(appErrorText(err));
      await readJoinState();
    } finally {
      if (mountedRef.current) setRetryingJoin(false);
    }
  };

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
      // Refused because accounts arrived or the plant was never joined: show where this PC stands.
      void readJoinState();
    } finally {
      setSubmitting(false);
    }
  };

  const na = t("login.licenseDetails.na");
  const canCreate = join === "create_first_admin";
  const connectionNeeded = join === "connection_needed";

  return (
    <AuthShell width="sm">
      {canCreate ? (
        <>
          <h1 className={mfAuth.title}>{t("setupAdmin.title")}</h1>
          <p className={mfAuth.subtitle}>{t("setupAdmin.subtitle")}</p>
        </>
      ) : connectionNeeded ? (
        <>
          <h1 className={mfAuth.title}>{t("setupAdmin.join.connectionTitle")}</h1>
          <p className={mfAuth.subtitle}>{t("setupAdmin.join.connectionBody")}</p>
        </>
      ) : (
        <>
          <h1 className={mfAuth.title}>{t("setupAdmin.join.title")}</h1>
          <p className={mfAuth.subtitle}>{t("setupAdmin.join.body")}</p>
        </>
      )}

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

      {!canCreate ? (
        <div className="mt-6 space-y-3" data-testid="plant-join">
          {connectionNeeded ? (
            <>
              {(joinError ?? error) ? (
                <AuthAlert tone="danger" role="alert">
                  {joinError ?? error}
                </AuthAlert>
              ) : null}
              <AuthButton
                onClick={() => void retryJoin()}
                loading={retryingJoin}
                disabled={retryingJoin || resettingActivation}
              >
                <RefreshCw className="h-4 w-4" aria-hidden />
                {t("setupAdmin.join.retry")}
              </AuthButton>
            </>
          ) : (
            <p role="status" className="flex items-center gap-2 text-sm text-text-secondary">
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
              {t("setupAdmin.join.progress")}
            </p>
          )}
        </div>
      ) : null}

      {canCreate ? (
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
          <AuthButton
            type="submit"
            loading={submitting}
            disabled={submitting || resettingActivation}
          >
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
      ) : null}

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
