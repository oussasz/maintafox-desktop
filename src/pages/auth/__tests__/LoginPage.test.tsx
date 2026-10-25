/**
 * Review E-1 (SF-2): once the PC is activated, the first-start "create the first administrator"
 * screen stays reachable from the login page while the backend says no administrator exists
 * (`get_activation_bootstrap_state`). This covers a PC whose administrator was never saved (app
 * closed on the first-start screen) and a PC whose only administrator is the built-in "admin"
 * account with its password never changed (the backend does not count it). The backend alone
 * decides whether the creation is allowed.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LoginPage } from "../LoginPage";

const ipc = vi.hoisted(() => ({
  bootstrap: null as unknown,
  calls: [] as string[],
}));

// The sign-in refusal the session store holds (code from the backend `AppError`).
const session = vi.hoisted(() => ({
  error: null as string | null,
  errorCode: null as string | null,
}));

// The IPC boundary: the page and the product-licence service's schemas run for real.
vi.mock("@/lib/ipc-invoke", () => ({
  invoke: vi.fn((cmd: string) => {
    ipc.calls.push(cmd);
    switch (cmd) {
      case "get_product_license_onboarding_state":
        return Promise.resolve({
          complete: true,
          status: "active",
          pending_online_validation: false,
          tenant_id: "tenant-plant",
        });
      case "get_activation_license_metadata":
        return Promise.resolve(null);
      case "get_activation_bootstrap_state":
        return ipc.bootstrap instanceof Error
          ? Promise.reject(ipc.bootstrap)
          : Promise.resolve(ipc.bootstrap);
      default:
        return Promise.resolve(null);
    }
  }),
}));

// The shared session store bootstraps through its own commands; the page only reads it here.
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({
    isLoading: false,
    error: session.error,
    errorCode: session.errorCode,
    login: vi.fn(),
    refresh: vi.fn(),
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

function renderLogin() {
  return render(
    <MemoryRouter initialEntries={["/login"]}>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/admin-setup" element={<p>first-admin-screen</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

async function formReady() {
  await waitFor(() => expect(screen.getByLabelText("login.form.username.label")).toBeEnabled());
}

describe("LoginPage: first administrator", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.bootstrap = null;
  });

  it("offers the first-start screen when the activated PC has no administrator", async () => {
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: false, plant_join: "joining" };
    renderLogin();
    const cta = await screen.findByRole("button", { name: "login.firstAdmin.cta" });
    expect(screen.getByText("login.firstAdmin.message")).toBeInTheDocument();
    expect(ipc.calls).toContain("get_activation_bootstrap_state");
    // Signing in stays available too (e.g. the built-in account, password never changed).
    await formReady();

    fireEvent.click(cta);
    expect(await screen.findByText("first-admin-screen")).toBeInTheDocument();
  });

  it("offers nothing once an administrator exists", async () => {
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: true, plant_join: "sign_in" };
    renderLogin();
    await formReady();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));
    expect(screen.queryByTestId("login-first-admin")).not.toBeInTheDocument();
  });

  it("offers nothing once accounts have arrived from the plant", async () => {
    // Joined: the plant's accounts are here, none of them an administrator on this PC yet.
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: false, plant_join: "sign_in" };
    renderLogin();
    await formReady();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));
    expect(screen.queryByTestId("login-first-admin")).not.toBeInTheDocument();
  });

  it("offers nothing without an organisation, or when the state cannot be read", async () => {
    ipc.bootstrap = { tenant_id: null, has_tenant_admin: false, plant_join: null };
    const { unmount } = renderLogin();
    await formReady();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));
    expect(screen.queryByTestId("login-first-admin")).not.toBeInTheDocument();
    unmount();

    ipc.calls.length = 0;
    ipc.bootstrap = new Error("ipc down");
    renderLogin();
    await formReady();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));
    expect(screen.queryByTestId("login-first-admin")).not.toBeInTheDocument();
  });
});

describe("LoginPage: account received without its password", () => {
  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: true, plant_join: "sign_in" };
  });

  it("says the password was not received instead of the generic failure", async () => {
    session.error = "PASSWORD_NOT_RECEIVED: le mot de passe de ce compte n'a pas été reçu";
    session.errorCode = "PASSWORD_NOT_RECEIVED";
    try {
      renderLogin();
      expect(await screen.findByText("login.passwordNotReceived.title")).toBeInTheDocument();
      expect(screen.getByText("login.passwordNotReceived.message")).toBeInTheDocument();
      expect(screen.queryByText(/PASSWORD_NOT_RECEIVED/)).not.toBeInTheDocument();
    } finally {
      session.error = null;
      session.errorCode = null;
    }
  });
});
