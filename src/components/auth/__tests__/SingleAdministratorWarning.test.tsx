import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SingleAdministratorWarning } from "@/components/auth/SingleAdministratorWarning";

const ipc = vi.hoisted(() => ({ answer: null as unknown, calls: [] as string[] }));

// The IPC boundary only: the component and the service's schema run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string) => {
    ipc.calls.push(cmd);
    return Promise.resolve(ipc.answer);
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("SingleAdministratorWarning", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
  });

  it("asks for a second administrator while the signed-in one is the only one", async () => {
    ipc.answer = { sole_active_administrator: true };
    render(<SingleAdministratorWarning sessionUserId={1} />);
    const warning = await screen.findByTestId("single-administrator-warning");
    expect(warning).toHaveTextContent("session.singleAdministrator.title");
    expect(warning).toHaveTextContent("session.singleAdministrator.message");
    expect(ipc.calls).toEqual(["get_single_administrator_warning"]);
  });

  it("hides once a second administrator exists", async () => {
    ipc.answer = { sole_active_administrator: true };
    const { rerender } = render(<SingleAdministratorWarning sessionUserId={1} />);
    expect(await screen.findByTestId("single-administrator-warning")).toBeInTheDocument();

    ipc.answer = { sole_active_administrator: false };
    rerender(<SingleAdministratorWarning sessionUserId={2} />);
    await waitFor(() =>
      expect(screen.queryByTestId("single-administrator-warning")).not.toBeInTheDocument(),
    );
    expect(ipc.calls).toHaveLength(2);
  });

  it("asks nothing while signed out", () => {
    ipc.answer = { sole_active_administrator: true };
    render(<SingleAdministratorWarning sessionUserId={null} />);
    expect(ipc.calls).toEqual([]);
    expect(screen.queryByTestId("single-administrator-warning")).not.toBeInTheDocument();
  });

  it("refuses a malformed answer instead of guessing", async () => {
    ipc.answer = { sole_active_administrator: "yes" };
    render(<SingleAdministratorWarning sessionUserId={1} />);
    await waitFor(() => expect(ipc.calls).toHaveLength(1));
    expect(screen.queryByTestId("single-administrator-warning")).not.toBeInTheDocument();
  });
});
