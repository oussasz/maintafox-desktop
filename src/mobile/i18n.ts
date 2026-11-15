// Self-contained i18n of the mobile (Android) shell. It does not touch the desktop i18n
// (`src/i18n/`): its own i18next instance, its own resources (`./locales/{fr,en,ar}.json`),
// provided to React through `<I18nextProvider>` (never through the global instance).
//
// Arabic is right-to-left: every language change sets `dir` and `lang` on <html>.

import i18next, { type i18n as I18nInstance } from "i18next";
import { useTranslation } from "react-i18next";

import ar from "./locales/ar.json";
import en from "./locales/en.json";
import fr from "./locales/fr.json";

export const MOBILE_LANGUAGES = ["fr", "ar", "en"] as const;
export type MobileLanguage = (typeof MOBILE_LANGUAGES)[number];

export const MOBILE_DEFAULT_LANGUAGE: MobileLanguage = "fr";
export const MOBILE_FALLBACK_LANGUAGE: MobileLanguage = "en";
export const MOBILE_LANGUAGE_STORAGE_KEY = "maintafox-mobile:language";

export function isMobileLanguage(value: unknown): value is MobileLanguage {
  return typeof value === "string" && (MOBILE_LANGUAGES as readonly string[]).includes(value);
}

/** Text direction of a language: only Arabic is right-to-left. */
export function directionOf(language: MobileLanguage): "rtl" | "ltr" {
  return language === "ar" ? "rtl" : "ltr";
}

/** Sets `lang` and `dir` on the document so the layout mirrors for Arabic. */
export function applyDocumentLanguage(language: MobileLanguage): void {
  document.documentElement.lang = language;
  document.documentElement.dir = directionOf(language);
}

function storedLanguage(): MobileLanguage | null {
  try {
    const value = localStorage.getItem(MOBILE_LANGUAGE_STORAGE_KEY);
    return isMobileLanguage(value) ? value : null;
  } catch {
    return null;
  }
}

function rememberLanguage(language: MobileLanguage): void {
  try {
    localStorage.setItem(MOBILE_LANGUAGE_STORAGE_KEY, language);
  } catch {
    // Storage unavailable: the choice only lasts for this run.
  }
}

/** A new instance (tests build their own); the app uses {@link mobileI18n}. */
export function createMobileI18n(initialLanguage?: MobileLanguage): I18nInstance {
  const instance = i18next.createInstance();
  const language = initialLanguage ?? storedLanguage() ?? MOBILE_DEFAULT_LANGUAGE;
  void instance.init({
    resources: {
      fr: { translation: fr },
      en: { translation: en },
      ar: { translation: ar },
    },
    lng: language,
    fallbackLng: MOBILE_FALLBACK_LANGUAGE,
    supportedLngs: [...MOBILE_LANGUAGES],
    interpolation: { escapeValue: false },
    initAsync: false,
  });
  applyDocumentLanguage(language);
  instance.on("languageChanged", (next) => {
    if (isMobileLanguage(next)) {
      applyDocumentLanguage(next);
      rememberLanguage(next);
    }
  });
  return instance;
}

export const mobileI18n: I18nInstance = createMobileI18n();

/** Translate a key of the mobile resources (the desktop's typed keys do not apply here). */
export type MobileT = (key: string, options?: Record<string, unknown>) => string;

export function useMobileT(): { t: MobileT; language: MobileLanguage } {
  const { t, i18n } = useTranslation();
  const language = isMobileLanguage(i18n.resolvedLanguage)
    ? i18n.resolvedLanguage
    : MOBILE_DEFAULT_LANGUAGE;
  return { t: t as unknown as MobileT, language };
}
