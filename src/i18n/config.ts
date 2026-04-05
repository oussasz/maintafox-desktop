// src/i18n/config.ts
// i18next initializer with lazy backend loading.
// Replaces the static import pattern from SP02-F03.

import i18n from "i18next";
import resourcesToBackend from "i18next-resources-to-backend";
import { initReactI18next } from "react-i18next";

// Eager resources: imported directly to guarantee availability at frame 0.
// These are special-cased because lazy loading would cause a visible flicker
// on the shell layout and login screen.

import enAuth from "./en/auth.json";
import enCommon from "./en/common.json";
import enErrors from "./en/errors.json";
import enFormats from "./en/formats.json";
import enShell from "./en/shell.json";
import enValidation from "./en/validation.json";
import frAuth from "./fr/auth.json";
import frCommon from "./fr/common.json";
import frErrors from "./fr/errors.json";
import frFormats from "./fr/formats.json";
import frShell from "./fr/shell.json";
import frValidation from "./fr/validation.json";
import { EAGER_NAMESPACES, type SupportedLocale } from "./namespaces";

const eagerResources = {
  fr: {
    common: frCommon,
    auth: frAuth,
    errors: frErrors,
    validation: frValidation,
    formats: frFormats,
    shell: frShell,
  },
  en: {
    common: enCommon,
    auth: enAuth,
    errors: enErrors,
    validation: enValidation,
    formats: enFormats,
    shell: enShell,
  },
};

// The default locale is French. The locale can be changed at runtime via
// i18n.changeLanguage() after reading the user or tenant preference from
// system_config.
export const DEFAULT_LOCALE: SupportedLocale = "fr";
export const FALLBACK_LOCALE: SupportedLocale = "en";

// Initialize once (idempotent — safe to call multiple times).
let initialized = false;

export function initI18n(): void {
  if (initialized) return;
  initialized = true;

  void i18n
    .use(initReactI18next)
    // Lazy-loads module namespaces on demand via dynamic import.
    // The `ns` and `lng` arguments match the file path pattern:
    //   src/i18n/locale-data/{lng}/{ns}.json
    .use(resourcesToBackend((lng: string, ns: string) => import(`./locale-data/${lng}/${ns}.json`)))
    .init({
      // Eager resources are pre-loaded; lazy content goes through the backend.
      resources: eagerResources,
      // Default language is French.
      lng: DEFAULT_LOCALE,
      fallbackLng: FALLBACK_LOCALE,
      // Namespaces loaded eagerly — others are loaded on demand.
      ns: [...EAGER_NAMESPACES],
      defaultNS: "common",
      // No HTML escaping — React handles this.
      interpolation: { escapeValue: false },
      // Show the key name if a translation is missing (never empty string).
      // Example: missing key "equipment.detail.title" shows as-is in dev.
      parseMissingKeyHandler: (key: string, defaultValue?: string) => defaultValue ?? `[${key}]`,
      // Debug mode in development: logs missing keys to console.
      debug: import.meta.env.DEV,
      // React-specific settings.
      react: {
        useSuspense: false, // Avoids React Suspense requirement
        transSupportBasicHtmlNodes: true,
      },
      // Accept region-specific codes by mapping to base locale:
      // fr-DZ → fr, en-US → en
      load: "languageOnly",
    });
}

export { i18n };
