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
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n, issueNamespacesReady } from "@/i18n";

import { LoginPage } from "../LoginPage";

const ipc = vi.hoisted(() => ({
  bootstrap: null as unknown,
  resetRefusal: null as unknown,
  calls: [] as string[],
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
      case "reset_product_license_activation":
        return ipc.resetRefusal ? Promise.reject(ipc.resetRefusal) : Promise.resolve(null);
      default:
        return Promise.resolve(null);
    }
  }),
}));

// The shared session store bootstraps through its own commands; the page only reads it here.
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({
    isLoading: false,
    error: null,
    errorCode: null,
    login: vi.fn(),
    refresh: vi.fn(),
  }),
}));

// The page's own labels by key; the coded refusals go through the real i18n instance
// (`appErrorText`), so `initReactI18next` stays the real one.
vi.mock("react-i18next", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
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

/**
 * F8: once the plant has an administrator (`has_tenant_admin`), a signed-out reset is refused
 * (`LICENSE_RESET_REQUIRES_ADMIN_SIGNIN`). The page does not offer it: "Changer de clé de licence…"
 * explains the two routes that work (an administrator signed in with a fresh identity check, or a
 * vendor recovery code). A refusal that still reaches the page shows its translated reason, not
 * "Erreur d'authentification".
 */
describe("LoginPage: licence key change", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
    await issueNamespacesReady();
  });

  beforeEach(() => {
    ipc.calls.length = 0;
    ipc.bootstrap = null;
    ipc.resetRefusal = null;
  });

  it("names the key change and shows the two routes once the plant has an administrator", async () => {
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: true, plant_join: "sign_in" };
    renderLogin();
    await formReady();
    const link = await screen.findByTestId("login-change-key-link");
    expect(link).toHaveTextContent("login.changeKey.link");
    expect(i18n.t("auth:login.changeKey.link")).toBe("Changer de clé de licence…");
    expect(screen.queryByText("login.resetActivation.cta")).not.toBeInTheDocument();

    fireEvent.click(link);
    const dialog = await screen.findByTestId("login-change-key-dialog");
    expect(dialog).toHaveTextContent("login.changeKey.signInRoute");
    expect(dialog).toHaveTextContent("login.changeKey.recoveryRoute");

    // The recovery route opens the existing vendor recovery-code dialog; nothing is reset.
    fireEvent.click(screen.getByRole("button", { name: "login.changeKey.useRecoveryCode" }));
    expect(await screen.findByTestId("admin-recovery-dialog")).toBeInTheDocument();
    expect(ipc.calls).not.toContain("reset_product_license_activation");
  });

  it("keeps the reset as before while the plant has no administrator", async () => {
    ipc.bootstrap = { tenant_id: "tenant-plant", has_tenant_admin: false, plant_join: "joining" };
    renderLogin();
    await formReady();
    await waitFor(() => expect(ipc.calls).toContain("get_activation_bootstrap_state"));
    expect(screen.getByRole("button", { name: "login.resetActivation.cta" })).toBeInTheDocument();
    expect(screen.queryByTestId("login-change-key-link")).not.toBeInTheDocument();
  });

  it("shows the translated reason of a refused reset", async () => {
    // The page could not tell (no answer on the administrators): the backend refuses with its code.
    ipc.bootstrap = new Error("ipc down");
    ipc.resetRefusal = {
      code: "ORG_VALIDATION_FAILED",
      message: "Validation failed",
      details: [
        {
          code: "LICENSE_RESET_REQUIRES_ADMIN_SIGNIN",
          message: "This PC already has an administrator: sign in as an administrator.",
        },
      ],
    };
    renderLogin();
    await formReady();
    fireEvent.click(screen.getByRole("button", { name: "login.resetActivation.cta" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "login.resetActivation.confirmAction" }),
    );

    const expected = String(i18n.t("settings:errors.LICENSE_RESET_REQUIRES_ADMIN_SIGNIN"));
    expect(expected).toContain("connectez-vous en administrateur");
    expect(await screen.findByText(expected)).toBeInTheDocument();
    expect(screen.queryByText(/Erreur d'authentification/)).not.toBeInTheDocument();
  });
});
