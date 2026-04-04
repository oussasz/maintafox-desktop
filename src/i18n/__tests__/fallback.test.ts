import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { describe, it, expect, beforeAll } from "vitest";

// Use a dedicated i18next instance so this test is isolated from any
// other test file that might initialize the global singleton.
const i18n = i18next.createInstance();

const FRENCH_COMMON = {
  "app.name": "Maintafox",
  "action.save": "Enregistrer",
  "action.cancel": "Annuler",
};

const ENGLISH_COMMON = {
  "app.name": "Maintafox",
  "action.save": "Save",
  "action.cancel": "Cancel",
  // Deliberately add a key that only exists in English (for fallback test)
  "action.onlyInEnglish": "Only in English",
};

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "fr",
    fallbackLng: "en",
    ns: ["common"],
    defaultNS: "common",
    resources: {
      fr: { common: FRENCH_COMMON },
      en: { common: ENGLISH_COMMON },
    },
    interpolation: { escapeValue: false },
    parseMissingKeyHandler: (key: string) => `[${key}]`,
    saveMissing: false,
  });
});

describe("i18n fallback chain", () => {
  it("returns French string when key exists in fr", () => {
    expect(i18n.t("action.save", { ns: "common" })).toBe("Enregistrer");
  });

  it("returns English fallback when key is missing in fr but exists in en", () => {
    const result = i18n.t("action.onlyInEnglish", { ns: "common" });
    expect(result).toBe("Only in English");
  });

  it("returns [key] pattern when key is missing in BOTH fr and en", () => {
    const result = i18n.t("action.nonExistentKey", { ns: "common" });
    expect(result).toContain("nonExistentKey");
    expect(result.startsWith("[")).toBe(true);
  });

  it("returns correct app name in both locales", () => {
    expect(i18n.t("app.name", { ns: "common", lng: "fr" })).toBe("Maintafox");
    expect(i18n.t("app.name", { ns: "common", lng: "en" })).toBe("Maintafox");
  });

  it("returns French string when explicitly requesting fr locale", () => {
    expect(i18n.t("action.cancel", { ns: "common", lng: "fr" })).toBe("Annuler");
  });

  it("returns English string when explicitly requesting en locale", () => {
    expect(i18n.t("action.cancel", { ns: "common", lng: "en" })).toBe("Cancel");
  });
});
