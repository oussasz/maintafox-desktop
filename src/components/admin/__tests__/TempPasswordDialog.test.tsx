import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TempPasswordDialog } from "@/components/admin/TempPasswordDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("TempPasswordDialog", () => {
  it("renders temporary password once and clears via onClose", () => {
    const onClose = vi.fn();

    const { rerender } = render(
      <TempPasswordDialog open temporaryPassword="TempAdmin#2026!" onClose={onClose} />,
    );

    expect(screen.getByTestId("temp-password-value")).toHaveTextContent("TempAdmin#2026!");

    fireEvent.click(screen.getByRole("button", { name: "users.resetPassword.close" }));
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(<TempPasswordDialog open={false} temporaryPassword={null} onClose={onClose} />);
    expect(screen.queryByTestId("temp-password-value")).not.toBeInTheDocument();
  });
});
