import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BuiltinAdminWarning } from "@/components/auth/BuiltinAdminWarning";

const ipc = vi.hoisted(() => ({ answer: null as unknown, calls: [] as string[] }));

// The IPC boundary only: the component and the service's schema and visibility rule run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string) => {
    ipc.calls.push(cmd);
    return Promise.resolve(ipc.answer);
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function answer(signedIn: boolean, otherAdmin: boolean, pending: boolean) {
  ipc.answer = {
    signed_in_as_builtin_admin: signedIn,
    other_administrator_exists: otherAdmin,
    password_change_pending: pending,
  };
}

describe("BuiltinAdminWarning", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
  });

  it("warns the built-in account while it is the only administrator", async () => {
    answer(true, false, true);
    render(<BuiltinAdminWarning sessionUserId={1} />);
    expect(await screen.findByTestId("builtin-admin-warning")).toHaveTextContent(
      "session.builtinAdmin.passwordPending",
    );
    expect(ipc.calls).toEqual(["get_builtin_admin_warning"]);
  });

  it("asks to disable the account by hand once its password was changed", async () => {
    answer(true, false, false);
    render(<BuiltinAdminWarning sessionUserId={1} />);
    expect(await screen.findByTestId("builtin-admin-warning")).toHaveTextContent(
      "session.builtinAdmin.passwordChanged",
    );
  });

  it("shows nothing once another administrator exists, or for any other account", async () => {
    answer(true, true, true);
    const { rerender } = render(<BuiltinAdminWarning sessionUserId={1} />);
    await waitFor(() => expect(ipc.calls.length).toBe(1));
    expect(screen.queryByTestId("builtin-admin-warning")).not.toBeInTheDocument();

    answer(false, false, false);
    rerender(<BuiltinAdminWarning sessionUserId={2} />);
    await waitFor(() => expect(ipc.calls.length).toBe(2));
    expect(screen.queryByTestId("builtin-admin-warning")).not.toBeInTheDocument();
  });

  it("asks nothing while signed out", () => {
    answer(true, false, true);
    render(<BuiltinAdminWarning sessionUserId={null} />);
    expect(ipc.calls).toEqual([]);
    expect(screen.queryByTestId("builtin-admin-warning")).not.toBeInTheDocument();
  });

  it("refuses a malformed answer instead of guessing", async () => {
    ipc.answer = { signed_in_as_builtin_admin: "yes" };
    render(<BuiltinAdminWarning sessionUserId={1} />);
    await waitFor(() => expect(ipc.calls.length).toBe(1));
    expect(screen.queryByTestId("builtin-admin-warning")).not.toBeInTheDocument();
  });
});
