import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { DeviceSecurityGate } from "../screens/DeviceSecurityScreen";
import { isScreenLockRefused } from "../services/device-security";

import { ipcAnswering, renderMobile } from "./helpers";

function security(present: boolean | null, required = true) {
  return {
    platform: "android" as const,
    screen_lock_required: required,
    screen_lock_present: present,
  };
}

describe("device security gate", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("refuses a phone without a screen lock and shows nothing of the flow", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({ get_mobile_device_security: () => security(false) }),
    );
    renderMobile(
      <DeviceSecurityGate>
        <p>FLOW</p>
      </DeviceSecurityGate>,
    );

    expect(await screen.findByRole("alert")).toHaveTextContent("verrouillage d'écran");
    expect(screen.queryByText("FLOW")).toBeNull();
    // The only action is a new check.
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Vérifier à nouveau",
      "Français",
      "العربية",
      "English",
    ]);
  });

  it("stays refused while the lock is absent, and lets the flow through once it is present", async () => {
    let present = false;
    mockInvoke.mockImplementation(
      ipcAnswering({ get_mobile_device_security: () => security(present) }),
    );
    renderMobile(
      <DeviceSecurityGate>
        <p>FLOW</p>
      </DeviceSecurityGate>,
    );
    await screen.findByRole("alert");

    fireEvent.click(screen.getByRole("button", { name: "Vérifier à nouveau" }));
    await screen.findByRole("alert");
    expect(screen.queryByText("FLOW")).toBeNull();

    present = true;
    fireEvent.click(screen.getByRole("button", { name: "Vérifier à nouveau" }));
    await waitFor(() => expect(screen.getByText("FLOW")).toBeTruthy());
  });

  it("refuses when the engine cannot tell whether a required lock exists", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({ get_mobile_device_security: () => security(null) }),
    );
    renderMobile(
      <DeviceSecurityGate>
        <p>FLOW</p>
      </DeviceSecurityGate>,
    );
    await screen.findByRole("alert");
    expect(screen.queryByText("FLOW")).toBeNull();
  });

  it("shows the engine's error and does not let the flow through when the check fails", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_mobile_device_security: () => {
          throw { code: "INTERNAL_ERROR", message: "keyguard unreachable" };
        },
      }),
    );
    renderMobile(
      <DeviceSecurityGate>
        <p>FLOW</p>
      </DeviceSecurityGate>,
    );
    // The engine's technical text stays in the log; the user reads a plain sentence.
    expect(await screen.findByRole("alert")).toHaveTextContent("Une erreur est survenue");
    expect(screen.queryByText(/keyguard unreachable/)).toBeNull();
    expect(screen.queryByText("FLOW")).toBeNull();
  });

  it("does not refuse where no lock is required (desktop)", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_mobile_device_security: () => ({
          platform: "desktop",
          screen_lock_required: false,
          screen_lock_present: null,
        }),
      }),
    );
    renderMobile(
      <DeviceSecurityGate>
        <p>FLOW</p>
      </DeviceSecurityGate>,
    );
    await waitFor(() => expect(screen.getByText("FLOW")).toBeTruthy());
  });

  it("isScreenLockRefused follows the rule", () => {
    expect(isScreenLockRefused(security(false))).toBe(true);
    expect(isScreenLockRefused(security(null))).toBe(true);
    expect(isScreenLockRefused(security(true))).toBe(false);
    expect(isScreenLockRefused(security(null, false))).toBe(false);
  });
});
