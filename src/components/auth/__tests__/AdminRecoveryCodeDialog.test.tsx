import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AdminRecoveryCodeDialog } from "@/components/auth/AdminRecoveryCodeDialog";

const ipc = vi.hoisted(() => ({
  answer: null as unknown,
  calls: [] as { cmd: string; args: unknown }[],
}));

// The IPC boundary only: the dialog and the service's schema run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string, args: unknown) => {
    ipc.calls.push({ cmd, args });
    return ipc.answer instanceof Error ||
      (typeof ipc.answer === "object" && ipc.answer !== null && "code" in ipc.answer)
      ? Promise.reject(ipc.answer)
      : Promise.resolve(ipc.answer);
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key} ${JSON.stringify(params)}` : key,
  }),
}));

const CODE = "MFXREC1.eyJ2IjoxfQ.abcd";

function fill(password = "Chef-Usine-2026!Sûr", confirmation = password) {
  fireEvent.change(screen.getByLabelText("login.recoveryCode.code"), {
    target: { value: `  ${CODE}\n` },
  });
  fireEvent.change(screen.getByLabelText("login.recoveryCode.username"), {
    target: { value: " sauveur " },
  });
  fireEvent.change(screen.getByLabelText("login.recoveryCode.displayName"), {
    target: { value: "Responsable Maintenance" },
  });
  fireEvent.change(screen.getByLabelText("login.recoveryCode.password"), {
    target: { value: password },
  });
  fireEvent.change(screen.getByLabelText("login.recoveryCode.confirmation"), {
    target: { value: confirmation },
  });
}

describe("AdminRecoveryCodeDialog", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.answer = null;
  });

  it("sends the code and the new account, then says which account to sign in with", async () => {
    ipc.answer = { username: "sauveur" };
    render(<AdminRecoveryCodeDialog open onOpenChange={() => {}} />);
    fill();
    fireEvent.click(screen.getByRole("button", { name: /login.recoveryCode.submit/ }));

    const success = await screen.findByTestId("admin-recovery-success");
    expect(success).toHaveTextContent('login.recoveryCode.success {"username":"sauveur"}');
    expect(ipc.calls).toEqual([
      {
        cmd: "redeem_admin_recovery_code",
        args: {
          input: {
            code: CODE,
            username: "sauveur",
            display_name: "Responsable Maintenance",
            password: "Chef-Usine-2026!Sûr",
            password_confirmation: "Chef-Usine-2026!Sûr",
          },
        },
      },
    ]);
  });

  it("does not send mismatched passwords", async () => {
    render(<AdminRecoveryCodeDialog open onOpenChange={() => {}} />);
    fill("Chef-Usine-2026!Sûr", "autre-chose");
    fireEvent.click(screen.getByRole("button", { name: /login.recoveryCode.submit/ }));
    expect(await screen.findByTestId("admin-recovery-error")).toHaveTextContent(
      "login.recoveryCode.mismatch",
    );
    expect(ipc.calls).toEqual([]);
  });

  it("shows the backend's refusal and stays on the form", async () => {
    ipc.answer = {
      code: "VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "ADMIN_RECOVERY_CODE_EXPIRED",
          message: "Vendor recovery code refused (expired).",
          params: {},
        },
      ],
    };
    render(<AdminRecoveryCodeDialog open onOpenChange={() => {}} />);
    fill();
    fireEvent.click(screen.getByRole("button", { name: /login.recoveryCode.submit/ }));
    expect(await screen.findByTestId("admin-recovery-error")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-recovery-success")).not.toBeInTheDocument();
    expect(screen.getByLabelText("login.recoveryCode.username")).toHaveValue(" sauveur ");
  });

  it("refuses a malformed answer instead of reporting success", async () => {
    ipc.answer = { ok: true };
    render(<AdminRecoveryCodeDialog open onOpenChange={() => {}} />);
    fill();
    fireEvent.click(screen.getByRole("button", { name: /login.recoveryCode.submit/ }));
    await waitFor(() => expect(ipc.calls).toHaveLength(1));
    expect(await screen.findByTestId("admin-recovery-error")).toBeInTheDocument();
    expect(screen.queryByTestId("admin-recovery-success")).not.toBeInTheDocument();
  });
});
