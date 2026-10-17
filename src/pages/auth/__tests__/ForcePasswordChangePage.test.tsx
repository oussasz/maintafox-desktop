/**
 * Forced password change: field checks before the request, and a refusal shown through the
 * shared error description (translated title + server details), never a raw object.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ForcePasswordChangePage } from "../ForcePasswordChangePage";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

function fill(newPassword: string, confirm: string) {
  fireEvent.change(screen.getByLabelText("session.forcePasswordChange.newPassword"), {
    target: { value: newPassword },
  });
  fireEvent.change(screen.getByLabelText("session.forcePasswordChange.confirmPassword"), {
    target: { value: confirm },
  });
  fireEvent.click(screen.getByRole("button", { name: "session.forcePasswordChange.submit" }));
}

describe("ForcePasswordChangePage", () => {
  it("does not send a mismatched password", () => {
    const onComplete = vi.fn();
    render(<ForcePasswordChangePage onComplete={onComplete} />);
    fill("Nouveau#2026", "Autre#2026");
    expect(screen.getByText("session.forcePasswordChange.mismatch")).toBeInTheDocument();
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("sends the new password and shows a refusal as readable text", async () => {
    const onComplete = vi.fn().mockRejectedValue({
      code: "VALIDATION_FAILED",
      message: "Le mot de passe a déjà été utilisé.",
    });
    render(<ForcePasswordChangePage onComplete={onComplete} />);
    fill("Nouveau#2026", "Nouveau#2026");
    await waitFor(() => expect(onComplete).toHaveBeenCalledWith("Nouveau#2026"));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).not.toContain("[object Object]");
    expect(alert.textContent?.length ?? 0).toBeGreaterThan(0);
  });
});
