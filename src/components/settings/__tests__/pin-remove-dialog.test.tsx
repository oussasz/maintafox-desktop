import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PinRemoveDialog } from "@/components/settings/ProfileSettingsPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

const auth = vi.hoisted(() => ({ clearPin: vi.fn(), setPin: vi.fn() }));
vi.mock("@/services/auth-service", () => auth);

beforeEach(() => {
  vi.clearAllMocks();
});

describe("PinRemoveDialog", () => {
  it("asks for the password in a masked field and refuses an empty one", async () => {
    const onRemoved = vi.fn();
    render(<PinRemoveDialog onClose={vi.fn()} onRemoved={onRemoved} />);
    const field = screen.getByLabelText("profile.currentPassword");
    expect(field).toHaveAttribute("type", "password");

    fireEvent.click(screen.getByRole("button", { name: "profile.removePinConfirm" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("profile.passwordRequired");
    expect(auth.clearPin).not.toHaveBeenCalled();
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("shows a refused password on the field and keeps the dialog open", async () => {
    auth.clearPin.mockRejectedValueOnce({ code: "AUTH_ERROR", message: "Mot de passe incorrect." });
    const onRemoved = vi.fn();
    render(<PinRemoveDialog onClose={vi.fn()} onRemoved={onRemoved} />);
    fireEvent.change(screen.getByLabelText("profile.currentPassword"), {
      target: { value: "not-it" },
    });
    fireEvent.click(screen.getByRole("button", { name: "profile.removePinConfirm" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(auth.clearPin).toHaveBeenCalledWith({ current_password: "not-it" });
    expect(onRemoved).not.toHaveBeenCalled();
  });

  it("removes the PIN with the typed password", async () => {
    auth.clearPin.mockResolvedValueOnce(undefined);
    const onRemoved = vi.fn();
    render(<PinRemoveDialog onClose={vi.fn()} onRemoved={onRemoved} />);
    fireEvent.change(screen.getByLabelText("profile.currentPassword"), {
      target: { value: "fixture-pass" },
    });
    fireEvent.click(screen.getByRole("button", { name: "profile.removePinConfirm" }));
    await waitFor(() => expect(onRemoved).toHaveBeenCalledTimes(1));
  });
});
