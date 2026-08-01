import { ArrowRight, Eye, EyeOff, Info, Lock, User } from "lucide-react";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useProductLicenseGateRefresh } from "@/contexts/product-license-gate-context";
import { mfAuth } from "@/design-system/tokens";
import { useSession } from "@/hooks/use-session";
import {
  bootstrapInitialTenantAdmin,
  clearProductLicenseBrowserState,
  getActivationLicenseMetadata,
  POST_ACTIVATION_LOGIN_HINT_KEY,
  resetProductLicenseActivation,
  type ActivationLicenseMetadata,
} from "@/services/product-license-service";
import { toErrorMessage } from "@/utils/errors";

/**
 * First-run wizard when the device is activated but no tenant-scoped Administrator exists.
 */
export function SetupInitialAdminPage() {
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
      const message =
        e instanceof Error
          ? e.message
          : typeof e === "object" && e !== null && "message" in e
            ? String((e as { message: unknown }).message)
            : "Could not reset activation. Try again.";
      setResetActivationError(message);
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
      setError(toErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className={mfAuth.shell}>
      <div className={mfAuth.cardCompact}>
        <div className={mfAuth.cardBrandSeparator}>
          <MaintafoxWordmark size="md" align="center" />
        </div>
        <h1 className="text-lg font-semibold text-text-primary">Create initial administrator</h1>
        <p className="mt-2 text-sm text-text-secondary">
          No tenant administrator was found for this activation. Create the first admin account for
          your organization.
        </p>
        <button
          type="button"
          onClick={() => setShowLicenseMeta((v) => !v)}
          className="mt-3 inline-flex items-center gap-2 text-sm font-medium text-primary"
        >
          <Info className="h-4 w-4" />
          {showLicenseMeta ? "Hide license details" : "View license details"}
        </button>
        {showLicenseMeta ? (
          <div className="mt-3 rounded border border-surface-border bg-surface-0 p-3 text-sm text-text-secondary">
            <p>
              <span className="font-medium text-text-primary">License tier:</span>{" "}
              {licenseMeta?.license_tier ?? "n/a"}
            </p>
            <p>
              <span className="font-medium text-text-primary">Slot limit:</span>{" "}
              {licenseMeta?.device_limit ?? "n/a"}
            </p>
            <p>
              <span className="font-medium text-text-primary">Expiry date:</span>{" "}
              {licenseMeta?.expires_at ?? "n/a"}
            </p>
            <p>
              <span className="font-medium text-text-primary">Company:</span>{" "}
              {licenseMeta?.company_display_name ?? licenseMeta?.tenant_id ?? "n/a"}
            </p>
          </div>
        ) : null}
        <form className="mt-6 space-y-4" onSubmit={(e) => void handleSubmit(e)}>
          <div className="space-y-2">
            <Label htmlFor="bootstrap-username">Username</Label>
            <p className="text-xs text-text-secondary">
              Do not use <span className="font-mono">admin</span> — that username is reserved for
              the system bootstrap account created on first run.
            </p>
            <div className="relative">
              <User className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
              <Input
                id="bootstrap-username"
                autoComplete="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                className="pl-10"
                required
                minLength={2}
              />
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="bootstrap-display">Display name (optional)</Label>
            <Input
              id="bootstrap-display"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="Administrator"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="bootstrap-email">Email</Label>
            <Input
              id="bootstrap-email"
              type="email"
              autoComplete="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="admin@company.com"
              required
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="bootstrap-password">Password</Label>
            <div className="relative">
              <Lock className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-secondary" />
              <Input
                id="bootstrap-password"
                type={showPassword ? "text" : "password"}
                autoComplete="new-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="pl-10 pr-10"
                required
                minLength={8}
              />
              <button
                type="button"
                onClick={() => setShowPassword((v) => !v)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-text-secondary"
                aria-label={showPassword ? "Hide password" : "Show password"}
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            </div>
            <p className="text-xs text-text-secondary">
              Use at least 8 characters with uppercase, lowercase, and a digit.
            </p>
          </div>
          {error ? <p className="text-sm text-text-danger">{error}</p> : null}
          <Button type="submit" className="w-full" disabled={submitting || resettingActivation}>
            {submitting ? (
              "Creating…"
            ) : (
              <>
                Create administrator
                <ArrowRight className="ml-2 inline h-4 w-4" />
              </>
            )}
          </Button>
        </form>
        <div className="mt-5 border-t border-surface-border pt-4 text-center">
          <p className="mb-2 text-xs text-text-secondary">
            Wrong activation? Clear local license data and enter a different key.
          </p>
          {resetActivationError ? (
            <p
              role="alert"
              className="mb-2 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-xs text-red-800"
            >
              {resetActivationError}
            </p>
          ) : null}
          <button
            type="button"
            onClick={() => void handleResetActivation()}
            disabled={resettingActivation || submitting}
            className="text-sm font-semibold text-primary underline-offset-2 hover:underline disabled:opacity-50"
          >
            {resettingActivation ? "Resetting…" : "Reset activation / use another license key"}
          </button>
        </div>
      </div>
    </div>
  );
}
