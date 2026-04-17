import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { getAppInfo } from "@/services/app.service";
import {
  applyProductLicenseReconciliation,
  claimProductActivation,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  type ProductLicenseDiagnostics,
  type ProductLicenseOnboardingState,
  submitProductLicenseKey,
} from "@/services/product-license-service";

interface ProductLicenseGateProps {
  children: ReactNode;
}

/**
 * Blocks the authenticated app until a product (tenant) license key has been submitted once.
 * Key material is hashed server-side; only a fingerprint is stored in settings.
 */
export function ProductLicenseGate({ children }: ProductLicenseGateProps) {
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

  const keyCacheStorage = "maintafox:product-license:key-cache:v1";
  const deviceFingerprint = useState(() => {
    const storageKey = "maintafox:product-license:device-fingerprint:v1";
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

  const recordOutcome = useCallback(
    async (
      outcome:
        | { kind: "success"; claim: Awaited<ReturnType<typeof claimProductActivation>> }
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

  const attemptReconciliation = useCallback(
    async (origin: "submit" | "scheduled" | "manual") => {
      if (!isOnline) {
        if (origin !== "submit") return;
        setWarning("You are offline. Local state is saved and reconciliation will resume when online.");
        return;
      }

      const cachedKey = localStorage.getItem(keyCacheStorage)?.trim();
      if (!cachedKey || cachedKey.length < 8) {
        // On first boot pending state without cache, user must re-enter key.
        if (state?.status === "pending_online_validation") {
          setWarning("Online reconciliation needs the original license key. Re-enter it to continue.");
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
        const next = await recordOutcome({ kind: "success", claim });
        if (next.status === "active") {
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
          await recordOutcome({
            kind: "denied",
            ...(code ? { error_code: code } : {}),
            error_message: message,
          });
        } else if (typeof status === "number" && status >= 400 && status < 500) {
          await recordOutcome({ kind: "denied", error_code: code ?? "license_denied", error_message: message });
        } else if (typeof status === "number") {
          await recordOutcome({ kind: "http_error", error_code: code ?? `http_${status}`, error_message: message });
        } else {
          await recordOutcome({ kind: "network_error", error_code: code ?? "network_unreachable", error_message: message });
        }
      } finally {
        setReconciling(false);
      }
    },
    [appVersion, deviceFingerprint, isOnline, recordOutcome, state?.status],
  );

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!key.trim()) return;
    setSubmitting(true);
    setError(null);
    setWarning(null);
    try {
      const trimmed = key.trim();
      let claim: Awaited<ReturnType<typeof claimProductActivation>> | null = null;
      try {
        claim = await claimProductActivation({
          license_key: trimmed,
          machine_fingerprint: deviceFingerprint,
          machine_label: "desktop-client",
          app_version: appVersion,
        });
      } catch (claimErr) {
        const e = claimErr as Error & { code?: string; status?: number };
        if (e.code === "license_revoked" || e.code === "license_expired" || e.code === "slot_limit_reached") {
          await recordOutcome({
            kind: "denied",
            ...(e.code ? { error_code: e.code } : {}),
            error_message: e.message,
          });
          throw claimErr;
        }
        setWarning("Activation API is unreachable. Saved locally and scheduled for online reconciliation.");
      }
      await submitProductLicenseKey({
        key: trimmed,
        claim,
        machine_fingerprint: deviceFingerprint,
        app_version: appVersion,
      });
      localStorage.setItem(keyCacheStorage, trimmed);
      setKey("");
      await refresh();
      await attemptReconciliation("submit");
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

  if (loading) {
    return (
      <div className="flex h-screen items-center justify-center bg-surface-0">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  const complete = state?.complete ?? false;
  const denied = denyView !== null;

  if (!complete || denied) {
    return (
      <div className="flex min-h-screen flex-col items-center justify-center bg-surface-0 px-4">
        <div className="w-full max-w-md rounded-lg border border-surface-border bg-surface-1 p-6 shadow-panel">
          <h1 className="text-lg font-semibold text-text-primary">
            {denied ? denyView?.title : "Product license"}
          </h1>
          <p className="mt-2 text-sm text-text-secondary">
            {denied
              ? (state?.deny_message ?? denyView?.body)
              : "Enter the license key issued for this organization. Online reconciliation runs automatically and enforces policy deny states."}
          </p>
          {!denied ? (
            <form className="mt-6 space-y-4" onSubmit={onSubmit}>
              <div className="space-y-2">
                <Label htmlFor="product-license-key">License key</Label>
                <Input
                  id="product-license-key"
                  autoComplete="off"
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
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
              <Button type="submit" className="w-full" disabled={submitting || key.trim().length < 8}>
                {submitting ? "Saving…" : "Continue"}
              </Button>
            </form>
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
              Last reconciled: <span className="font-mono">{state?.last_reconciled_at ?? "never"}</span>
            </p>
            <p>
              Last error:{" "}
              <span className="font-mono">
                {state?.last_error_code ?? "none"} {state?.last_error_message ? `· ${state.last_error_message}` : ""}
              </span>
            </p>
            {diagnostics?.diagnostics.length ? (
              <ul className="mt-2 max-h-24 space-y-1 overflow-auto font-mono">
                {diagnostics.diagnostics.slice(-4).reverse().map((evt) => (
                  <li key={`${evt.at}-${evt.kind}-${evt.code ?? "none"}`}>
                    {evt.at} · {evt.kind} · {evt.code ?? "ok"} · {evt.message}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}
