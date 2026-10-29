import { act, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { SLOW_HINT_MS, UI_DEADLINE_MS } from "../lib/ui-deadline";
import { ActivationGate } from "../screens/ActivationGate";

import { type IpcHandlers, ipcAnswering, renderMobile } from "./helpers";

// The camera plugin is the only thing replaced besides the IPC boundary.
const scanner = vi.hoisted(() => ({
  checkPermissions: vi.fn(),
  requestPermissions: vi.fn(),
  scan: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-barcode-scanner", () => ({
  Format: { QRCode: "QR_CODE" },
  ...scanner,
}));

const NOT_ACTIVATED = {
  complete: false,
  status: "uninitialized",
  pending_online_validation: false,
};

const ACTIVATED = {
  complete: true,
  status: "active",
  pending_online_validation: false,
  tenant_id: "tnt-1",
};

const REDEEMED = {
  tenant_id: "tnt-1",
  company_display_name: "Usine Blida",
  person_sync_id: "person-1",
  person_label: "Karim Benali",
  site_sync_id: "site-1",
  site_label: "Atelier Nord",
  enrolled_at: "2026-10-29T10:00:00Z",
};

/** The QR text exactly as scanned: the screen must pass it on untouched. */
const SCANNED = "  maintafox-enrol:1?s=https%3A%2F%2Fmfx.example&t=AbC-123_xyz \n";

/** An engine call that never answers (a panic in the engine, a lost IPC answer). */
const never = () => new Promise<never>(() => undefined);

/** An engine refusal as the app serialises it: the stable rule code is in `details`. */
function refusal(code: string, params: Record<string, string> = {}) {
  return {
    code: "VALIDATION_FAILED",
    message: `${code}: technical text`,
    details: [{ code, message: "technical text", params }],
  };
}

function renderGate(handlers: IpcHandlers) {
  mockInvoke.mockImplementation(
    ipcAnswering({ get_product_license_onboarding_state: () => NOT_ACTIVATED, ...handlers }),
  );
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

async function openCodeEntry() {
  await advance(0);
  fireEvent.click(screen.getByRole("button", { name: "Saisir le code" }));
}

async function typeCode(value: string) {
  fireEvent.change(screen.getByLabelText("Code d'activation"), { target: { value } });
  await advance(0);
}

async function submitCode(value = "K7M2-93QA") {
  await typeCode(value);
  fireEvent.click(screen.getByRole("button", { name: "Activer le téléphone" }));
  await advance(0);
}

function redeemCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "redeem_mobile_enrolment");
}

describe("phone enrolment screen", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvoke.mockReset();
    scanner.checkPermissions.mockReset();
    scanner.requestPermissions.mockReset();
    scanner.scan.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("offers the QR code first and the typed code second, and no licence key", async () => {
    renderGate({});
    await advance(0);
    expect(screen.getByRole("button", { name: "Scanner le QR code" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Saisir le code" })).toBeEnabled();
    expect(screen.queryByLabelText("Clé de licence")).toBeNull();
    expect(screen.queryByText(/clé de licence/i)).toBeNull();
  });

  it("a scanned QR code is sent untouched, then shows who the phone is for and where", async () => {
    let activated = false;
    renderGate({
      get_product_license_onboarding_state: () => (activated ? ACTIVATED : NOT_ACTIVATED),
      get_activation_bootstrap_state: () => ({
        tenant_id: "tnt-1",
        has_tenant_admin: true,
        plant_join: "sign_in",
      }),
      redeem_mobile_enrolment: () => {
        activated = true;
        return REDEEMED;
      },
    });
    scanner.checkPermissions.mockResolvedValue("granted");
    scanner.scan.mockResolvedValue({ content: SCANNED, format: "QR_CODE" });

    await advance(0);
    fireEvent.click(screen.getByRole("button", { name: "Scanner le QR code" }));
    await advance(0);

    expect(scanner.scan).toHaveBeenCalledWith({ formats: ["QR_CODE"] });
    expect(redeemCalls()).toEqual([
      ["redeem_mobile_enrolment", { input: { qr_payload: SCANNED } }],
    ]);
    expect(
      screen.getByText("Téléphone attribué à Karim Benali — Atelier Nord"),
    ).toBeInTheDocument();
    expect(screen.queryByText("FLOW")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
    await advance(0);
    expect(screen.getByText("FLOW")).toBeInTheDocument();
  });

  it("a cancelled scan returns to the choice with no error and no enrolment", async () => {
    renderGate({});
    scanner.checkPermissions.mockResolvedValue("granted");
    scanner.scan.mockRejectedValue(new Error("cancelled"));

    await advance(0);
    fireEvent.click(screen.getByRole("button", { name: "Scanner le QR code" }));
    await advance(0);

    expect(screen.getByRole("button", { name: "Scanner le QR code" })).toBeEnabled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByText(/n'a pas abouti/)).toBeNull();
    expect(redeemCalls()).toEqual([]);
  });

  it("a scan that fails says so and offers the code", async () => {
    renderGate({});
    scanner.checkPermissions.mockResolvedValue("granted");
    scanner.scan.mockRejectedValue(new Error("camera unavailable"));

    await advance(0);
    fireEvent.click(screen.getByRole("button", { name: "Scanner le QR code" }));
    await advance(0);

    expect(
      screen.getByText("Le scan n'a pas abouti. Réessayez ou saisissez le code."),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Saisir le code" })).toBeEnabled();
  });

  it("explains the camera in French before asking, and a refusal falls back to the code", async () => {
    renderGate({});
    scanner.checkPermissions.mockResolvedValue("prompt");
    scanner.requestPermissions.mockResolvedValue("denied");

    await advance(0);
    fireEvent.click(screen.getByRole("button", { name: "Scanner le QR code" }));
    await advance(0);
    expect(
      screen.getByText(/utilise l'appareil photo uniquement pour lire le QR code/),
    ).toBeInTheDocument();
    expect(scanner.requestPermissions).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Autoriser l'appareil photo" }));
    await advance(0);

    expect(scanner.scan).not.toHaveBeenCalled();
    expect(screen.getByText(/L'appareil photo n'est pas autorisé/)).toBeInTheDocument();
    expect(screen.getByLabelText("Code d'activation")).toBeInTheDocument();
  });

  it("a camera already refused goes straight to the code", async () => {
    renderGate({});
    scanner.checkPermissions.mockResolvedValue("denied");

    await advance(0);
    fireEvent.click(screen.getByRole("button", { name: "Scanner le QR code" }));
    await advance(0);

    expect(scanner.requestPermissions).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Code d'activation")).toBeInTheDocument();
  });

  it("the typed code is upper-cased, grouped XXXX-XXXX and tolerant of spaces and dashes", async () => {
    renderGate({ redeem_mobile_enrolment: () => REDEEMED });
    await openCodeEntry();
    const field = screen.getByLabelText("Code d'activation") as HTMLInputElement;
    const submit = screen.getByRole("button", { name: "Activer le téléphone" });

    await typeCode("k7m2");
    expect(field.value).toBe("K7M2");
    expect(submit).toBeDisabled();

    await typeCode("k7m2 93q");
    expect(field.value).toBe("K7M2-93Q");
    expect(submit).toBeDisabled();

    await typeCode(" k7m2-93qa-zzzz ");
    expect(field.value).toBe("K7M2-93QA");
    expect(submit).toBeEnabled();

    fireEvent.click(submit);
    await advance(0);
    expect(redeemCalls()).toEqual([["redeem_mobile_enrolment", { input: { code: "K7M2-93QA" } }]]);
    expect(
      screen.getByText("Téléphone attribué à Karim Benali — Atelier Nord"),
    ).toBeInTheDocument();
  });

  it.each([
    [
      "ENROLMENT_CODE_INVALID",
      "Code invalide ou expiré. Demandez un nouveau QR code à votre administrateur.",
    ],
    [
      "ENROLMENT_INVALID",
      "Code invalide ou expiré. Demandez un nouveau QR code à votre administrateur.",
    ],
    ["ENROLMENT_QR_OTHER_SERVER", "Ce QR code ne vient pas de votre serveur Maintafox."],
    [
      "ENROLMENT_WIPE_PENDING",
      "Ce téléphone doit d'abord terminer son effacement (envoi des travaux en attente, puis effacement). Gardez la connexion, puis réessayez.",
    ],
    ["ENROLMENT_RATE_LIMITED", "Trop de tentatives. Patientez quelques minutes, puis réessayez."],
    ["ENROLMENT_NO_FREE_SLOT", /Tous les téléphones autorisés par la licence/],
    ["ENROLMENT_OFFLINE", /Vérifiez la connexion du téléphone/],
  ])("a refused enrolment (%s) is explained in French", async (code, text) => {
    renderGate({
      redeem_mobile_enrolment: () => {
        throw refusal(code);
      },
    });
    await openCodeEntry();
    await submitCode();
    expect(screen.getByRole("alert")).toHaveTextContent(text);
    expect(screen.queryByText("FLOW")).toBeNull();
  });

  it("a phone whose earlier answer was lost moves on when the server says it is activated", async () => {
    let calls = 0;
    renderGate({
      get_product_license_onboarding_state: () => (calls > 0 ? ACTIVATED : NOT_ACTIVATED),
      get_activation_bootstrap_state: () => ({
        tenant_id: "tnt-1",
        has_tenant_admin: true,
        plant_join: "sign_in",
      }),
      redeem_mobile_enrolment: () => {
        calls += 1;
        throw refusal("ENROLMENT_ALREADY_ACTIVATED");
      },
    });
    await openCodeEntry();
    await submitCode();
    expect(screen.getByText("FLOW")).toBeInTheDocument();
  });
});

describe("activation gate never waits forever", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvoke.mockReset();
    scanner.checkPermissions.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("an enrolment with no answer ends on 'Serveur injoignable' with a retry of the same code", async () => {
    renderGate({ redeem_mobile_enrolment: never });
    await openCodeEntry();
    await submitCode();
    expect(screen.getByRole("button", { name: "Activation du téléphone…" })).toBeDisabled();

    await advance(SLOW_HINT_MS);
    expect(screen.getByText("Le serveur met du temps à répondre…")).toBeInTheDocument();

    await advance(UI_DEADLINE_MS - SLOW_HINT_MS);
    expect(screen.getByText("Serveur injoignable")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Activation du téléphone…" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
    await advance(0);
    expect(redeemCalls()).toEqual([
      ["redeem_mobile_enrolment", { input: { code: "K7M2-93QA" } }],
      ["redeem_mobile_enrolment", { input: { code: "K7M2-93QA" } }],
    ]);
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
      redeem_mobile_enrolment: () => {
        throw refusal("ENROLMENT_OFFLINE");
      },
    });
    await openCodeEntry();
    await submitCode();
    expect(screen.getByText("Serveur injoignable")).toBeInTheDocument();
  });
});
