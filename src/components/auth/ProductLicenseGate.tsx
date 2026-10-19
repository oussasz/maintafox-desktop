import { ClipboardPaste, KeyRound, Loader2, Monitor } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import { AuthLockLayer } from "@/components/auth/AuthLockLayer";
import { ActivationStatusCard } from "@/components/auth/activation/ActivationStatusCard";
import { ActivationStepper } from "@/components/auth/activation/ActivationStepper";
import { ActivationTechnicalDetails } from "@/components/auth/activation/ActivationTechnicalDetails";
import { OrganizationLicenseCard } from "@/components/auth/activation/OrganizationLicenseCard";
import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton, AuthStatusChip } from "@/components/auth/shell/AuthPrimitives";
import { AuthShell } from "@/components/auth/shell/AuthShell";
import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { ProductLicenseGateContext } from "@/contexts/product-license-gate-context";
import { mfAuth } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import { useStepUp } from "@/hooks/use-step-up";
import { appErrorText } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { ForcePasswordChangePage } from "@/pages/auth/ForcePasswordChangePage";
import { LockScreen } from "@/pages/auth/LockScreen";
import { getAppInfo } from "@/services/app.service";
import { unlockSessionWithPin } from "@/services/auth-service";
import { getEntitlementSummary, notifyEntitlementsUpdated } from "@/services/entitlement-service";
import {
  CLAIM_FALLBACK_TOKEN_ERRORS,
  LICENCE_DENIAL_CODES,
  LICENSE_RECHECK_REQUESTED_EVENT,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY,
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY,
  type ProductActivationClaim,
  type ProductActivationPreview,
  type ProductLicenseDiagnostics,
  type ProductLicenseOnboardingState,
  type UserFacingActivationError,
  activationRequiresLocalDataReset,
  applyProductLicenseReconciliation,
  assertClaimSerializableForDesktop,
  checkProductLicenseKeyEntry,
  claimProductActivation,
  getActivationBootstrapState,
  getProductActivationToken,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  markActivationTenantInitialized,
  previewProductActivation,
  refreshProductActivationPolicy,
  resetLocalTenantRuntimeData,
  submitProductLicenseKey,
  toUserFacingActivationError,
} from "@/services/product-license-service";
import { useSessionStore } from "@/store/session-store";
import {
  extractIpcValidationDetails,
  isSessionUnavailableError,
  isStepUpRequiredError,
} from "@/utils/errors";
import type { SessionInfo } from "@shared/ipc-types";

/**
 * Codes of a licence change this PC refused (key entry, its check, reconciliation): another
 * organisation or activation, or extended licence terms, without an administrator signed in; or
 * licence data from the server that is incomplete or malformed. None is a network failure.
 */
const LICENCE_CHANGE_REFUSAL_CODES = new Set([
  "LICENSE_RECONCILE_ADMIN_REQUIRED",
  "LICENSE_EXTENSION_ADMIN_REQUIRED",
  "LICENSE_CLAIM_INVALID",
]);

/**
 * This PC refused a licence change (it needs an administrator signed in, with a fresh identity
 * check, or the licence data was malformed): the translated "what to do", or null for any other
 * failure. Never a network failure.
 */
function licenceChangeRefusal(err: unknown): string | null {
  const refused =
    extractIpcValidationDetails(err).some((d) => LICENCE_CHANGE_REFUSAL_CODES.has(d.code ?? "")) ||
    isStepUpRequiredError(err);
  return refused ? appErrorText(err) : null;
}

/** The step-up dialog was closed without confirming (`useStepUp` rejects with this). */
function isStepUpCancelled(err: unknown): boolean {
  return err instanceof Error && err.message === "Step-up cancelled by user";
}

type ReconcileOrigin = "submit" | "scheduled" | "manual" | "policy" | "startup" | "login";

/**
 * Re-validate the licence with the control plane every 15 minutes, so a revocation (licence,
 * device or account) takes effect within that window. Sync refusals trigger an immediate check.
 */
const POLICY_REFRESH_INTERVAL_MS = 15 * 60 * 1000;

interface ProductLicenseGateProps {
  children: ReactNode;
}

function defaultDeviceName(): string {
  if (typeof navigator !== "undefined" && navigator.platform) {
    return `Desktop (${navigator.platform})`;
  }
  return "This computer";
}

/**
 * Blocks the authenticated app until a product (tenant) license key has been submitted once.
 * Validate (preview) → confirm org → Activate this device (claim + local persist).
 */
export function ProductLicenseGate({ children }: ProductLicenseGateProps) {
  const { t } = useTranslation("auth");
  const navigate = useNavigate();
  const location = useLocation();
  const [loading, setLoading] = useState(true);
  const [state, setState] = useState<ProductLicenseOnboardingState | null>(null);
  const [diagnostics, setDiagnostics] = useState<ProductLicenseDiagnostics | null>(null);
  const [key, setKey] = useState("");
  const [deviceName, setDeviceName] = useState(defaultDeviceName);
  const [appVersion, setAppVersion] = useState("0.1.0-dev");
  /** The version read from the running app; unset while unknown (never the placeholder above). */
  const [runningAppVersion, setRunningAppVersion] = useState<string | undefined>(undefined);
  const [error, setError] = useState<UserFacingActivationError | null>(null);
  const [showErrorDetails, setShowErrorDetails] = useState(false);
  const [warning, setWarning] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [reconciling, setReconciling] = useState(false);
  const [preferLoginView, setPreferLoginView] = useState(false);
  const [isOnline, setIsOnline] = useState(
    typeof navigator === "undefined" ? true : navigator.onLine,
  );
  /** Successful preview awaiting user activation (no slot consumed yet). */
  const [pendingPreview, setPendingPreview] = useState<ProductActivationPreview | null>(null);
  /** Claim/submit succeeded but control-plane mark-initialized still needs retry. */
  const [pendingMarkToken, setPendingMarkToken] = useState<string | null>(null);
  /**
   * A licence change this PC refused only for lack of a fresh identity check (`StepUpRequired`:
   * the signed-in user may administer). Holds the same change, to apply once the identity is
   * confirmed here: the in-app screens that ask for it cannot be reached while the licence is
   * denied, so the gate offers it itself.
   */
  const [stepUpRetry, setStepUpRetry] = useState<{ run: () => Promise<void> } | null>(null);
  const [confirmingIdentity, setConfirmingIdentity] = useState(false);
  const { withStepUp, StepUpDialogElement } = useStepUp();
  /**
   * The session as the app sees it. The licence commands count a session that must change its
   * password, or that is idle-locked, as signed out; the screens that clear those states live
   * inside this gate's children, which a denied licence replaces. So the gate shows them itself.
   */
  const session = useSession();
  /** The session state last read again after a licence refusal (see `showLicenceRefusal`). */
  const recheckedSessionRef = useRef<SessionInfo | null>(null);

  const stateRef = useRef(state);
  const reconcilingRef = useRef(false);
  const isOnlineRef = useRef(isOnline);
  stateRef.current = state;
  isOnlineRef.current = isOnline;

  const keyCacheStorage = PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY;
  const deviceFingerprint = useState(() => {
    const storageKey = PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY;
    const existing = localStorage.getItem(storageKey);
    if (existing) return existing;
    const generated =
      typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
        ? crypto.randomUUID()
        : `device-${Date.now()}`;
    localStorage.setItem(storageKey, generated);
    return generated;
  })[0];

  const refresh = useCallback(async (opts?: { includeDiagnostics?: boolean }) => {
    setLoading(true);
    setError(null);
    try {
      const [s, d] = await Promise.all([
        getProductLicenseOnboardingState(),
        opts?.includeDiagnostics === false ? Promise.resolve(null) : getProductLicenseDiagnostics(),
      ]);
      setState(s);
      if (opts?.includeDiagnostics !== false) {
        setDiagnostics(d);
      }
    } catch (e) {
      setError(toUserFacingActivationError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const info = await getAppInfo();
        setAppVersion(info.version);
        setRunningAppVersion(info.version);
      } catch {
        // Keep default fallback.
      }
      await refresh();
    })();
  }, [refresh]);

  const licenseComplete = state?.complete ?? false;
  const prevLicenseCompleteRef = useRef<boolean | null>(null);
  useEffect(() => {
    if (loading) return;
    const prev = prevLicenseCompleteRef.current;
    if (prev === null) {
      prevLicenseCompleteRef.current = licenseComplete;
      return;
    }
    prevLicenseCompleteRef.current = licenseComplete;
    if (prev === true && licenseComplete === false) {
      const path = location.pathname;
      if (path === "/login" || path === "/admin-setup") {
        navigate("/", { replace: true });
      }
    }
  }, [loading, licenseComplete, location.pathname, navigate]);

  const recordOutcome = useCallback(
    async (
      outcome:
        | { kind: "success"; claim: ProductActivationClaim }
        | { kind: "network_error"; error_code?: string; error_message: string }
        | { kind: "http_error"; error_code?: string; error_message: string }
        | { kind: "denied"; error_code?: string; error_message: string },
    ) => {
      const next = await applyProductLicenseReconciliation({
        ...outcome,
        app_version: appVersion,
      });
      setState(next);
      const latest = await getProductLicenseDiagnostics();
      setDiagnostics(latest);
      if (outcome.kind === "success") {
        notifyEntitlementsUpdated();
      }
      return next;
    },
    [appVersion],
  );

  const recordOutcomeSafely = useCallback(
    async (
      outcome:
        | { kind: "success"; claim: ProductActivationClaim }
        | { kind: "network_error"; error_code?: string; error_message: string }
        | { kind: "http_error"; error_code?: string; error_message: string }
        | { kind: "denied"; error_code?: string; error_message: string },
    ) => {
      try {
        return await recordOutcome(outcome);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (msg.includes("No local product license state found")) {
          return null;
        }
        throw e;
      }
    },
    [recordOutcome],
  );

  /**
   * Shows why this PC refused a licence change; when the refusal only asks for an identity check,
   * keeps `retry` (the same change) for the "confirm identity" action. True when it was a refusal.
   */
  const showLicenceRefusal = useCallback((err: unknown, retry: () => Promise<void>) => {
    const refusal = licenceChangeRefusal(err);
    if (!refusal) return false;
    setWarning(refusal);
    const stepUp = isStepUpRequiredError(err);
    setStepUpRetry(stepUp ? { run: retry } : null);
    // Refused as signed out while a session is held here: it may have been idle-locked or be
    // waiting for a password change since it was last read. Read it again, so the gate offers
    // the unlock or password-change screen (then the refused change is retried).
    // Once per session state: a session read again that is still usable (a signed-in user who
    // may not administer) is not read again until it changes (sign-in, unlock, password change).
    const held = useSessionStore.getState();
    if (!stepUp && held.info?.user_id != null && held.info !== recheckedSessionRef.current) {
      recheckedSessionRef.current = held.info;
      void held.refresh().then(() => {
        recheckedSessionRef.current = useSessionStore.getState().info;
      });
    }
    return true;
  }, []);

  const attemptReconciliation = useCallback(
    async (origin: ReconcileOrigin) => {
      if (!isOnlineRef.current) {
        if (origin !== "submit") return;
        setWarning(t("activation.offlineWarning"));
        return;
      }
      if (reconcilingRef.current) return;

      reconcilingRef.current = true;
      setReconciling(true);
      setWarning(null);
      setStepUpRetry(null);
      let claim: ProductActivationClaim | null = null;
      try {
        // Prefer policy-refresh (no slot consumption) when a local activation token exists.
        const existingToken = await getProductActivationToken().catch(() => null);
        if (existingToken) {
          try {
            claim = await refreshProductActivationPolicy(existingToken, runningAppVersion);
          } catch (refreshErr) {
            // Re-claim only when the token itself is unusable; a revocation or other denial must
            // surface as a denial, never be retried as a fresh claim.
            const refreshCode = (refreshErr as { code?: string }).code;
            if (!refreshCode || !CLAIM_FALLBACK_TOKEN_ERRORS.has(refreshCode)) {
              throw refreshErr;
            }
            const cachedKey = localStorage.getItem(keyCacheStorage)?.trim();
            if (!cachedKey || cachedKey.length < 8) {
              throw refreshErr;
            }
            claim = await claimProductActivation({
              license_key: cachedKey,
              machine_fingerprint: deviceFingerprint,
              machine_label: deviceName.trim() || defaultDeviceName(),
              app_version: appVersion,
            });
          }
        } else {
          const cachedKey = localStorage.getItem(keyCacheStorage)?.trim();
          if (!cachedKey || cachedKey.length < 8) {
            if (stateRef.current?.status === "pending_online_validation") {
              setWarning(t("activation.subtitle"));
            }
            return;
          }
          claim = await claimProductActivation({
            license_key: cachedKey,
            machine_fingerprint: deviceFingerprint,
            machine_label: deviceName.trim() || defaultDeviceName(),
            app_version: appVersion,
          });
        }
        const next = await recordOutcomeSafely({ kind: "success", claim });
        if (next?.status === "active") {
          setWarning(null);
        }
      } catch (err) {
        // This PC refused to apply the claim (a licence change needs an administrator signed
        // in): say what to do, record nothing (it is not a network or server failure). The same
        // claim is applied again once the administrator confirms their identity.
        const refusedClaim = claim;
        const reapply = async () => {
          if (refusedClaim) await recordOutcome({ kind: "success", claim: refusedClaim });
        };
        if (showLicenceRefusal(err, reapply)) {
          return;
        }
        const e = err as Error & { code?: string; status?: number };
        const code = e.code;
        const status = e.status;
        const message = e.message || "Activation reconciliation failed.";
        // Only the licence server's denial codes are a denial (an administrator then lifts it);
        // any other 4xx or 5xx is an HTTP error, retried, never needing an administrator.
        if (code !== undefined && LICENCE_DENIAL_CODES.has(code)) {
          await recordOutcomeSafely({
            kind: "denied",
            error_code: code,
            error_message: message,
          });
        } else if (typeof status === "number") {
          await recordOutcomeSafely({
            kind: "http_error",
            error_code: code ?? `http_${status}`,
            error_message: message,
          });
        } else {
          await recordOutcomeSafely({
            kind: "network_error",
            error_code: code ?? "network_unreachable",
            error_message: message,
          });
        }
      } finally {
        reconcilingRef.current = false;
        setReconciling(false);
      }
    },
    [
      appVersion,
      deviceFingerprint,
      deviceName,
      keyCacheStorage,
      recordOutcome,
      recordOutcomeSafely,
      runningAppVersion,
      showLicenceRefusal,
      t,
    ],
  );

  const reconcileProductLicense = useCallback(async () => {
    await attemptReconciliation("login");
  }, [attemptReconciliation]);

  const finalizeActivationAndRoute = useCallback(async () => {
    sessionStorage.setItem(POST_ACTIVATION_LOGIN_HINT_KEY, "1");
    setPreferLoginView(false);
    const bootstrap = await getActivationBootstrapState();
    if (bootstrap.has_tenant_admin) {
      navigate("/login", { replace: true });
    } else {
      navigate("/admin-setup", { replace: true });
    }
  }, [navigate]);

  const onValidateKey = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!key.trim()) return;
    setSubmitting(true);
    setError(null);
    setShowErrorDetails(false);
    setWarning(null);
    try {
      const trimmed = key.trim();
      const preview = await previewProductActivation({
        license_key: trimmed,
        machine_fingerprint: deviceFingerprint,
        machine_label: deviceName.trim() || defaultDeviceName(),
        app_version: appVersion,
      });
      if (preview.machine_label?.trim()) {
        setDeviceName(preview.machine_label.trim());
      }
      setPendingPreview(preview);
    } catch (claimErr) {
      setError(toUserFacingActivationError(claimErr));
    } finally {
      setSubmitting(false);
    }
  };

  const completeLocalActivation = useCallback(
    async (claim: ProductActivationClaim, trimmedKey: string) => {
      localStorage.setItem(keyCacheStorage, trimmedKey);
      setKey("");
      setPendingPreview(null);
      setPendingMarkToken(null);
      await refresh();
      await finalizeActivationAndRoute();
    },
    [finalizeActivationAndRoute, keyCacheStorage, refresh],
  );

  const onRetryMarkInitialized = async () => {
    if (!pendingMarkToken) return;
    setSubmitting(true);
    setError(null);
    setShowErrorDetails(false);
    try {
      await markActivationTenantInitialized(pendingMarkToken);
      setPendingMarkToken(null);
      setPendingPreview(null);
      setKey("");
      await refresh();
      await finalizeActivationAndRoute();
    } catch (err) {
      setError(toUserFacingActivationError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const onActivateDevice = async (): Promise<void> => {
    if (!pendingPreview) return;
    const trimmed = key.trim();
    if (trimmed.length < 8) {
      setError(
        toUserFacingActivationError(new Error("License key must be at least 8 characters.")),
      );
      return;
    }
    setSubmitting(true);
    setError(null);
    setShowErrorDetails(false);
    setWarning(null);
    setStepUpRetry(null);
    setPendingMarkToken(null);
    const preview = pendingPreview;
    /** The activation itself; throws a refusal so the identity check can run it again. */
    const activate = async (): Promise<void> => {
      // Ask this PC first whether it would accept this licence: a refusal must stop before the
      // claim takes a device slot on the server and before local data is wiped.
      await checkProductLicenseKeyEntry(preview);
      const label = deviceName.trim() || defaultDeviceName();
      const claim = await claimProductActivation({
        license_key: trimmed,
        machine_fingerprint: deviceFingerprint,
        machine_label: label,
        app_version: appVersion,
      });
      // Validate desktop claim shape BEFORE wiping local tenant data.
      // Previous bug: reset ran, then submit failed on alias-duplicate claim JSON.
      assertClaimSerializableForDesktop(claim);
      if (activationRequiresLocalDataReset(stateRef.current?.tenant_id, claim.tenant_id)) {
        await resetLocalTenantRuntimeData();
      }
      await submitProductLicenseKey({
        key: trimmed,
        claim,
        machine_fingerprint: deviceFingerprint,
        app_version: appVersion,
      });
      notifyEntitlementsUpdated();
      if (claim.is_initialized !== true) {
        try {
          await markActivationTenantInitialized(claim.activation_token);
        } catch (markErr) {
          setPendingMarkToken(claim.activation_token);
          localStorage.setItem(keyCacheStorage, trimmed);
          setError(toUserFacingActivationError(markErr));
          await refresh({ includeDiagnostics: false });
          return;
        }
      }
      await completeLocalActivation(claim, trimmed);
    };
    try {
      await activate();
    } catch (err) {
      // Run again as a whole once the identity is confirmed (this PC's check first, as here).
      if (showLicenceRefusal(err, activate)) return;
      setError(toUserFacingActivationError(err));
    } finally {
      setSubmitting(false);
    }
  };

  useEffect(() => {
    const onlineHandler = () => setIsOnline(true);
    const offlineHandler = () => setIsOnline(false);
    window.addEventListener("online", onlineHandler);
    window.addEventListener("offline", offlineHandler);
    return () => {
      window.removeEventListener("online", onlineHandler);
      window.removeEventListener("offline", offlineHandler);
    };
  }, []);

  useEffect(() => {
    // Wait until initial onboarding load finishes so stateRef is populated for startup.
    if (loading) return;

    const isBlockedStatus = (status: string | undefined) =>
      status === "denied_revoked" ||
      status === "denied_expired" ||
      status === "denied_slot_limit" ||
      status === "denied_force_update_required" ||
      status === "denied_invalid";

    const shouldRetryBySchedule = () => {
      const s = stateRef.current;
      if (!s?.complete || isBlockedStatus(s.status)) return false;
      if (s.pending_online_validation) {
        if (!s.next_retry_at) return true;
        const at = Date.parse(s.next_retry_at);
        return Number.isFinite(at) ? Date.now() >= at : true;
      }
      if (s.status === "denied_offline_grace_expired") return true;
      if (s.status === "active") {
        const last = s.last_reconciled_at ? Date.parse(s.last_reconciled_at) : Number.NaN;
        if (!Number.isFinite(last)) return true;
        return Date.now() - last >= POLICY_REFRESH_INTERVAL_MS;
      }
      return false;
    };

    let cancelled = false;
    const maybeReconcile = async (origin: "startup" | "scheduled" | "policy") => {
      const s = stateRef.current;
      if (!s?.complete || isBlockedStatus(s.status)) return;
      if (!isOnlineRef.current || reconcilingRef.current || cancelled) return;

      let missingEnvelope = false;
      try {
        const summary = await getEntitlementSummary();
        missingEnvelope = !summary.envelope_id;
      } catch (err) {
        if (isSessionUnavailableError(err)) {
          // Pre-login: do not treat as missing envelope — wait for login trigger.
          return;
        }
        // Non-auth read failure: skip force-reconcile; do not invent missingEnvelope.
        return;
      }
      // Missing signed envelope must bypass retry backoff — otherwise Core stays soft-open forever.
      if (shouldRetryBySchedule() || missingEnvelope) {
        await attemptReconciliation(missingEnvelope ? "policy" : origin);
      }
    };

    void maybeReconcile("startup");
    const interval = window.setInterval(() => {
      if (!isOnlineRef.current) {
        // Offline: re-read the local state so an expired offline grace window blocks the app.
        void getProductLicenseOnboardingState()
          .then((next) => {
            if (!cancelled) setState(next);
          })
          .catch(() => undefined);
        return;
      }
      void maybeReconcile("scheduled");
    }, POLICY_REFRESH_INTERVAL_MS);
    const onRecheckRequested = () => {
      const s = stateRef.current;
      if (!s?.complete || !isOnlineRef.current || reconcilingRef.current || cancelled) return;
      void attemptReconciliation("policy");
    };
    window.addEventListener(LICENSE_RECHECK_REQUESTED_EVENT, onRecheckRequested);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
      window.removeEventListener(LICENSE_RECHECK_REQUESTED_EVENT, onRecheckRequested);
    };
  }, [attemptReconciliation, isOnline, loading]);

  const denyView = useMemo(() => {
    switch (state?.status) {
      case "denied_revoked":
        return {
          title: t("activation.denied.revokedTitle"),
          body: t("activation.denied.revokedBody"),
        };
      case "denied_expired":
        return {
          title: t("activation.denied.expiredTitle"),
          body: t("activation.denied.expiredBody"),
        };
      case "denied_slot_limit":
        return {
          title: t("activation.denied.slotTitle"),
          body: t("activation.denied.slotBody"),
        };
      case "denied_force_update_required":
        return {
          title: t("activation.denied.forceUpdateTitle"),
          body: t("activation.denied.forceUpdateBody"),
        };
      case "denied_invalid":
        return {
          title: t("activation.denied.invalidTitle"),
          body: t("activation.denied.invalidBody"),
        };
      case "denied_offline_grace_expired":
        return {
          title: t("activation.denied.offlineGraceTitle"),
          body: t("activation.denied.offlineGraceBody"),
        };
      default:
        return null;
    }
  }, [state?.status, t]);

  /**
   * Confirms the administrator's identity (the app's step-up dialog and service), then applies
   * the refused change again. A further refusal is shown the same way; a cancel keeps the offer.
   */
  const onConfirmIdentity = async () => {
    const pending = stepUpRetry;
    if (!pending) return;
    setConfirmingIdentity(true);
    try {
      // The change itself records the new licence state (and its entitlements), as on any path.
      await withStepUp(pending.run);
      setStepUpRetry(null);
      setWarning(null);
    } catch (err) {
      if (!showLicenceRefusal(err, pending.run) && !isStepUpCancelled(err)) {
        setError(toUserFacingActivationError(err));
      }
    } finally {
      setConfirmingIdentity(false);
    }
  };

  /** What the session needs before an administrator can confirm a licence change, if anything. */
  const sessionInfo = session.info;
  const sessionInterrupt: "password" | "locked" | null =
    sessionInfo?.is_locked && sessionInfo.user_id !== null
      ? "locked"
      : sessionInfo?.is_authenticated && sessionInfo.force_password_change
        ? "password"
        : null;

  /** After the password change or the unlock, the licence check runs again with the session. */
  const onForcedPasswordChange = async (newPassword: string) => {
    await session.changePassword(newPassword);
    await attemptReconciliation("login");
  };
  const onUnlock = async (password: string) => {
    await session.unlock(password);
    await attemptReconciliation("login");
  };
  const onUnlockWithPin = async (pin: string) => {
    await unlockSessionWithPin({ pin });
    await session.refresh();
    await attemptReconciliation("login");
  };

  const licenceRefusalBlock = warning ? (
    <AuthAlert tone="warning">
      <p>{warning}</p>
      {stepUpRetry ? (
        <AuthButton
          className="mt-2"
          loading={confirmingIdentity}
          disabled={confirmingIdentity}
          onClick={() => void onConfirmIdentity()}
        >
          {t("stepUp.submit")}
        </AuthButton>
      ) : null}
    </AuthAlert>
  ) : null;

  const gateContextValue = useMemo(
    () => ({
      refreshProductLicense: refresh,
      reconcileProductLicense,
      preferLoginView,
      setPreferLoginView,
    }),
    [preferLoginView, reconcileProductLicense, refresh],
  );

  const complete = state?.complete ?? false;
  const denied = denyView !== null;
  const allowLoginEscape =
    preferLoginView && (location.pathname === "/login" || location.pathname === "/admin-setup");
  /** Keep gate UI when control-plane finalize (mark initialized) still needs retry. */
  const needsMarkRetry = pendingMarkToken != null;

  const stepId = pendingPreview || needsMarkRetry ? "activate" : "validate";
  const companyName =
    pendingPreview?.tenant_display_name?.trim() ||
    state?.company_display_name?.trim() ||
    t("activation.fields.notSpecified");

  const techRows = [
    { label: t("activation.diag.status"), value: state?.status ?? "uninitialized" },
    { label: t("activation.diag.online"), value: isOnline ? "yes" : "no" },
    {
      label: t("activation.diag.retryAttempt"),
      value: String(state?.retry_attempt ?? 0),
    },
    {
      label: t("activation.diag.nextRetry"),
      value: state?.next_retry_at ?? "immediate",
    },
    {
      label: t("activation.diag.lastReconciled"),
      value: state?.last_reconciled_at ?? "never",
    },
    {
      label: t("activation.diag.lastError"),
      value: `${state?.last_error_code ?? "none"} ${state?.last_error_message ?? ""}`.trim(),
    },
    ...(diagnostics?.has_activation_claim != null
      ? [
          {
            label: "has_activation_claim",
            value: String(diagnostics.has_activation_claim),
          },
        ]
      : []),
  ];

  /** Fills the key field from the clipboard. Silently does nothing when access is denied. */
  const pasteKeyFromClipboard = async () => {
    try {
      const text = (await navigator.clipboard.readText()).trim();
      if (text) {
        setKey(text);
        setPendingPreview(null);
      }
    } catch {
      // Clipboard permission denied or unavailable: the user can still type or Ctrl+V.
    }
  };

  const errorBlock =
    error != null ? (
      <AuthAlert
        tone={error.referenceCode === "network_unreachable" ? "warning" : "danger"}
        title={t(`activation.errors.${error.referenceCode}.title`, { defaultValue: error.title })}
      >
        <p>
          {t(`activation.errors.${error.referenceCode}.message`, {
            defaultValue: error.message,
          })}
        </p>
        <p className="text-xs text-text-secondary">
          {t("activation.diag.referenceCode")}:{" "}
          <span className="font-mono text-text-primary">{error.referenceCode}</span>
        </p>
        <button
          type="button"
          aria-expanded={showErrorDetails}
          className={cn("text-xs", mfAuth.linkPrimary)}
          onClick={() => setShowErrorDetails((v) => !v)}
        >
          {showErrorDetails
            ? t("activation.hideTechnicalDetails")
            : t("activation.showTechnicalDetails")}
        </button>
        {showErrorDetails ? (
          <pre className="mt-1 max-h-32 overflow-auto whitespace-pre-wrap rounded-md bg-surface-0/70 p-2 font-mono text-xs text-text-secondary">
            {error.technicalDetails}
          </pre>
        ) : null}
      </AuthAlert>
    ) : null;

  const technicalDetails = (
    <ActivationTechnicalDetails
      summaryLabel={t("activation.technicalDetails")}
      rows={techRows}
      copyLabel={t("activation.copyDetails")}
      copiedLabel={t("activation.copied")}
    />
  );

  const goToLogin = () => {
    setPreferLoginView(true);
    navigate("/login", { replace: true });
  };

  const stepNumber = stepId === "activate" ? 2 : 1;

  const activationOrOutlet = loading ? (
    <div className={mfAuth.shell}>
      <MaintafoxWordmark size="md" tone="auth" align="center" />
      <div
        role="status"
        className="mt-6 flex items-center gap-2 text-sm text-text-secondary"
        aria-live="polite"
      >
        <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
        {t("activation.loading")}
      </div>
    </div>
  ) : allowLoginEscape ? (
    children
  ) : complete && !denied && !needsMarkRetry ? (
    <>
      {children}
      {/* A licence update refused in the background while the app is open (renewal, upgrade):
          offered here so it is applied before the licence locks the plant. */}
      {stepUpRetry && warning ? (
        <div className="fixed bottom-4 right-4 z-50 w-[min(28rem,calc(100vw-2rem))]">
          {licenceRefusalBlock}
        </div>
      ) : null}
    </>
  ) : sessionInterrupt === "password" ? (
    // Same screens and actions as AuthGuard, which this blocked view replaces.
    <ForcePasswordChangePage onComplete={onForcedPasswordChange} />
  ) : sessionInterrupt === "locked" && sessionInfo ? (
    <LockScreen
      displayName={sessionInfo.display_name ?? sessionInfo.username}
      onUnlock={onUnlock}
      onUnlockWithPin={onUnlockWithPin}
      onLogout={() => void session.logout()}
      pinConfigured={sessionInfo.pin_configured ?? false}
    />
  ) : !complete || denied || needsMarkRetry ? (
    <AuthShell width="md">
      {!denied ? (
        <div className="mb-6">
          <ActivationStepper
            current={stepId}
            progressLabel={t("activation.stepOf", { current: stepNumber, total: 3 })}
            labels={{
              validate: t("activation.steps.validate"),
              activate: t("activation.steps.activate"),
              finish: t("activation.steps.finish"),
            }}
          />
        </div>
      ) : null}

      <h1 className={mfAuth.title}>{denied ? denyView?.title : t("activation.title")}</h1>
      <p className={mfAuth.subtitle}>
        {denied
          ? (state?.deny_message ?? denyView?.body)
          : pendingPreview
            ? t("activation.verifiedTitle")
            : t("activation.subtitle")}
      </p>
      <div className="mt-3">
        <AuthStatusChip tone={isOnline ? "ok" : "warn"}>
          {isOnline ? t("activation.online") : t("activation.offline")}
        </AuthStatusChip>
      </div>

      {!denied ? (
        pendingPreview || needsMarkRetry ? (
          <div className="mt-6 space-y-4">
            <ActivationStatusCard
              items={[
                { id: "verified", label: t("activation.status.licenseVerified"), done: true },
                { id: "connected", label: t("activation.status.connected"), done: true },
              ]}
              waitingLabel={t("activation.status.waiting")}
            />

            <p className="text-sm text-text-secondary">
              {t("activation.verifiedBody")}{" "}
              <span className="font-semibold text-text-primary">{companyName}</span>
            </p>

            <OrganizationLicenseCard
              fields={{
                companyName,
                tenantSlug: pendingPreview?.tenant_slug ?? null,
                licensePlan:
                  pendingPreview?.edition ??
                  pendingPreview?.license_plan ??
                  pendingPreview?.license_tier ??
                  null,
                licenseStatus: pendingPreview?.license_status ?? "active",
                channel: pendingPreview?.update_channel ?? null,
                expiresAt: pendingPreview?.expires_at ?? null,
                allowedDevices: pendingPreview?.device_limit ?? null,
                activatedDevices: pendingPreview?.activated_device_count ?? null,
                deviceName: deviceName.trim() || defaultDeviceName(),
              }}
              labels={{
                companyBelongsTo: t("activation.companyBelongsTo"),
                slug: t("activation.fields.slug"),
                plan: t("activation.fields.plan"),
                status: t("activation.fields.status"),
                channel: t("activation.fields.channel"),
                expiration: t("activation.fields.expiration"),
                allowedDevices: t("activation.fields.allowedDevices"),
                activatedDevices: t("activation.fields.activatedDevices"),
                deviceName: t("activation.fields.deviceName"),
                notSpecified: t("activation.fields.notSpecified"),
              }}
            />

            <AuthField
              id="activation-device-name"
              label={t("activation.deviceNameLabel")}
              icon={<Monitor />}
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={t("activation.deviceNamePlaceholder")}
              disabled={submitting}
              hint={t("activation.slotNote")}
            />

            {errorBlock}
            {licenceRefusalBlock}

            <div className="flex flex-col gap-2.5">
              {pendingMarkToken ? (
                <AuthButton
                  disabled={submitting}
                  loading={submitting}
                  onClick={() => void onRetryMarkInitialized()}
                >
                  {submitting ? t("activation.activating") : t("activation.retryFinalize")}
                </AuthButton>
              ) : null}
              <AuthButton
                variant={pendingMarkToken ? "secondary" : "primary"}
                disabled={submitting || pendingMarkToken != null}
                loading={submitting && !pendingMarkToken}
                onClick={() => void onActivateDevice()}
              >
                {submitting && !pendingMarkToken
                  ? t("activation.activating")
                  : t("activation.activateCta")}
              </AuthButton>
              <AuthButton
                variant="secondary"
                disabled={submitting}
                onClick={() => {
                  setPendingPreview(null);
                  setPendingMarkToken(null);
                  setError(null);
                  setShowErrorDetails(false);
                }}
              >
                {t("activation.changeKey")}
              </AuthButton>
              <AuthButton variant="ghost" disabled={submitting} onClick={goToLogin}>
                {t("activation.backToLogin")}
              </AuthButton>
            </div>

            {technicalDetails}
          </div>
        ) : (
          <form className="mt-6 space-y-4" onSubmit={onValidateKey}>
            <AuthField
              id="product-license-key"
              label={t("activation.keyLabel")}
              icon={<KeyRound />}
              autoComplete="off"
              spellCheck={false}
              value={key}
              onChange={(e) => {
                setKey(e.target.value);
                setPendingPreview(null);
              }}
              placeholder={t("activation.keyPlaceholder")}
              hint={t("activation.keyHint")}
              inputClassName="pr-[5.5rem] font-mono text-sm tracking-wide"
              trailing={
                <button
                  type="button"
                  onClick={() => void pasteKeyFromClipboard()}
                  aria-label={t("activation.pasteKeyLabel")}
                  className="inline-flex h-9 items-center gap-1.5 rounded-md px-2.5 text-[13px] font-medium text-[var(--mfa-blue)] transition-colors hover:bg-[var(--mfa-blue-tint)]"
                >
                  <ClipboardPaste className="h-4 w-4" aria-hidden />
                  {t("activation.pasteKey")}
                </button>
              }
            />
            {errorBlock}
            {licenceRefusalBlock}
            {state?.status === "degraded_api_unavailable" ? (
              <AuthAlert tone="warning">
                {state.degraded_reason ?? t("activation.status.waiting")}
              </AuthAlert>
            ) : null}
            <AuthButton
              type="submit"
              loading={submitting}
              disabled={submitting || key.trim().length < 8}
            >
              {submitting ? t("activation.validating") : t("activation.validate")}
            </AuthButton>
            <AuthButton variant="ghost" disabled={submitting} onClick={goToLogin}>
              {t("activation.backToLogin")}
            </AuthButton>
            {technicalDetails}
          </form>
        )
      ) : (
        <div className="mt-6 space-y-3">
          {errorBlock}
          {licenceRefusalBlock}
          <AuthButton
            variant="secondary"
            onClick={() => void attemptReconciliation("manual")}
            disabled={!isOnline || reconciling}
            loading={reconciling}
          >
            {reconciling ? t("activation.denied.revalidating") : t("activation.denied.revalidate")}
          </AuthButton>
          <AuthButton variant="ghost" onClick={goToLogin}>
            {t("activation.backToLogin")}
          </AuthButton>
          {technicalDetails}
        </div>
      )}
    </AuthShell>
  ) : (
    <>
      {children}
      <AuthLockLayer />
    </>
  );

  return (
    <ProductLicenseGateContext.Provider value={gateContextValue}>
      {activationOrOutlet}
      {StepUpDialogElement}
    </ProductLicenseGateContext.Provider>
  );
}
