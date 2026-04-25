import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { AuthLockLayer } from "@/components/auth/AuthLockLayer";
import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProductLicenseGateContext } from "@/contexts/product-license-gate-context";
import { mfAuth } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { getAppInfo } from "@/services/app.service";
import {
  applyProductLicenseReconciliation,
  claimProductActivation,
  getActivationTenantStatus,
  getActivationBootstrapState,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  markActivationTenantInitialized,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY,
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY,
  resetLocalTenantRuntimeData,
  type ProductActivationClaim,
  type ProductLicenseDiagnostics,
  type ProductLicenseOnboardingState,
  submitProductLicenseKey,
} from "@/services/product-license-service";

interface ProductLicenseGateProps {
  children: ReactNode;
}

/**
 * Blocks the authenticated app until a product (tenant) license key has been submitted once.
 * Requires a successful online activation claim before persisting state (strict guard).
 */
export function ProductLicenseGate({ children }: ProductLicenseGateProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const [loading, setLoading] = useState(true);
  const [state, setState] = useState<ProductLicenseOnboardingState | null>(null);
  const [diagnostics, setDiagnostics] = useState<ProductLicenseDiagnostics | null>(null);
  const [key, setKey] = useState("");
  const [appVersion, setAppVersion] = useState("0.1.0-dev");
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [reconciling, setReconciling] = useState(false);
  const [isOnline, setIsOnline] = useState(
    typeof navigator === "undefined" ? true : navigator.onLine,
  );
  /** Successful claim awaiting user confirmation before local persistence. */
  const [pendingClaim, setPendingClaim] = useState<ProductActivationClaim | null>(null);

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
      setError(e instanceof Error ? e.message : "Could not read license state.");
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

  /** After a reset (was activated → no longer), normalize URL off /login or /admin-setup. Avoids / ↔ /login loops on first boot. */
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
          // Happens when reconciliation runs before first successful local submit.
          return null;
        }
        throw e;
      }
    },
    [recordOutcome],
  );

  const attemptReconciliation = useCallback(
    async (origin: "submit" | "scheduled" | "manual") => {
      if (!isOnline) {
        if (origin !== "submit") return;
        setWarning(
          "You are offline. Local state is saved and reconciliation will resume when online.",
        );
        return;
      }

      const cachedKey = localStorage.getItem(keyCacheStorage)?.trim();
      if (!cachedKey || cachedKey.length < 8) {
        if (state?.status === "pending_online_validation") {
          setWarning(
            "Online reconciliation needs the original license key. Re-enter it to continue.",
          );
        }
        return;
      }

      setReconciling(true);
      setWarning(null);
      try {
        const claim = await claimProductActivation({
          license_key: cachedKey,
          machine_fingerprint: deviceFingerprint,
          machine_label: "desktop-client",
          app_version: appVersion,
        });
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
        setReconciling(false);
      }
    },
    [appVersion, deviceFingerprint, isOnline, keyCacheStorage, recordOutcomeSafely, state?.status],
  );

  const finalizeActivationAndRoute = useCallback(async () => {
    sessionStorage.setItem(POST_ACTIVATION_LOGIN_HINT_KEY, "1");
    const bootstrap = await getActivationBootstrapState();
    if (bootstrap.has_tenant_admin) {
      navigate("/login", { replace: true });
    } else {
      navigate("/admin-setup", { replace: true });
    }
  }, [navigate]);

  const onVerifyKey = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!key.trim()) return;
    setSubmitting(true);
    setError(null);
    setWarning(null);
    try {
      const trimmed = key.trim();
      const claim = await claimProductActivation({
        license_key: trimmed,
        machine_fingerprint: deviceFingerprint,
        machine_label: "desktop-client",
        app_version: appVersion,
      });
      let normalizedClaim = claim;
      try {
        const status = await getActivationTenantStatus(claim.activation_token);
        normalizedClaim = { ...claim, is_initialized: status.is_initialized };
      } catch {
        // Fall back to claim-provided is_initialized when tenant-status endpoint is unavailable.
      }
      setPendingClaim(normalizedClaim);
    } catch (claimErr) {
      const e = claimErr as Error & { code?: string; status?: number };
      const deniedCodes = new Set([
        "license_revoked",
        "license_expired",
        "slot_limit_reached",
        "force_update_required",
      ]);
      if (deniedCodes.has(e.code ?? "")) {
        setError(e.message || "Activation denied.");
      } else if (typeof e.status === "number" && e.status >= 400 && e.status < 500) {
        // Keep server message for 409 conflict or validation failures.
        setError(e.message || "Invalid Key");
      } else {
        setError(
          "Unable to reach the activation service. Check your network connection and try again.",
        );
      }
    } finally {
      setSubmitting(false);
    }
  };

  const onConfirmActivation = async () => {
    if (!pendingClaim) return;
    const trimmed = key.trim();
    if (trimmed.length < 8) {
      setError("License key must be at least 8 characters.");
      return;
    }
    setSubmitting(true);
    setError(null);
    setWarning(null);
    try {
      // Always wipe runtime first so submit/bootstrap seeds into a clean tenant scope.
      await resetLocalTenantRuntimeData();
      await submitProductLicenseKey({
        key: trimmed,
        claim: pendingClaim,
        machine_fingerprint: deviceFingerprint,
        app_version: appVersion,
      });
      if (pendingClaim.is_initialized !== true) {
        await markActivationTenantInitialized(pendingClaim.activation_token);
      }
      localStorage.setItem(keyCacheStorage, trimmed);
      setKey("");
      setPendingClaim(null);
      await refresh();
      await finalizeActivationAndRoute();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Submit failed.");
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
    if (!state?.complete) return;
    const blocked =
      state.status === "denied_revoked" ||
      state.status === "denied_expired" ||
      state.status === "denied_slot_limit" ||
      state.status === "denied_force_update_required" ||
      state.status === "denied_invalid";
    if (blocked) return;

    const shouldRetryNow = () => {
      if (!state.next_retry_at) return state.pending_online_validation;
      const at = Date.parse(state.next_retry_at);
      return Number.isFinite(at) ? Date.now() >= at : state.pending_online_validation;
    };

    if (isOnline && shouldRetryNow()) {
      void attemptReconciliation("scheduled");
    }
    const interval = window.setInterval(() => {
      if (!isOnline || reconciling) return;
      if (shouldRetryNow()) {
        void attemptReconciliation("scheduled");
      }
    }, 30_000);
    return () => window.clearInterval(interval);
  }, [attemptReconciliation, isOnline, reconciling, state]);

  const denyView = useMemo(() => {
    switch (state?.status) {
      case "denied_revoked":
        return {
          title: "License revoked",
          body: "This device license is revoked by operator policy. Contact support with diagnostics.",
        };
      case "denied_expired":
        return {
          title: "License expired",
          body: "License validity window has ended. Renewal is required before continuing.",
        };
      case "denied_slot_limit":
        return {
          title: "Machine slot limit reached",
          body: "No activation slot is available for this machine fingerprint.",
        };
      case "denied_force_update_required":
        return {
          title: "Update required",
          body: "Your desktop version is below the forced minimum version policy.",
        };
      case "denied_invalid":
        return {
          title: "License denied",
          body: "Control plane rejected this activation attempt.",
        };
      default:
        return null;
    }
  }, [state?.status]);

  const companyConfirmationName =
    pendingClaim?.tenant_display_name?.trim() || state?.company_display_name?.trim() || null;

  const gateContextValue = useMemo(() => ({ refreshProductLicense: refresh }), [refresh]);

  const complete = state?.complete ?? false;
  const denied = denyView !== null;

  const activationOrOutlet = loading ? (
    <div className={cn(mfAuth.shell, "flex-col gap-6")}>
      <MaintafoxWordmark size="md" align="center" />
      <div className="h-8 w-8 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
    </div>
  ) : !complete || denied ? (
    <div className={mfAuth.shell}>
      <div className={cn(mfAuth.cardCompact, "max-h-[90vh] overflow-y-auto")}>
        <div className={cn(mfAuth.cardBrandSeparator, "mb-5 pb-5")}>
          <MaintafoxWordmark size="md" align="center" />
        </div>
        <h1 className="text-lg font-semibold text-text-primary">
          {denied ? denyView?.title : "Product activation"}
        </h1>
        <p className="mt-2 text-sm text-text-secondary">
          {denied
            ? (state?.deny_message ?? denyView?.body)
            : "Enter the license key issued for this organization. The activation service must validate the key before you can continue."}
        </p>
        {!denied ? (
          pendingClaim ? (
            <div className="mt-6 space-y-4">
              <div className="rounded-md border border-surface-border bg-surface-0 p-4 text-sm text-text-primary">
                {companyConfirmationName ? (
                  <p>
                    <span className="font-medium">Connected to:</span>{" "}
                    <span className="text-text-secondary">{companyConfirmationName}</span>
                  </p>
                ) : (
                  <p className="text-text-secondary">
                    Key validated. Confirm to save activation on this device.
                  </p>
                )}
              </div>
              {error ? <p className="text-sm text-text-danger">{error}</p> : null}
              <div className="flex flex-col gap-2 sm:flex-row">
                <Button
                  type="button"
                  variant="outline"
                  className="flex-1"
                  disabled={submitting}
                  onClick={() => {
                    setPendingClaim(null);
                    setError(null);
                  }}
                >
                  Back
                </Button>
                <Button
                  type="button"
                  className="flex-1"
                  disabled={submitting}
                  onClick={() => void onConfirmActivation()}
                >
                  {submitting ? "Saving…" : "Confirm and continue"}
                </Button>
              </div>
            </div>
          ) : (
            <form className="mt-6 space-y-4" onSubmit={onVerifyKey}>
              <div className="space-y-2">
                <Label htmlFor="product-license-key">License key</Label>
                <Input
                  id="product-license-key"
                  autoComplete="off"
                  value={key}
                  onChange={(e) => {
                    setKey(e.target.value);
                    setPendingClaim(null);
                  }}
                  placeholder="••••-••••-••••"
                  className="font-mono text-sm"
                />
              </div>
              {error ? <p className="text-sm text-text-danger">{error}</p> : null}
              {warning ? <p className="text-sm text-text-warning">{warning}</p> : null}
              {state?.status === "degraded_api_unavailable" ? (
                <p className="text-xs text-text-warning">
                  Degraded mode: {state.degraded_reason ?? "waiting for API revalidation"}
                </p>
              ) : null}
              <Button
                type="submit"
                className="w-full"
                disabled={submitting || key.trim().length < 8}
              >
                {submitting ? "Checking…" : "Continue"}
              </Button>
            </form>
          )
        ) : (
          <div className="mt-6 space-y-3">
            {error ? <p className="text-sm text-text-danger">{error}</p> : null}
            <Button
              type="button"
              className="w-full"
              variant="outline"
              onClick={() => void attemptReconciliation("manual")}
              disabled={!isOnline || reconciling}
            >
              {reconciling ? "Revalidating…" : "Revalidate now"}
            </Button>
          </div>
        )}
        <div className="mt-5 rounded border border-surface-border p-3 text-xs text-text-secondary">
          <p className="font-medium text-text-primary">Activation diagnostics</p>
          <p className="mt-1">
            Status: <span className="font-mono">{state?.status ?? "uninitialized"}</span>
          </p>
          <p>
            Online: <span className="font-mono">{isOnline ? "yes" : "no"}</span> · Retry attempt:{" "}
            <span className="font-mono">{state?.retry_attempt ?? 0}</span>
          </p>
          <p>
            Next retry: <span className="font-mono">{state?.next_retry_at ?? "immediate"}</span>
          </p>
          <p>
            Last reconciled:{" "}
            <span className="font-mono">{state?.last_reconciled_at ?? "never"}</span>
          </p>
          <p>
            Last error:{" "}
            <span className="font-mono">
              {state?.last_error_code ?? "none"}{" "}
              {state?.last_error_message ? `· ${state.last_error_message}` : ""}
            </span>
          </p>
          {diagnostics?.diagnostics.length ? (
            <ul className="mt-2 max-h-24 space-y-1 overflow-auto font-mono">
              {diagnostics.diagnostics
                .slice(-4)
                .reverse()
                .map((evt) => (
                  <li key={`${evt.at}-${evt.kind}-${evt.code ?? "none"}`}>
                    {evt.at} · {evt.kind} · {evt.code ?? "ok"} · {evt.message}
                  </li>
                ))}
            </ul>
          ) : null}
        </div>
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
