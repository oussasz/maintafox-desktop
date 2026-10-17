/**
 * An unreadable stored password policy is shown to the administrator with a fix action.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PasswordPolicyPanel } from "@/components/admin/PasswordPolicyPanel";
import type { PasswordPolicySettings } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: null,
  }),
}));

const svc = vi.hoisted(() => ({
  getPasswordPolicy: vi.fn(),
  setPasswordPolicy: vi.fn(),
}));
vi.mock("@/services/rbac-service", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getPasswordPolicy: svc.getPasswordPolicy,
  setPasswordPolicy: svc.setPasswordPolicy,
}));

const applied: PasswordPolicySettings = {
  max_age_days: 90,
  warn_days_before_expiry: 14,
  min_length: 8,
  require_uppercase: true,
  require_lowercase: true,
  require_digit: true,
  require_special: false,
  unreadable_keys: ["password_min_length"],
};

describe("PasswordPolicyPanel", () => {
  it("warns that the stored policy is unreadable and saves the applied values to fix it", async () => {
    svc.getPasswordPolicy
      .mockResolvedValueOnce(applied)
      .mockResolvedValueOnce({ ...applied, unreadable_keys: [] });
    svc.setPasswordPolicy.mockResolvedValueOnce(undefined);

    render(<PasswordPolicyPanel />);

    expect(await screen.findByRole("alert")).toHaveTextContent("passwordPolicy.unreadableTitle");
    fireEvent.click(screen.getByRole("button", { name: "passwordPolicy.unreadableFix" }));

    await waitFor(() => expect(svc.setPasswordPolicy).toHaveBeenCalledWith(applied));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("shows no warning when the stored policy is readable", async () => {
    svc.getPasswordPolicy.mockResolvedValueOnce({ ...applied, unreadable_keys: [] });
    render(<PasswordPolicyPanel />);
    expect(await screen.findByText("passwordPolicy.title")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
