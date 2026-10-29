import { act, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { SLOW_HINT_MS, UI_DEADLINE_MS } from "../lib/ui-deadline";
import { ActivationGate } from "../screens/ActivationGate";

import { type IpcHandlers, ipcAnswering, renderMobile } from "./helpers";

const NOT_ACTIVATED = {
  complete: false,
  status: "uninitialized",
  pending_online_validation: false,
};

const APP_INFO = {
  version: "0.9.2",
  build_mode: "debug",
  os: "android",
  arch: "x86_64",
  app_name: "Maintafox",
  default_locale: "fr",
};

/** An engine call that never answers (a panic in the engine, a lost IPC answer). */
const never = () => new Promise<never>(() => undefined);

function renderGate(handlers: IpcHandlers) {
  mockInvoke.mockImplementation(ipcAnswering(handlers));
  return renderMobile(
    <ActivationGate>
      <p>FLOW</p>
    </ActivationGate>,
  );
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function submitKey() {
  fireEvent.change(screen.getByLabelText("Clé de licence"), {
    target: { value: "ABCD-EFGH-IJKL-MNOP" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  await advance(0);
}

describe("activation gate never waits forever", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvoke.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("a licence check with no answer ends on 'Serveur injoignable' with a retry", async () => {
    renderGate({
      get_product_license_onboarding_state: () => NOT_ACTIVATED,
      get_app_info: () => APP_INFO,
      preview_product_activation: never,
    });
    await advance(0);
    await submitKey();
    expect(screen.getByRole("button", { name: "Vérification…" })).toBeDisabled();

    await advance(SLOW_HINT_MS);
    expect(screen.getByText("Le serveur met du temps à répondre…")).toBeInTheDocument();

    await advance(UI_DEADLINE_MS - SLOW_HINT_MS);
    expect(screen.getByText("Serveur injoignable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Vérification…" })).toBeNull();
    expect(screen.getByRole("button", { name: "Continuer" })).toBeEnabled();
  });

  it("the person can stop waiting for a slow licence check", async () => {
    renderGate({
      get_product_license_onboarding_state: () => NOT_ACTIVATED,
      get_app_info: () => APP_INFO,
      preview_product_activation: never,
    });
    await advance(0);
    await submitKey();
    expect(screen.queryByRole("button", { name: "Annuler" })).toBeNull();

    await advance(SLOW_HINT_MS);
    fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    await advance(0);
    expect(screen.getByRole("button", { name: "Continuer" })).toBeEnabled();

    // The abandoned check's later deadline changes nothing.
    await advance(UI_DEADLINE_MS);
    expect(screen.queryByText("Serveur injoignable")).toBeNull();
  });

  it("a first state check with no answer ends on an error with a retry", async () => {
    renderGate({ get_product_license_onboarding_state: never });
    await advance(0);
    expect(screen.getByText("Vérification de l'activation…")).toBeInTheDocument();

    await advance(UI_DEADLINE_MS);
    expect(screen.getByText("Serveur injoignable")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Réessayer" })).toBeInTheDocument();
    expect(screen.queryByText("FLOW")).toBeNull();
  });

  it("an engine refusal still shows at once", async () => {
    renderGate({
      get_product_license_onboarding_state: () => NOT_ACTIVATED,
      get_app_info: () => APP_INFO,
      preview_product_activation: () => {
        throw { code: "offline", message: "Cannot reach the control API" };
      },
    });
    await advance(0);
    await submitKey();
    expect(screen.getByText("Serveur injoignable")).toBeInTheDocument();
  });
});
