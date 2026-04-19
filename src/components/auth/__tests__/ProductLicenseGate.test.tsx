import { render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ProductLicenseGate } from "@/components/auth/ProductLicenseGate";
import { getAppInfo } from "@/services/app.service";
import {
  getActivationBootstrapState,
  getProductLicenseDiagnostics,
  getProductLicenseOnboardingState,
} from "@/services/product-license-service";

vi.mock("@/services/app.service", () => ({
  getAppInfo: vi.fn(),
}));

vi.mock("@/services/product-license-service", () => ({
  POST_ACTIVATION_LOGIN_HINT_KEY: "maintafox:post-activation-login-hint:v1",
  PRODUCT_LICENSE_KEY_CACHE_STORAGE_KEY: "maintafox:product-license:key-cache:v1",
  PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY: "maintafox:product-license:device-fingerprint:v1",
  applyProductLicenseReconciliation: vi.fn(),
  claimProductActivation: vi.fn(),
  getActivationBootstrapState: vi.fn(),
  getProductLicenseDiagnostics: vi.fn(),
  getProductLicenseOnboardingState: vi.fn(),
  submitProductLicenseKey: vi.fn(),
}));

const mockedGetAppInfo = vi.mocked(getAppInfo);
const mockedGetProductLicenseOnboardingState = vi.mocked(getProductLicenseOnboardingState);
const mockedGetProductLicenseDiagnostics = vi.mocked(getProductLicenseDiagnostics);
const mockedGetActivationBootstrapState = vi.mocked(getActivationBootstrapState);
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
});
