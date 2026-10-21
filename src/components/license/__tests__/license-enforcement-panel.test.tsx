import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  LicenseEnforcementPanel,
  licenseAdviceKey,
  licenseStateTone,
} from "@/components/license/LicenseEnforcementPanel";
import type { LicenseStatusView } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr", exists: () => true },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const svc = vi.hoisted(() => ({
  getLicenseEnforcementStatus: vi.fn(),
  applyAdminLicenseAction: vi.fn(),
  applyLicensingCompromiseResponse: vi.fn(),
}));
vi.mock("@/services/license-service", () => svc);

const STATUS: LicenseStatusView = {
  entitlement_state: "active",
  activation_state: "active",
  machine_activation_state: "active",
  trust_state: "trusted",
  license_edition: "standard",
  entitlement_tier: null,
  envelope_id: "env-1",
  envelope_issuer: "issuer-a",
  envelope_key_id: "key-7",
  verification_result: "verified",
  policy_sync_pending: false,
  pending_local_writes: 0,
  last_admin_action: null,
  last_admin_action_at: null,
  actionable_message: "License is healthy.",
  recovery_paths: [],
  recent_traces: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  svc.getLicenseEnforcementStatus.mockResolvedValue(STATUS);
});

describe("LicenseEnforcementPanel", () => {
  it("never suspends in one click: a confirmation with a reason comes first", async () => {
    svc.applyAdminLicenseAction.mockResolvedValue({});
    render(<LicenseEnforcementPanel />);
    fireEvent.click(await screen.findByRole("button", { name: "license.actions.suspend.label" }));
    expect(svc.applyAdminLicenseAction).not.toHaveBeenCalled();

    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "license.actions.suspend.confirm" });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("license.reason"), {
      target: { value: "court" },
    });
    expect(confirm).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText("license.reason"), {
      target: { value: "Audit annuel du contrat" },
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(svc.applyAdminLicenseAction).toHaveBeenCalledWith({
        action: "suspend",
        reason: "Audit annuel du contrat",
        expected_entitlement_state: "active",
        expected_activation_state: "active",
      }),
    );
  });

  it("targets the issuer and key that signed the active licence", async () => {
    svc.applyLicensingCompromiseResponse.mockResolvedValue({});
    render(<LicenseEnforcementPanel />);
    fireEvent.click(
      await screen.findByRole("button", { name: "license.actions.compromise.label" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("license.reason"), {
      target: { value: "Fuite de la clé constatée" },
    });
    fireEvent.click(
      within(dialog).getByRole("button", { name: "license.actions.compromise.confirm" }),
    );
    await waitFor(() =>
      expect(svc.applyLicensingCompromiseResponse).toHaveBeenCalledWith({
        issuer: "issuer-a",
        key_id: "key-7",
        reason: "Fuite de la clé constatée",
        force_revocation: true,
      }),
    );
  });

  it("disables the compromise response with its reason when no signed licence is active", async () => {
    svc.getLicenseEnforcementStatus.mockResolvedValue({
      ...STATUS,
      envelope_issuer: null,
      envelope_key_id: null,
    });
    render(<LicenseEnforcementPanel />);
    expect(
      await screen.findByRole("button", { name: "license.actions.compromise.label" }),
    ).toBeDisabled();
    expect(screen.getByText("license.actions.compromise.noEnvelope")).toBeInTheDocument();
  });

  it("shows a failed load with a retry", async () => {
    svc.getLicenseEnforcementStatus.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "x" });
    render(<LicenseEnforcementPanel />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("license.loadError")).toBeInTheDocument();
  });
});

describe("licence state rules", () => {
  it("gives one advice in the backend's order of precedence", () => {
    expect(licenseAdviceKey(STATUS)).toBe("advice.healthy");
    expect(
      licenseAdviceKey({ ...STATUS, policy_sync_pending: true, entitlement_state: "revoked" }),
    ).toBe("advice.policySync");
    expect(licenseAdviceKey({ ...STATUS, entitlement_state: "suspended" })).toBe(
      "advice.suspended",
    );
    expect(licenseAdviceKey({ ...STATUS, activation_state: "denied_expired" })).toBe(
      "advice.activationDenied",
    );
    expect(licenseAdviceKey({ ...STATUS, trust_state: "untrusted" })).toBe("advice.untrusted");
    expect(licenseAdviceKey({ ...STATUS, activation_state: "not_activated" })).toBe(
      "advice.activationIncomplete",
    );
  });

  it("tones states without relying on colour alone", () => {
    expect(licenseStateTone("active")).toBe("success");
    expect(licenseStateTone("denied_invalid")).toBe("danger");
    expect(licenseStateTone("pending_online_validation")).toBe("warning");
    expect(licenseStateTone(null)).toBe("neutral");
  });
});
