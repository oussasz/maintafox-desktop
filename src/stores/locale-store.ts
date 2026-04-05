// src/stores/locale-store.ts
// Zustand store for locale state and direction (RTL readiness).
// Reads locale preference from the Rust backend via IPC and drives
// i18next language switching.

import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";

import { i18n, DEFAULT_LOCALE } from "@/i18n";
import { getLocaleDirection } from "@/utils/formatters";
import type { LocalePreference } from "@shared/ipc-types";

export interface LocaleState {
  /** Current active locale code ("fr" | "en"). */
  activeLocale: string;
  /** Text direction for the active locale ("ltr" or "rtl"). */
  direction: "ltr" | "rtl";
  /** True while locale preference is being loaded or changed. */
  isLoading: boolean;
  /** Last error from locale IPC, or null. */
  error: string | null;
  /** Locales supported by the backend. */
  supportedLocales: string[];
  /** Read locale preference from backend, update i18next. */
  initialize: () => Promise<void>;
  /** Switch locale, persist via backend, update i18next. */
  setLocale: (locale: string, asTenantDefault?: boolean) => Promise<void>;
}

export const useLocaleStore = create<LocaleState>()((set) => ({
  activeLocale: DEFAULT_LOCALE,
  direction: "ltr",
  isLoading: true,
  error: null,
  supportedLocales: ["fr", "en"],

  initialize: async () => {
    try {
      set({ isLoading: true, error: null });
      const pref = await invoke<LocalePreference>("get_locale_preference");
      const locale = pref.active_locale;
      await i18n.changeLanguage(locale);
      set({
        activeLocale: locale,
        direction: getLocaleDirection(locale),
        supportedLocales: pref.supported_locales,
        isLoading: false,
      });
    } catch (err) {
      // Fallback to default locale if IPC fails (e.g., during dev without Tauri bridge)
      console.warn("locale-store: failed to read preference, using default", err);
      set({
        activeLocale: DEFAULT_LOCALE,
        direction: getLocaleDirection(DEFAULT_LOCALE),
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },

  setLocale: async (locale: string, asTenantDefault?: boolean) => {
    try {
      set({ isLoading: true, error: null });
      await invoke<LocalePreference>("set_locale_preference", {
        payload: { locale, asTenantDefault: asTenantDefault ?? false },
      });
      await i18n.changeLanguage(locale);
      set({
        activeLocale: locale,
        direction: getLocaleDirection(locale),
        isLoading: false,
      });
    } catch (err) {
      console.error("locale-store: failed to set preference", err);
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      });
    }
  },
}));
