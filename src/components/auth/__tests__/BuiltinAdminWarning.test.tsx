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

  it("keeps warning once another administrator exists, asking to disable the account", async () => {
    answer(true, true, false);
    const { rerender } = render(<BuiltinAdminWarning sessionUserId={1} />);
    expect(await screen.findByTestId("builtin-admin-warning")).toHaveTextContent(
      "session.builtinAdmin.otherAdministratorExists",
    );

    // Also while its forced password change is pending: nothing promises an automatic disable.
    answer(true, true, true);
    rerender(<BuiltinAdminWarning sessionUserId={3} />);
    await waitFor(() => expect(ipc.calls.length).toBe(2));
    expect(screen.getByTestId("builtin-admin-warning")).toHaveTextContent(
      "session.builtinAdmin.otherAdministratorExists",
    );
  });

  it("shows nothing for any other account", async () => {
    answer(false, true, false);
    render(<BuiltinAdminWarning sessionUserId={2} />);
    await waitFor(() => expect(ipc.calls.length).toBe(1));
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
