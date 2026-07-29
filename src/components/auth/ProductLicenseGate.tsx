import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import { AuthLockLayer } from "@/components/auth/AuthLockLayer";
import { ActivationStatusCard } from "@/components/auth/activation/ActivationStatusCard";
import { ActivationStepper } from "@/components/auth/activation/ActivationStepper";
import { ActivationTechnicalDetails } from "@/components/auth/activation/ActivationTechnicalDetails";
import { OrganizationLicenseCard } from "@/components/auth/activation/OrganizationLicenseCard";
import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProductLicenseGateContext } from "@/contexts/product-license-gate-context";
import { mfAlert, mfAuth } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { getAppInfo } from "@/services/app.service";
import { getEntitlementSummary, notifyEntitlementsUpdated } from "@/services/entitlement-service";
import {
  applyProductLicenseReconciliation,
  assertClaimSerializableForDesktop,
  claimProductActivation,
  getActivationBootstrapState,
  getProductActivationToken,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  markActivationTenantInitialized,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY,
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY,
  previewProductActivation,
  refreshProductActivationPolicy,
  resetLocalTenantRuntimeData,
  submitProductLicenseKey,
  toUserFacingActivationError,
  type ProductActivationClaim,
  type ProductActivationPreview,
  type ProductLicenseDiagnostics,
  type ProductLicenseOnboardingState,
  type UserFacingActivationError,
} from "@/services/product-license-service";
import { isSessionUnavailableError } from "@/utils/errors";

type ReconcileOrigin = "submit" | "scheduled" | "manual" | "policy" | "startup" | "login";

/** Soft-phase: refresh active editions at least every 6h so upgrades/downgrades apply without re-claim. */
const POLICY_REFRESH_INTERVAL_MS = 6 * 60 * 60 * 1000;

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
      try {
        // Prefer policy-refresh (no slot consumption) when a local activation token exists.
        const existingToken = await getProductActivationToken().catch(() => null);
        let claim: ProductActivationClaim;
        if (existingToken) {
          try {
            claim = await refreshProductActivationPolicy(existingToken);
          } catch (refreshErr) {
            // Fall back to claim for devices that predate policy-refresh or lost token validity.
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
        const e = err as Error & { code?: string; status?: number };
        const code = e.code;
        const status = e.status;
        const message = e.message || "Activation reconciliation failed.";
        const deniedCodes = new Set([
          "license_revoked",
          "license_expired",
          "slot_limit_reached",
          "force_update_required",
        ]);
        if (deniedCodes.has(code ?? "")) {
          await recordOutcomeSafely({
            kind: "denied",
            ...(code ? { error_code: code } : {}),
            error_message: message,
          });
        } else if (typeof status === "number" && status >= 400 && status < 500) {
          await recordOutcomeSafely({
            kind: "denied",
            error_code: code ?? "license_denied",
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
    [appVersion, deviceFingerprint, deviceName, keyCacheStorage, recordOutcomeSafely, t],
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

  const onActivateDevice = async () => {
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
    setPendingMarkToken(null);
    try {
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
      await resetLocalTenantRuntimeData();
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
    } catch (err) {
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
      if (s.status === "active") {
        const last = s.last_reconciled_at ? Date.parse(s.last_reconciled_at) : NaN;
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
      void maybeReconcile("scheduled");
    }, POLICY_REFRESH_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
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
      default:
        return null;
    }
  }, [state?.status, t]);

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

  const errorBlock =
    error != null ? (
      <div className={cn(mfAlert.danger, "space-y-2")} role="alert">
        <p className="font-medium">{error.title}</p>
        <p>{error.message}</p>
        <p className="text-xs opacity-90">
          {t("activation.diag.referenceCode")}:{" "}
          <span className="font-mono">{error.referenceCode}</span>
        </p>
        <Button
          type="button"
          variant="ghost"
          className="h-auto px-0 text-xs underline"
          onClick={() => setShowErrorDetails((v) => !v)}
        >
          {showErrorDetails
            ? t("activation.hideTechnicalDetails")
            : t("activation.showTechnicalDetails")}
        </Button>
        {showErrorDetails ? (
          <pre className="max-h-32 overflow-auto whitespace-pre-wrap font-mono text-xs opacity-90">
            {error.technicalDetails}
          </pre>
        ) : null}
      </div>
    ) : null;

  const activationOrOutlet = loading ? (
    <div className={cn(mfAuth.shell, "flex-col gap-6")}>
      <MaintafoxWordmark size="md" align="center" />
      <p className="text-sm text-text-secondary">{t("activation.loading")}</p>
      <div className="h-8 w-8 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
    </div>
  ) : allowLoginEscape || (complete && !denied && !needsMarkRetry) ? (
    children
  ) : !complete || denied || needsMarkRetry ? (
    <div className={mfAuth.shell}>
      <div className={cn(mfAuth.cardCompact, "max-h-[90vh] max-w-lg overflow-y-auto")}>
        <div className={cn(mfAuth.cardBrandSeparator, "mb-5 pb-5")}>
          <MaintafoxWordmark size="md" align="center" />
        </div>

        {!denied ? (
          <div className="mb-5">
            <ActivationStepper
              current={stepId}
              labels={{
                validate: t("activation.steps.validate"),
                activate: t("activation.steps.activate"),
                finish: t("activation.steps.finish"),
              }}
            />
          </div>
        ) : null}

        <h1 className="text-lg font-semibold text-text-primary">
          {denied ? denyView?.title : t("activation.title")}
        </h1>
        <p className="mt-2 text-sm text-text-secondary">
          {denied
            ? (state?.deny_message ?? denyView?.body)
            : pendingPreview
              ? t("activation.verifiedTitle")
              : t("activation.subtitle")}
        </p>

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
              <p className="text-xs text-text-muted">{t("activation.slotNote")}</p>

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

              <div className="space-y-2">
                <Label htmlFor="activation-device-name">{t("activation.deviceNameLabel")}</Label>
                <Input
                  id="activation-device-name"
                  value={deviceName}
                  onChange={(e) => setDeviceName(e.target.value)}
                  placeholder={t("activation.deviceNamePlaceholder")}
                  disabled={submitting}
                />
              </div>

              {errorBlock}

              {pendingMarkToken ? (
                <Button
                  type="button"
                  className="w-full"
                  disabled={submitting}
                  onClick={() => void onRetryMarkInitialized()}
                >
                  {submitting ? t("activation.activating") : t("activation.retryFinalize")}
                </Button>
              ) : null}

              <div className="flex flex-col gap-2">
                <Button
                  type="button"
                  className="w-full"
                  disabled={submitting || pendingMarkToken != null}
                  onClick={() => void onActivateDevice()}
                >
                  {submitting && !pendingMarkToken
                    ? t("activation.activating")
                    : t("activation.activateCta")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  className="w-full"
                  disabled={submitting}
                  onClick={() => {
                    setPendingPreview(null);
                    setPendingMarkToken(null);
                    setError(null);
                    setShowErrorDetails(false);
                  }}
                >
                  {t("activation.changeKey")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  className="w-full"
                  disabled={submitting}
                  onClick={() => {
                    setPreferLoginView(true);
                    navigate("/login", { replace: true });
                  }}
                >
                  {t("activation.backToLogin")}
                </Button>
              </div>

              <ActivationTechnicalDetails
                summaryLabel={t("activation.technicalDetails")}
                rows={techRows}
              />
            </div>
          ) : (
            <form className="mt-6 space-y-4" onSubmit={onValidateKey}>
              <div className="space-y-2">
                <Label htmlFor="product-license-key">{t("activation.keyLabel")}</Label>
                <Input
                  id="product-license-key"
                  autoComplete="off"
                  value={key}
                  onChange={(e) => {
                    setKey(e.target.value);
                    setPendingPreview(null);
                  }}
                  placeholder={t("activation.keyPlaceholder")}
                  className="font-mono text-sm"
                />
              </div>
              {errorBlock}
              {warning ? <p className={cn(mfAlert.warning)}>{warning}</p> : null}
              {state?.status === "degraded_api_unavailable" ? (
                <p className={cn(mfAlert.warning)}>
                  {state.degraded_reason ?? t("activation.status.waiting")}
                </p>
              ) : null}
              <Button
                type="submit"
                className="w-full"
                disabled={submitting || key.trim().length < 8}
              >
                {submitting ? t("activation.validating") : t("activation.validate")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                className="w-full"
                disabled={submitting}
                onClick={() => {
                  setPreferLoginView(true);
                  navigate("/login", { replace: true });
                }}
              >
                {t("activation.backToLogin")}
              </Button>
              <ActivationTechnicalDetails
                summaryLabel={t("activation.technicalDetails")}
                rows={techRows}
              />
            </form>
          )
        ) : (
          <div className="mt-6 space-y-3">
            {errorBlock}
            <Button
              type="button"
              className="w-full"
              variant="outline"
              onClick={() => void attemptReconciliation("manual")}
              disabled={!isOnline || reconciling}
            >
              {reconciling
                ? t("activation.denied.revalidating")
                : t("activation.denied.revalidate")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              className="w-full"
              onClick={() => {
                setPreferLoginView(true);
                navigate("/login", { replace: true });
              }}
            >
              {t("activation.backToLogin")}
            </Button>
            <ActivationTechnicalDetails
              summaryLabel={t("activation.technicalDetails")}
              rows={techRows}
            />
          </div>
        )}
      </div>
    </div>
  ) : (
    <>
      {children}
      <AuthLockLayer />
    </>
  );

  return (
    <ProductLicenseGateContext.Provider value={gateContextValue}>
      {activationOrOutlet}
    </ProductLicenseGateContext.Provider>
  );
}
