/**
 * Policy draft test results are shown in the user's language from their code and values; a code
 * this version does not know shows the server's text.
 */
import i18next from "i18next";
import { beforeAll, describe, expect, it } from "vitest";

import { policyTestText } from "@/components/settings/policy-test-text";
import enSettings from "@/i18n/locale-data/en/settings.json";
import frSettings from "@/i18n/locale-data/fr/settings.json";
import type { PolicyTestResult } from "@shared/ipc-types";

const i18n = i18next.createInstance();
const t = (key: string, opts?: Record<string, unknown>): string =>
  String(i18n.t(key as never, (opts ?? {}) as never));

function result(code: string, params: Record<string, string>, message = "server text") {
  return { rule_name: "r", severity: "fail", code, params, message } as PolicyTestResult;
}

beforeAll(async () => {
  await i18n.init({
    lng: "fr",
    fallbackLng: "en",
    ns: ["settings"],
    defaultNS: "settings",
    resources: { fr: { settings: frSettings }, en: { settings: enSettings } },
    interpolation: { escapeValue: false },
  });
});

describe("policyTestText", () => {
  it("names the field and its range in the user's language", async () => {
    await i18n.changeLanguage("fr");
    const out = result("POLICY_TEST_OUT_OF_RANGE", {
      field: "idle_timeout_minutes",
      value: "600",
      min: "1",
      max: "480",
    });
    expect(policyTestText(out, t)).toBe(
      "Délai d'inactivité (minutes) : 600 est hors limites (1 à 480).",
    );
    await i18n.changeLanguage("en");
    expect(policyTestText(out, t)).toBe("Idle Timeout (minutes): 600 is out of range (1 to 480).");
  });

  it("translates yes / no and plain notes", async () => {
    await i18n.changeLanguage("fr");
    expect(policyTestText(result("POLICY_TEST_INCLUDE_PHOTOS", { value: "false" }), t)).toBe(
      "Photos incluses dans les sauvegardes : non.",
    );
    expect(policyTestText(result("POLICY_TEST_PASSWORD_ELSEWHERE", {}), t)).toBe(
      "La politique de mot de passe se règle dans son propre panneau.",
    );
  });

  it("keeps the server's text for a code this version does not know", () => {
    expect(policyTestText(result("POLICY_TEST_SOMETHING_NEW", {}, "New check passed"), t)).toBe(
      "New check passed",
    );
  });
});
