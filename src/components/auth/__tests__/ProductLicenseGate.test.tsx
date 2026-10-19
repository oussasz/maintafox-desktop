import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ProductLicenseGate } from "@/components/auth/ProductLicenseGate";
import { getAppInfo } from "@/services/app.service";
import { forceChangePassword, getSessionInfo, login, unlockSession } from "@/services/auth-service";
import { getEntitlementSummary } from "@/services/entitlement-service";
import {
  applyProductLicenseReconciliation,
  checkProductLicenseKeyEntry,
  claimProductActivation,
  getActivationBootstrapState,
  getProductActivationToken,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
  previewProductActivation,
  refreshProductActivationPolicy,
  resetLocalTenantRuntimeData,
  submitProductLicenseKey,
} from "@/services/product-license-service";
import { verifyStepUp } from "@/services/rbac-service";
import { useSessionStore } from "@/store/session-store";
import type { SessionInfo } from "@shared/ipc-types";

// One `t` for every render, as react-i18next gives: a new function per render would re-run the
// gate's licence check effect on every render.
vi.mock("react-i18next", () => {
  const t = (key: string) => {
    const map: Record<string, string> = {
      "activation.title": "Product activation",
      "activation.subtitle": "Enter the license key",
      "activation.steps.validate": "Validate license",
      "activation.steps.activate": "Activate device",
      "activation.steps.finish": "Finish",
      "activation.validate": "Validate license",
      "activation.validating": "Validating…",
      "activation.keyLabel": "License key",
      "activation.keyPlaceholder": "••••-••••-••••",
      "activation.backToLogin": "← Back to Login",
      "activation.technicalDetails": "Technical details",
      "activation.loading": "Loading activation state…",
      "activation.denied.expiredTitle": "License expired",
      "activation.denied.expiredBody": "License validity window has ended.",
      "activation.denied.revalidate": "Revalidate now",
      "activation.denied.revalidating": "Revalidating…",
      "activation.diag.status": "Status",
      "activation.diag.online": "Online",
      "activation.diag.retryAttempt": "Retry attempt",
      "activation.diag.nextRetry": "Next retry",
      "activation.diag.lastReconciled": "Last reconciled",
      "activation.diag.lastError": "Last error",
      "activation.fields.notSpecified": "—",
      "activation.activateCta": "Activate this device",
      "stepUp.submit": "Confirm identity",
      "stepUp.passwordLabel": "Password",
    };
    return map[key] ?? key;
  };
  return { useTranslation: () => ({ t }) };
});

vi.mock("@/services/rbac-service", () => ({
  verifyStepUp: vi.fn(),
}));

vi.mock("@/services/auth-service", () => ({
  getSessionInfo: vi.fn(),
  login: vi.fn(),
  logout: vi.fn(),
  unlockSession: vi.fn(),
  unlockSessionWithPin: vi.fn(),
  forceChangePassword: vi.fn(),
}));

vi.mock("@/services/app.service", () => ({
  getAppInfo: vi.fn(),
}));

vi.mock("@/services/entitlement-service", () => ({
  getEntitlementSummary: vi.fn(),
  notifyEntitlementsUpdated: vi.fn(),
}));

vi.mock("@/services/product-license-service", async () => ({
  CLAIM_FALLBACK_TOKEN_ERRORS: new Set(["missing_token", "invalid_token", "token_superseded"]),
  // The real list: which codes are a denial is the logic under test.
  LICENCE_DENIAL_CODES: (
    await vi.importActual<{ LICENCE_DENIAL_CODES: ReadonlySet<string> }>(
      "@/services/product-license-service",
    )
  ).LICENCE_DENIAL_CODES,
  activationRequiresLocalDataReset: (bound: string | null | undefined, claim: string) =>
    !bound?.trim() || bound.trim() !== claim.trim(),
  LICENSE_RECHECK_REQUESTED_EVENT: "maintafox:license-recheck-requested",
  POST_ACTIVATION_LOGIN_HINT_KEY: "maintafox:post-activation-login-hint:v1",
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY: "maintafox:product-license:key-cache:v1",
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY: "maintafox:product-license:device-fingerprint:v1",
  applyProductLicenseReconciliation: vi.fn(),
  checkProductLicenseKeyEntry: vi.fn(),
  claimProductActivation: vi.fn(),
  previewProductActivation: vi.fn(),
  markActivationTenantInitialized: vi.fn(),
  resetLocalTenantRuntimeData: vi.fn(),
  getProductActivationToken: vi.fn(),
  refreshProductActivationPolicy: vi.fn(),
  toUserFacingActivationError: (err: unknown) => ({
    title: "Activation failed",
    message: err instanceof Error ? err.message : String(err),
    referenceCode: "test",
    technicalDetails: err instanceof Error ? err.message : String(err),
  }),
  getActivationBootstrapState: vi.fn(),
  getProductLicenseDiagnostics: vi.fn(),
  getProductLicenseOnboardingState: vi.fn(),
  submitProductLicenseKey: vi.fn(),
}));

const mockedGetAppInfo = vi.mocked(getAppInfo);
const mockedGetProductLicenseOnboardingState = vi.mocked(getProductLicenseOnboardingState);
const mockedGetProductLicenseDiagnostics = vi.mocked(getProductLicenseDiagnostics);
const mockedGetActivationBootstrapState = vi.mocked(getActivationBootstrapState);
const mockedGetEntitlementSummary = vi.mocked(getEntitlementSummary);
const mockedGetProductActivationToken = vi.mocked(getProductActivationToken);
const mockedRefreshProductActivationPolicy = vi.mocked(refreshProductActivationPolicy);
const mockedApplyProductLicenseReconciliation = vi.mocked(applyProductLicenseReconciliation);
const mockedClaimProductActivation = vi.mocked(claimProductActivation);
const mockedCheckProductLicenseKeyEntry = vi.mocked(checkProductLicenseKeyEntry);
const mockedPreviewProductActivation = vi.mocked(previewProductActivation);
const mockedResetLocalTenantRuntimeData = vi.mocked(resetLocalTenantRuntimeData);
const mockedSubmitProductLicenseKey = vi.mocked(submitProductLicenseKey);
const mockedVerifyStepUp = vi.mocked(verifyStepUp);
const mockedGetSessionInfo = vi.mocked(getSessionInfo);
const mockedLogin = vi.mocked(login);
const mockedForceChangePassword = vi.mocked(forceChangePassword);
const mockedUnlockSession = vi.mocked(unlockSession);

const signedOutSession: SessionInfo = {
  is_authenticated: false,
  is_locked: false,
  user_id: null,
  username: null,
  display_name: null,
  is_admin: null,
  force_password_change: null,
  expires_at: null,
  last_activity_at: null,
  password_expires_in_days: null,
  pin_configured: null,
};
/** An administrator signed in on this PC. */
const adminSession: SessionInfo = {
  is_authenticated: true,
  is_locked: false,
  user_id: 7,
  username: "chef",
  display_name: "Chef",
  is_admin: true,
  force_password_change: false,
  expires_at: null,
  last_activity_at: null,
  password_expires_in_days: null,
  pin_configured: false,
};
const appInfo = {
  version: "1.2.3",
  build_mode: "debug" as const,
  os: "windows",
  arch: "x64",
  app_name: "Maintafox Desktop",
  default_locale: "en-US",
};

function renderWithRouter(ui: ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

describe("ProductLicenseGate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedGetActivationBootstrapState.mockResolvedValue({
      has_tenant_admin: true,
      tenant_id: "ten_test",
      company_display_name: null,
    });
    mockedGetEntitlementSummary.mockResolvedValue({
      envelope_id: "env_test",
      state: "active",
      effective_state: "active",
      tier: "core",
      channel: "stable",
      lineage_version: 1,
      valid_until: null,
      offline_grace_until: null,
      last_verified_at: null,
      capability_map_json: "{}",
      feature_flag_map_json: "{}",
    });
    mockedGetProductActivationToken.mockResolvedValue(null);
    useSessionStore.getState().resetForTests();
    mockedGetSessionInfo.mockResolvedValue(signedOutSession);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("blocks when onboarding is incomplete (e.g. missing tenant claim)", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: false,
      status: "pending_online_validation",
      pending_online_validation: true,
      tenant_id: null,
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "pending_online_validation",
      pending_online_validation: true,
      reconciliation: { retry_attempt: 0 },
      diagnostics: [],
      has_activation_claim: false,
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("Product activation")).toBeInTheDocument();
    expect(screen.queryByText("app content")).not.toBeInTheDocument();
  });

  it("blocks first boot until activation exists", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: false,
      status: "uninitialized",
      pending_online_validation: false,
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "uninitialized",
      pending_online_validation: false,
      reconciliation: {
        retry_attempt: 0,
      },
      diagnostics: [],
      has_activation_claim: false,
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("Product activation")).toBeInTheDocument();
    expect(screen.queryByText("app content")).not.toBeInTheDocument();
  });

  it("lets activated devices continue to normal flow", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "active",
      pending_online_validation: false,
      tenant_id: "ten_test",
      last_reconciled_at: new Date().toISOString(),
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "active",
      pending_online_validation: false,
      reconciliation: {
        retry_attempt: 0,
      },
      diagnostics: [],
      has_activation_claim: true,
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("app content")).toBeInTheDocument();
    expect(screen.queryByText("Product activation")).not.toBeInTheDocument();
  });

  it("keeps degraded offline mode available for activated devices", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "degraded_api_unavailable",
      pending_online_validation: true,
      degraded_reason: "api_down",
      retry_attempt: 2,
      tenant_id: "ten_test",
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "degraded_api_unavailable",
      pending_online_validation: true,
      reconciliation: {
        retry_attempt: 2,
      },
      diagnostics: [],
      has_activation_claim: true,
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("app content")).toBeInTheDocument();
  });

  it("shows deny state instead of app when reconciliation denies activation", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "denied_expired",
      pending_online_validation: false,
      deny_message: "License window ended.",
      tenant_id: "ten_test",
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "denied_expired",
      pending_online_validation: false,
      reconciliation: {
        retry_attempt: 1,
      },
      diagnostics: [],
      has_activation_claim: true,
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("License expired")).toBeInTheDocument();
    expect(screen.getByText("License window ended.")).toBeInTheDocument();
    expect(screen.queryByText("app content")).not.toBeInTheDocument();
  });

  it("does not force-reconcile when entitlement summary fails for missing session", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "active",
      pending_online_validation: false,
      tenant_id: "ten_test",
      // No last_reconciled_at → schedule would be due if we incorrectly force reconcile.
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "active",
      pending_online_validation: false,
      reconciliation: { retry_attempt: 0 },
      diagnostics: [],
      has_activation_claim: true,
    });
    mockedGetEntitlementSummary.mockRejectedValue({
      code: "AUTH_ERROR",
      message: "Not authenticated",
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    expect(await screen.findByText("app content")).toBeInTheDocument();

    await waitFor(() => {
      expect(mockedGetEntitlementSummary).toHaveBeenCalled();
    });
    // Allow microtasks / effect settle; must stay quiet (no loop).
    await new Promise((r) => setTimeout(r, 150));

    expect(mockedGetProductActivationToken).not.toHaveBeenCalled();
    expect(mockedRefreshProductActivationPolicy).not.toHaveBeenCalled();
    expect(mockedApplyProductLicenseReconciliation).not.toHaveBeenCalled();
    expect(mockedGetEntitlementSummary.mock.calls.length).toBeLessThan(5);
  });

  /** Active device due for its scheduled re-validation (never reconciled), with a stored token. */
  function setUpDueActiveDevice() {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "active",
      pending_online_validation: false,
      tenant_id: "ten_test",
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "active",
      pending_online_validation: false,
      reconciliation: { retry_attempt: 0 },
      diagnostics: [],
      has_activation_claim: true,
    });
    mockedGetProductActivationToken.mockResolvedValue("device-token");
    localStorage.setItem("maintafox:product-license:key-cache:v1", "MFX-TEST-KEY-0001");
  }

  it("records a vendor revocation as a denial and never re-claims with the cached key", async () => {
    setUpDueActiveDevice();
    mockedRefreshProductActivationPolicy.mockRejectedValue(
      Object.assign(new Error("Device revoked"), { code: "activation_revoked", status: 403 }),
    );
    mockedApplyProductLicenseReconciliation.mockResolvedValue({
      complete: true,
      status: "denied_revoked",
      pending_online_validation: false,
      tenant_id: "ten_test",
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    await waitFor(() => {
      expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "denied", error_code: "activation_revoked" }),
      );
    });
    expect(mockedClaimProductActivation).not.toHaveBeenCalled();
  });

  it.each([
    { label: "a proxy's 429", status: 429, code: undefined, expected: "http_error" },
    {
      label: "a 400 with an unknown code",
      status: 400,
      code: "invalid_body",
      expected: "http_error",
    },
    {
      label: "a 404 licence not found",
      status: 404,
      code: "license_not_found",
      expected: "http_error",
    },
    { label: "a known denial code", status: 403, code: "license_revoked", expected: "denied" },
  ])(
    "records only the licence server's denial codes as a denial: $label → $expected",
    async ({ status, code, expected }) => {
      setUpDueActiveDevice();
      mockedRefreshProductActivationPolicy.mockRejectedValue(
        Object.assign(new Error("refused"), { status, ...(code ? { code } : {}) }),
      );
      mockedApplyProductLicenseReconciliation.mockResolvedValue({
        complete: true,
        status: expected === "denied" ? "denied_revoked" : "active",
        pending_online_validation: false,
        tenant_id: "ten_test",
      });

      renderWithRouter(
        <ProductLicenseGate>
          <div>app content</div>
        </ProductLicenseGate>,
      );

      await waitFor(() => {
        expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalled();
      });
      const kinds = mockedApplyProductLicenseReconciliation.mock.calls.map((c) => c[0].kind);
      expect(kinds).toEqual([expected]);
      expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalledWith(
        expect.objectContaining({ error_code: code ?? `http_${status}` }),
      );
    },
  );

  it("never records a licence change this PC refused as a network failure", async () => {
    setUpDueActiveDevice();
    mockedRefreshProductActivationPolicy.mockResolvedValue({
      tenant_id: "ten_other",
      license_id: "lic_other",
      machine_fingerprint: "fp",
      activation_token: "device-token",
    } as never);
    mockedApplyProductLicenseReconciliation.mockRejectedValue({
      code: "ORG_VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "LICENSE_RECONCILE_ADMIN_REQUIRED",
          message:
            "This licence change needs an administrator: sign in as an administrator, then retry.",
        },
      ],
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    await waitFor(() => {
      expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalledWith(
        expect.objectContaining({ kind: "success" }),
      );
    });
    // Only the refused success is sent (once per check): never a "network_error", "http_error" or
    // "denial" recorded because of it.
    const kinds = mockedApplyProductLicenseReconciliation.mock.calls.map((c) => c[0].kind);
    expect(kinds.every((k) => k === "success")).toBe(true);
  });

  it("re-claims with the cached key when the device token was superseded", async () => {
    setUpDueActiveDevice();
    mockedRefreshProductActivationPolicy.mockRejectedValue(
      Object.assign(new Error("Token superseded"), { code: "token_superseded", status: 401 }),
    );
    mockedClaimProductActivation.mockRejectedValue(
      Object.assign(new Error("offline"), { status: undefined }),
    );
    mockedApplyProductLicenseReconciliation.mockResolvedValue({
      complete: true,
      status: "degraded_api_unavailable",
      pending_online_validation: true,
      tenant_id: "ten_test",
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    await waitFor(() => {
      expect(mockedClaimProductActivation).toHaveBeenCalledWith(
        expect.objectContaining({ license_key: "MFX-TEST-KEY-0001" }),
      );
    });
  });

  it("stops before taking a device slot or wiping local data when this PC would refuse the licence", async () => {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: false,
      status: "pending_online_validation",
      pending_online_validation: true,
      tenant_id: "ten_test",
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "pending_online_validation",
      pending_online_validation: true,
      reconciliation: { retry_attempt: 0 },
      diagnostics: [],
      has_activation_claim: false,
    });
    mockedPreviewProductActivation.mockResolvedValue({
      tenant_id: "ten_other",
      license_id: "lic_other",
      machine_fingerprint: "fp",
    });
    mockedCheckProductLicenseKeyEntry.mockRejectedValue({
      code: "ORG_VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "LICENSE_RECONCILE_ADMIN_REQUIRED",
          message:
            "This licence change needs an administrator: sign in as an administrator, then retry.",
        },
      ],
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    fireEvent.change(await screen.findByLabelText("License key"), {
      target: { value: "MFX-TEST-KEY-0002" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Validate license" }));
    fireEvent.click(await screen.findByRole("button", { name: "Activate this device" }));

    // The refusal is shown as a warning (the coded "needs an administrator"), never as a failure.
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText("Activation failed")).not.toBeInTheDocument();
    expect(mockedCheckProductLicenseKeyEntry).toHaveBeenCalledWith(
      expect.objectContaining({ tenant_id: "ten_other", license_id: "lic_other" }),
    );
    expect(mockedClaimProductActivation).not.toHaveBeenCalled();
    expect(mockedResetLocalTenantRuntimeData).not.toHaveBeenCalled();
    expect(mockedSubmitProductLicenseKey).not.toHaveBeenCalled();
  });
  /** A denied licence (expired), signed in, with a renewal waiting on the control plane. */
  function setUpDeniedDeviceWithRenewal() {
    mockedGetAppInfo.mockResolvedValue(appInfo);
    mockedGetProductLicenseOnboardingState.mockResolvedValue({
      complete: true,
      status: "denied_expired",
      pending_online_validation: false,
      deny_message: "License window ended.",
      tenant_id: "ten_test",
    });
    mockedGetProductLicenseDiagnostics.mockResolvedValue({
      status: "denied_expired",
      pending_online_validation: false,
      reconciliation: { retry_attempt: 1 },
      diagnostics: [],
      has_activation_claim: true,
    });
    mockedGetProductActivationToken.mockResolvedValue("device-token");
    mockedRefreshProductActivationPolicy.mockResolvedValue({
      tenant_id: "ten_test",
      license_id: "lic_test",
      machine_fingerprint: "fp",
      activation_token: "device-token",
      expires_at: "2028-06-30T00:00:00Z",
    } as never);
  }

  it("lets a signed-in administrator confirm their identity on the denied screen, then applies the renewal", async () => {
    setUpDeniedDeviceWithRenewal();
    const stepUpRequired = { code: "STEP_UP_REQUIRED", message: "Step-up required" };
    let verified = false;
    mockedVerifyStepUp.mockImplementation(async () => {
      verified = true;
      return { success: true, expires_at: new Date(Date.now() + 5 * 60_000).toISOString() };
    });
    let applied = false;
    const activeState = {
      complete: true,
      status: "active" as const,
      pending_online_validation: false,
      tenant_id: "ten_test",
      last_reconciled_at: new Date().toISOString(),
    };
    mockedApplyProductLicenseReconciliation.mockImplementation(async () => {
      if (!verified) throw stepUpRequired;
      applied = true;
      return activeState;
    });
    // What this PC reports once the renewal is stored.
    mockedGetProductLicenseOnboardingState.mockImplementation(async () =>
      applied
        ? activeState
        : {
            complete: true,
            status: "denied_expired",
            pending_online_validation: false,
            deny_message: "License window ended.",
            tenant_id: "ten_test",
          },
    );

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Revalidate now" }));
    // The refusal is shown on the denied screen, with the identity check it asks for.
    fireEvent.click(await screen.findByRole("button", { name: "Confirm identity" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "not-a-real-password" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm identity" }));

    // The dialog verifies and closes, then the gate applies the same change again.
    expect(await screen.findByText("app content")).toBeInTheDocument();
    expect(mockedVerifyStepUp).toHaveBeenCalledWith("not-a-real-password");
    // The same renewal claim is applied again after the identity check (no new server call).
    const successes = mockedApplyProductLicenseReconciliation.mock.calls.filter(
      (c) => c[0].kind === "success",
    );
    expect(successes.length).toBeGreaterThanOrEqual(2);
    expect(successes.at(-1)?.[0]).toEqual(
      expect.objectContaining({
        kind: "success",
        claim: expect.objectContaining({ expires_at: "2028-06-30T00:00:00Z" }),
      }),
    );
    expect(mockedRefreshProductActivationPolicy).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("License expired")).not.toBeInTheDocument();
  });

  it("shows the coded refusal on the denied screen without an identity check when nobody may administer", async () => {
    setUpDeniedDeviceWithRenewal();
    mockedApplyProductLicenseReconciliation.mockRejectedValue({
      code: "ORG_VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "LICENSE_EXTENSION_ADMIN_REQUIRED",
          message:
            "This licence update extends the licence terms or lifts a denial: sign in as an administrator.",
        },
      ],
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Revalidate now" }));
    await waitFor(() => {
      expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalled();
    });
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("License expired")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Confirm identity" })).not.toBeInTheDocument();
    const kinds = mockedApplyProductLicenseReconciliation.mock.calls.map((c) => c[0].kind);
    expect(kinds.every((k) => k === "success")).toBe(true);
  });

  /**
   * What this PC answers to the renewal: a session that must change its password, or that is
   * idle-locked, counts as signed out (coded refusal, no identity check); a signed-in administrator
   * must confirm their identity; then it is applied.
   */
  function renewalAnsweredAsThisPc(sessionUsable: () => boolean) {
    const activeState = {
      complete: true,
      status: "active" as const,
      pending_online_validation: false,
      tenant_id: "ten_test",
      last_reconciled_at: new Date().toISOString(),
    };
    let verified = false;
    let applied = false;
    mockedVerifyStepUp.mockImplementation(async () => {
      verified = true;
      return { success: true, expires_at: new Date(Date.now() + 5 * 60_000).toISOString() };
    });
    mockedApplyProductLicenseReconciliation.mockImplementation(async () => {
      if (!sessionUsable()) {
        throw {
          code: "ORG_VALIDATION_FAILED",
          message: "Validation failed",
          details: [
            {
              code: "LICENSE_EXTENSION_ADMIN_REQUIRED",
              message:
                "This licence update extends the licence terms: sign in as an administrator.",
            },
          ],
        };
      }
      if (!verified) throw { code: "STEP_UP_REQUIRED", message: "Step-up required" };
      applied = true;
      return activeState;
    });
    mockedGetProductLicenseOnboardingState.mockImplementation(async () =>
      applied
        ? activeState
        : {
            complete: true,
            status: "denied_expired",
            pending_online_validation: false,
            deny_message: "License window ended.",
            tenant_id: "ten_test",
          },
    );
  }

  async function confirmIdentityAndExpectTheRenewal() {
    fireEvent.click(await screen.findByRole("button", { name: "Confirm identity" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Password"), {
      target: { value: "not-a-real-password" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Confirm identity" }));
    expect(await screen.findByText("app content")).toBeInTheDocument();
    expect(mockedApplyProductLicenseReconciliation.mock.calls.at(-1)?.[0]).toEqual(
      expect.objectContaining({
        kind: "success",
        claim: expect.objectContaining({ expires_at: "2028-06-30T00:00:00Z" }),
      }),
    );
  }

  it("lets an administrator whose password must change sign in, change it, confirm identity and apply the renewal while denied", async () => {
    setUpDeniedDeviceWithRenewal();
    let passwordChanged = false;
    renewalAnsweredAsThisPc(() => passwordChanged);
    mockedLogin.mockResolvedValue({
      session_info: { ...adminSession, force_password_change: true },
    });
    mockedForceChangePassword.mockImplementation(async () => {
      passwordChanged = true;
      return adminSession;
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );
    expect(await screen.findByText("License expired")).toBeInTheDocument();

    // Sign-in (the login screen's call): the password has expired, so it must be changed first.
    await act(async () => {
      await useSessionStore.getState().login({ username: "chef", password: "not-a-real-password" });
    });
    fireEvent.change(await screen.findByLabelText("session.forcePasswordChange.newPassword"), {
      target: { value: "not-a-real-new-password" },
    });
    fireEvent.change(screen.getByLabelText("session.forcePasswordChange.confirmPassword"), {
      target: { value: "not-a-real-new-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "session.forcePasswordChange.submit" }));

    // Back on the denied screen, the licence check ran again and asks for the identity check.
    expect(await screen.findByText("License expired")).toBeInTheDocument();
    expect(mockedForceChangePassword).toHaveBeenCalledWith("not-a-real-new-password");
    await confirmIdentityAndExpectTheRenewal();
  });

  it("lets an administrator whose session was idle-locked unlock it, confirm identity and apply the renewal while denied", async () => {
    setUpDeniedDeviceWithRenewal();
    let locked = true;
    renewalAnsweredAsThisPc(() => !locked);
    // Signed in earlier; the session has since been idle-locked on this PC.
    mockedGetSessionInfo.mockResolvedValueOnce(adminSession);
    mockedGetSessionInfo.mockResolvedValue({
      ...adminSession,
      is_authenticated: false,
      is_locked: true,
    });
    mockedUnlockSession.mockImplementation(async () => {
      locked = false;
      mockedGetSessionInfo.mockResolvedValue(adminSession);
      return adminSession;
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );
    await waitFor(() => {
      expect(useSessionStore.getState().info?.is_authenticated).toBe(true);
    });

    // Refused as signed out: the gate reads the session again and offers the unlock screen.
    fireEvent.click(await screen.findByRole("button", { name: "Revalidate now" }));
    fireEvent.change(await screen.findByLabelText("session.idleLocked.unlockPrompt"), {
      target: { value: "not-a-real-password" },
    });
    fireEvent.click(screen.getByRole("button", { name: "session.idleLocked.unlockAction" }));

    expect(await screen.findByText("License expired")).toBeInTheDocument();
    expect(mockedUnlockSession).toHaveBeenCalledWith("not-a-real-password");
    await confirmIdentityAndExpectTheRenewal();
  });

  it("never records licence data this PC refused as malformed as a network failure", async () => {
    setUpDueActiveDevice();
    // Signed in, the session still usable: it is read again once after the refusal, not in a loop.
    mockedGetSessionInfo.mockResolvedValue(adminSession);
    mockedRefreshProductActivationPolicy.mockResolvedValue({
      tenant_id: "ten_test",
      license_id: "lic_test",
      machine_fingerprint: "fp",
      activation_token: " ",
    } as never);
    mockedApplyProductLicenseReconciliation.mockRejectedValue({
      code: "ORG_VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "LICENSE_CLAIM_INVALID",
          message: "The licence data received is incomplete or malformed: nothing was changed.",
        },
      ],
    });

    renderWithRouter(
      <ProductLicenseGate>
        <div>app content</div>
      </ProductLicenseGate>,
    );

    await waitFor(() => {
      expect(mockedApplyProductLicenseReconciliation).toHaveBeenCalled();
    });
    expect(await screen.findByText("app content")).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 150));
    const kinds = mockedApplyProductLicenseReconciliation.mock.calls.map((c) => c[0].kind);
    expect(kinds.every((k) => k === "success")).toBe(true);
    expect(mockedGetSessionInfo.mock.calls.length).toBeLessThan(5);
  });
});
