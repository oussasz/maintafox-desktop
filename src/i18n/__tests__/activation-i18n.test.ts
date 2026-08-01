import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vitest";

import enAuth from "../en/auth.json";
import frAuth from "../fr/auth.json";

/** Keys referenced by ProductLicenseGate + LoginPage activation UX. */
const ACTIVATION_UI_KEYS = [
  "activation.title",
  "activation.subtitle",
  "activation.steps.validate",
  "activation.steps.activate",
  "activation.steps.finish",
  "activation.keyLabel",
  "activation.keyPlaceholder",
  "activation.validate",
  "activation.validating",
  "activation.verifiedTitle",
  "activation.verifiedBody",
  "activation.slotNote",
  "activation.deviceNameLabel",
  "activation.deviceNamePlaceholder",
  "activation.activateCta",
  "activation.activating",
  "activation.retryFinalize",
  "activation.changeKey",
  "activation.backToLogin",
  "activation.companyBelongsTo",
  "activation.fields.slug",
  "activation.fields.plan",
  "activation.fields.status",
  "activation.fields.channel",
  "activation.fields.expiration",
  "activation.fields.allowedDevices",
  "activation.fields.activatedDevices",
  "activation.fields.deviceName",
  "activation.fields.notSpecified",
  "activation.status.licenseVerified",
  "activation.status.connected",
  "activation.status.waiting",
  "activation.technicalDetails",
  "activation.showTechnicalDetails",
  "activation.hideTechnicalDetails",
  "activation.denied.revokedTitle",
  "activation.denied.revokedBody",
  "activation.denied.expiredTitle",
  "activation.denied.expiredBody",
  "activation.denied.slotTitle",
  "activation.denied.slotBody",
  "activation.denied.forceUpdateTitle",
  "activation.denied.forceUpdateBody",
  "activation.denied.invalidTitle",
  "activation.denied.invalidBody",
  "activation.denied.revalidate",
  "activation.denied.revalidating",
  "activation.diag.status",
  "activation.diag.online",
  "activation.diag.retryAttempt",
  "activation.diag.nextRetry",
  "activation.diag.lastReconciled",
  "activation.diag.lastError",
  "activation.diag.referenceCode",
  "activation.offlineWarning",
  "activation.loading",
  "login.activationRequired.title",
  "login.activationRequired.message",
  "login.activationRequired.cta",
  "login.licenseDetails.show",
  "login.licenseDetails.hide",
  "login.licenseDetails.tier",
  "login.licenseDetails.slotLimit",
  "login.licenseDetails.expiry",
  "login.licenseDetails.company",
  "login.licenseDetails.na",
  "login.postActivationHint",
  "login.tenantScopeDenied.title",
  "login.tenantScopeDenied.action",
  "login.staleSession.title",
  "login.staleSession.action",
  "login.staleSession.resetCta",
  "login.staleSession.resetting",
  "login.resetActivation.hint",
  "login.resetActivation.cta",
  "login.resetActivation.resetting",
  "login.resetActivation.errorFallback",
] as const;

const HTML_ENTITY_PATTERN = /&(apos|amp|lt|gt|quot);/;

const i18n = i18next.createInstance();

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "fr",
    fallbackLng: "en",
    ns: ["auth"],
    defaultNS: "auth",
    // Mirror production: auth is an eager bundled resource (not locale-data).
    resources: {
      fr: { auth: frAuth },
      en: { auth: enAuth },
    },
    interpolation: { escapeValue: false },
    parseMissingKeyHandler: (key: string, defaultValue?: string) => defaultValue ?? `[${key}]`,
    saveMissing: false,
  });
});

function assertResolved(value: string, key: string, locale: string): void {
  expect(value, `${locale} ${key} should resolve`).not.toMatch(/^\[.+\]$/);
  expect(value, `${locale} ${key} should not equal the key path`).not.toBe(key);
  expect(value.trim().length, `${locale} ${key} should be non-empty`).toBeGreaterThan(0);
}

describe("activation auth i18n (eager namespace)", () => {
  it("resolves every activation UI key in English", async () => {
    await i18n.changeLanguage("en");
    for (const key of ACTIVATION_UI_KEYS) {
      assertResolved(i18n.t(key, { ns: "auth" }), key, "en");
    }
    expect(i18n.t("activation.title", { ns: "auth" })).toBe("Product activation");
    expect(i18n.t("activation.validate", { ns: "auth" })).toBe("Validate license");
    expect(i18n.t("activation.activateCta", { ns: "auth" })).toBe("Activate this device");
  });

  it("resolves every activation UI key in French", async () => {
    await i18n.changeLanguage("fr");
    for (const key of ACTIVATION_UI_KEYS) {
      assertResolved(i18n.t(key, { ns: "auth" }), key, "fr");
    }
    expect(i18n.t("activation.title", { ns: "auth" })).toBe("Activation produit");
    expect(i18n.t("activation.validate", { ns: "auth" })).toBe("Valider la licence");
    expect(i18n.t("activation.activateCta", { ns: "auth" })).toBe("Activer cet appareil");
    expect(i18n.t("login.staleSession.resetCta", { ns: "auth" })).toBe(
      "Réinitialiser l'activation / utiliser une autre clé",
    );
    expect(i18n.t("login.resetActivation.cta", { ns: "auth" })).toBe(
      "Réinitialiser l'activation / autre clé",
    );
  });

  it("uses missing-key fallback brackets when a key is absent in both locales", async () => {
    await i18n.changeLanguage("fr");
    const missing = i18n.t("activation.thisKeyDoesNotExistAnywhere" as never, { ns: "auth" });
    expect(missing).toBe("[activation.thisKeyDoesNotExistAnywhere]");
  });

  it("does not leave any activation UI key rendering as [key]", async () => {
    for (const lng of ["en", "fr"] as const) {
      await i18n.changeLanguage(lng);
      const unresolved = ACTIVATION_UI_KEYS.filter((key) => {
        const value = i18n.t(key, { ns: "auth" });
        return value.startsWith("[") && value.endsWith("]");
      });
      expect(unresolved, `${lng} unresolved keys`).toEqual([]);
    }
  });

  it("does not contain HTML entities in activation-related auth strings", async () => {
    for (const lng of ["en", "fr"] as const) {
      await i18n.changeLanguage(lng);
      for (const key of ACTIVATION_UI_KEYS) {
        const value = i18n.t(key, { ns: "auth" });
        expect(value, `${lng} ${key}`).not.toMatch(HTML_ENTITY_PATTERN);
      }
    }
  });
});
