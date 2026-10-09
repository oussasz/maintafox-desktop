import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ForgotPasswordDialog } from "@/components/auth/ForgotPasswordDialog";
import { getPasswordRecoveryContacts } from "@/services/auth-service";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return {
    useTranslation: () => ({ t }),
  };
});

vi.mock("@/services/auth-service", () => ({
  getPasswordRecoveryContacts: vi.fn(),
}));

const mockedContacts = vi.mocked(getPasswordRecoveryContacts);

describe("ForgotPasswordDialog", () => {
  beforeEach(() => {
    mockedContacts.mockReset();
  });

  it("shows loading then list of contacts", async () => {
    mockedContacts.mockResolvedValue([
      {
        display_name: "Alice Admin",
        email: "alice@example.com",
        phone: null,
      },
      {
        display_name: "Bob Site",
        email: null,
        phone: null,
      },
    ]);

    render(<ForgotPasswordDialog open onOpenChange={() => {}} />);

    expect(screen.getByTestId("forgot-password-loading")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId("forgot-password-list")).toBeInTheDocument();
    });

    expect(screen.getByText("Alice Admin")).toBeInTheDocument();
    expect(screen.getByText("alice@example.com")).toBeInTheDocument();
    expect(screen.getByText("Bob Site")).toBeInTheDocument();
    expect(screen.getByText("login.forgotPassword.noDigitalContact")).toBeInTheDocument();
  });

  it("shows empty state when no admins", async () => {
    mockedContacts.mockResolvedValue([]);

    render(<ForgotPasswordDialog open onOpenChange={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("forgot-password-empty")).toBeInTheDocument();
    });
  });

  it("shows error and retries", async () => {
    let fail = { current: true };
    mockedContacts.mockImplementation(async () => {
      if (fail.current) {
        throw new Error("db down");
      }
      return [{ display_name: "Alice Admin", email: "alice@example.com", phone: null }];
    });

    render(<ForgotPasswordDialog open onOpenChange={() => {}} />);

    await waitFor(() => {
      expect(screen.getByTestId("forgot-password-error")).toBeInTheDocument();
    });

    fail.current = false;
    fireEvent.click(screen.getByRole("button", { name: "login.forgotPassword.retry" }));

    await waitFor(() => {
      expect(screen.getByTestId("forgot-password-list")).toBeInTheDocument();
    });
    expect(screen.getByText("Alice Admin")).toBeInTheDocument();
  });
});
