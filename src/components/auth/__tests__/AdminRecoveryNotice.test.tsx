import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AdminRecoveryNotice } from "@/components/auth/AdminRecoveryNotice";

const ipc = vi.hoisted(() => ({
  notices: [] as unknown,
  ackFails: false,
  calls: [] as { cmd: string; args: unknown }[],
}));

// The IPC boundary only: the component and the service's schemas run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string, args: unknown) => {
    ipc.calls.push({ cmd, args });
    if (cmd === "get_admin_recovery_notices") return Promise.resolve(ipc.notices);
    if (cmd === "acknowledge_admin_recovery_notice") {
      return ipc.ackFails
        ? Promise.reject({ code: "VALIDATION_FAILED", message: "refused", details: [] })
        : Promise.resolve(null);
    }
    return Promise.resolve(null);
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key} ${JSON.stringify(params)}` : key,
    i18n: { language: "fr" },
  }),
}));

const NOTICE = {
  id: 7,
  created_at: "2026-10-19T09:00:00Z",
  created_username: "sauveur",
  created_display_name: "Responsable Maintenance",
  redeemed_device_code: "7KQ2",
};

describe("AdminRecoveryNotice", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.notices = [];
    ipc.ackFails = false;
  });

  it("tells the administrator who was created and when", async () => {
    ipc.notices = [NOTICE];
    render(<AdminRecoveryNotice sessionUserId={1} />);
    const notice = await screen.findByTestId("admin-recovery-notice");
    expect(notice).toHaveTextContent("session.recoveryNotice.title");
    expect(notice).toHaveTextContent("session.recoveryNotice.message");
    expect(notice).toHaveTextContent("Responsable Maintenance");
    expect(ipc.calls.map((c) => c.cmd)).toEqual(["get_admin_recovery_notices"]);
  });

  it("names the new account by its username as well as its display name, and the PC", async () => {
    // A display name chosen to look like an existing administrator: the username still shows.
    ipc.notices = [{ ...NOTICE, created_display_name: "Jean Dupont", created_username: "intrus" }];
    render(<AdminRecoveryNotice sessionUserId={1} />);
    const notice = await screen.findByTestId("admin-recovery-notice");
    expect(notice).toHaveTextContent('"name":"Jean Dupont"');
    expect(notice).toHaveTextContent('"username":"intrus"');
    expect(notice).toHaveTextContent('"device":"7KQ2"');
  });

  it("goes away once acknowledged, for this notice only", async () => {
    ipc.notices = [
      NOTICE,
      { ...NOTICE, id: 8, created_username: "autre", created_display_name: "Autre" },
    ];
    render(<AdminRecoveryNotice sessionUserId={1} />);
    expect(await screen.findAllByTestId("admin-recovery-notice")).toHaveLength(2);
    const [first] = screen.getAllByRole("button", { name: "session.recoveryNotice.acknowledge" });
    if (!first) throw new Error("acknowledge button");
    fireEvent.click(first);
    await waitFor(() => expect(screen.getAllByTestId("admin-recovery-notice")).toHaveLength(1));
    expect(ipc.calls).toContainEqual({
      cmd: "acknowledge_admin_recovery_notice",
      args: { noticeId: 7 },
    });
    expect(screen.getByTestId("admin-recovery-notice")).toHaveTextContent("Autre");
  });

  it("keeps the notice when the acknowledgement is refused", async () => {
    ipc.notices = [NOTICE];
    ipc.ackFails = true;
    render(<AdminRecoveryNotice sessionUserId={1} />);
    fireEvent.click(
      await screen.findByRole("button", { name: "session.recoveryNotice.acknowledge" }),
    );
    await waitFor(() =>
      expect(ipc.calls.map((c) => c.cmd)).toContain("acknowledge_admin_recovery_notice"),
    );
    expect(screen.getByTestId("admin-recovery-notice")).toBeInTheDocument();
  });

  it("asks nothing while signed out", () => {
    ipc.notices = [NOTICE];
    render(<AdminRecoveryNotice sessionUserId={null} />);
    expect(ipc.calls).toEqual([]);
    expect(screen.queryByTestId("admin-recovery-notices")).not.toBeInTheDocument();
  });

  it("shows nothing for a malformed answer", async () => {
    ipc.notices = [{ id: "7" }];
    render(<AdminRecoveryNotice sessionUserId={1} />);
    await waitFor(() => expect(ipc.calls).toHaveLength(1));
    expect(screen.queryByTestId("admin-recovery-notice")).not.toBeInTheDocument();
  });
});
