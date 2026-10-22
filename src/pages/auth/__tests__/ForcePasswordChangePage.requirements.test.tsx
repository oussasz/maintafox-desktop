/**
 * The forced password change screen states and applies the plant's own password rules (read from
 * the backend), not a number written into the screen.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ForcePasswordChangePage } from "../ForcePasswordChangePage";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}:${Object.values(opts).join(",")}` : key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/services/auth-service", () => ({
  getPasswordRequirements: vi.fn().mockResolvedValue({
    min_length: 12,
    require_uppercase: true,
    require_lowercase: false,
    require_digit: true,
    require_special: false,
  }),
}));

describe("ForcePasswordChangePage requirements", () => {
  it("shows the plant's minimum and rules, and refuses a shorter password before sending", async () => {
    const onComplete = vi.fn();
    render(<ForcePasswordChangePage onComplete={onComplete} />);
    expect(await screen.findByText(/session\.forcePasswordChange\.hint:12/)).toBeInTheDocument();
    expect(
      screen.getByText(
        /session\.forcePasswordChange\.rulesLine:session\.forcePasswordChange\.rule\.uppercase, session\.forcePasswordChange\.rule\.digit/,
      ),
    ).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("session.forcePasswordChange.newPassword"), {
      target: { value: "Court#2026" },
    });
    fireEvent.change(screen.getByLabelText("session.forcePasswordChange.confirmPassword"), {
      target: { value: "Court#2026" },
    });
    fireEvent.click(screen.getByRole("button", { name: "session.forcePasswordChange.submit" }));
    await waitFor(() =>
      expect(screen.getByText("session.forcePasswordChange.tooShort:12")).toBeInTheDocument(),
    );
    expect(onComplete).not.toHaveBeenCalled();
  });
});
