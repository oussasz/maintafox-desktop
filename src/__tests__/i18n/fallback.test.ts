import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { describe, it, expect, beforeAll } from "vitest";

// Initialize a test-specific i18next instance without the lazy backend.
// We load only the eager namespaces in-memory for fast unit tests.

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
  if (i18n.isInitialized) return;

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
    // "action.onlyInEnglish" is not in the fr resources
    // @ts-expect-error — deliberate test key not in typed namespace
    const result = i18n.t("action.onlyInEnglish", { ns: "common" });
    expect(result).toBe("Only in English");
  });

  it("returns [key] pattern when key is missing in BOTH fr and en", () => {
    // @ts-expect-error — deliberate test key not in typed namespace
    const result = i18n.t("action.nonExistentKey", { ns: "common" });
    // parseMissingKeyHandler returns "[key]" not an empty string
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
