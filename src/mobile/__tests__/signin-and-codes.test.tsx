import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { formatEnrolmentCode, isCompleteEnrolmentCode } from "../lib/enrolment-code";
import { classifyError } from "../lib/user-error";
import ar from "../locales/ar.json";
import en from "../locales/en.json";
import fr from "../locales/fr.json";
import { SignInScreen } from "../screens/SignInScreen";
import { ENROLMENT_ERROR_CODES, PHONE_SIGN_IN_ERROR_CODES } from "../services/enrolment";

import { ipcAnswering, renderMobile } from "./helpers";

function phoneRefusal(code: string, params: Record<string, string> = {}) {
  return {
    code: "VALIDATION_FAILED",
    message: `${code}: technical text`,
    details: [{ code, message: "technical text", params }],
  };
}

function bootstrap(phoneBinding: { person_label: string; site_label: string } | null) {
  return () => ({
    tenant_id: "tenant-1",
    company_display_name: "Usine",
    has_tenant_admin: true,
    plant_join: "sign_in",
    phone_binding: phoneBinding,
  });
}

async function signIn(error: unknown, language: "fr" | "en" = "fr") {
  mockInvoke.mockImplementation(
    ipcAnswering({
      get_activation_bootstrap_state: bootstrap(null),
      login: () => {
        throw error;
      },
    }),
  );
  renderMobile(<SignInScreen onSignedIn={() => undefined} />, language);
  fireEvent.change(screen.getByLabelText(language === "fr" ? "Nom d'utilisateur" : "Username"), {
    target: { value: "kbenali" },
  });
  fireEvent.change(screen.getByLabelText(language === "fr" ? "Mot de passe" : "Password"), {
    target: { value: "secret" },
  });
  fireEvent.click(
    screen.getByRole("button", { name: language === "fr" ? "Se connecter" : "Sign in" }),
  );
  await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
}

describe("sign-in on an enrolled phone", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("another person than the enrolled one is told whose phone it is", async () => {
    await signIn(phoneRefusal("PHONE_ASSIGNED_TO_OTHER_PERSON", { person_label: "Karim Benali" }));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Ce téléphone est attribué à Karim Benali. Seule cette personne peut s'y connecter.",
    );
  });

  it("a phone that must be enrolled is explained", async () => {
    await signIn(phoneRefusal("PHONE_NOT_ENROLLED"));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "doit être enrôlé par votre administrateur",
    );
  });

  it("a phone that is erasing itself is explained", async () => {
    await signIn(phoneRefusal("PHONE_REMOVAL_IN_PROGRESS"));
    expect(screen.getByRole("alert")).toHaveTextContent("puis s'efface");
  });

  it("the person's name needs no escaping, and shows in English too", async () => {
    await signIn(
      phoneRefusal("PHONE_ASSIGNED_TO_OTHER_PERSON", { person_label: "O'Brien <A&B>" }),
      "en",
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "This phone is assigned to O'Brien <A&B>. Only that person can sign in on it.",
    );
  });
});

describe("every refusal code has a text", () => {
  type Entry = { title: string; text: string };
  const entryOf = (tree: unknown, kind: string) =>
    (tree as { errors: Record<string, Entry> }).errors[kind];

  it.each([...ENROLMENT_ERROR_CODES, ...PHONE_SIGN_IN_ERROR_CODES])(
    "%s is a specific sentence in fr, en and ar",
    (code) => {
      const { kind } = classifyError(phoneRefusal(code, { person_label: "X" }), "activation");
      expect(kind).not.toBe("generic");
      for (const lang of [fr, en, ar]) {
        const entry = entryOf(lang, kind);
        expect(entry?.title.length).toBeGreaterThan(0);
        expect(entry?.text.length).toBeGreaterThan(0);
      }
    },
  );
});

describe("the phone's person before sign-in", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("an enrolled phone names its person and site, never an id", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({
        get_activation_bootstrap_state: bootstrap({
          person_label: "Amina Benali",
          site_label: "Usine Nord",
        }),
      }),
    );
    renderMobile(<SignInScreen onSignedIn={() => undefined} />);
    expect(await screen.findByText("Téléphone de Amina Benali — Usine Nord")).toBeInTheDocument();
  });

  it("a phone without binding keeps the usual subtitle", async () => {
    mockInvoke.mockImplementation(
      ipcAnswering({ get_activation_bootstrap_state: bootstrap(null) }),
    );
    renderMobile(<SignInScreen onSignedIn={() => undefined} />);
    expect(
      await screen.findByText("Connectez-vous avec votre compte Maintafox."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Téléphone de/)).toBeNull();
  });

  it("a locked phone refuses sign-in and says the data is kept", async () => {
    await signIn(phoneRefusal("PHONE_LOCKED"));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Ce téléphone est bloqué : le serveur l'a refusé sans confirmation. Les données sont conservées. Contactez votre administrateur.",
    );
  });
});

describe("typed enrolment code", () => {
  it("is grouped XXXX-XXXX, upper-cased, and drops spaces, dashes and other characters", () => {
    expect(formatEnrolmentCode("")).toBe("");
    expect(formatEnrolmentCode("ab12")).toBe("AB12");
    expect(formatEnrolmentCode("ab12c")).toBe("AB12-C");
    expect(formatEnrolmentCode(" ab12 - cd34 ")).toBe("AB12-CD34");
    expect(formatEnrolmentCode("ab12cd34ef56")).toBe("AB12-CD34");
    expect(formatEnrolmentCode("ab-12_cd.34")).toBe("AB12-CD34");
  });

  it("is complete at 8 characters only", () => {
    expect(isCompleteEnrolmentCode("AB12-CD3")).toBe(false);
    expect(isCompleteEnrolmentCode("AB12-CD34")).toBe(true);
  });
});
