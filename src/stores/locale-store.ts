// src/stores/locale-store.ts
// Zustand store for locale state and direction (RTL readiness).
// Reads locale preference from the Rust backend via IPC and drives
// i18next language switching. Also holds tenant display currency from settings.

import { create } from "zustand";

import { i18n, DEFAULT_LOCALE } from "@/i18n";
import {
  FALLBACK_CURRENCY_CODE,
  LOCALE_CURRENCY_SETTING_KEY,
  normalizeCurrencyCode,
} from "@/lib/currency";
import { applyI18nFallbackLanguage } from "@/lib/i18n-fallback";
import { invoke } from "@/lib/ipc-invoke";
import { getSetting, listReferenceCurrencies } from "@/services/settings-service";
import { toErrorMessage } from "@/utils/errors";
import { getLocaleDirection } from "@/utils/formatters";
import type { LocalePreference } from "@shared/ipc-types";

/** Stored choice meaning "follow the Windows language" (see the backend `FOLLOW_OS_LOCALE`). */
export const FOLLOW_SYSTEM_LOCALE = "system";

const FALLBACK_LANGUAGE_SETTING_KEY = "locale.fallback_language";

export interface LocaleState {
  /** Current active locale code ("fr" | "en"). */
  activeLocale: string;
  /** True when the user explicitly chose to follow the Windows language. */
  followsSystemLanguage: boolean;
  /** Tenant display currency (ISO 4217), from `locale.currency_code` setting. */
  activeCurrencyCode: string;
  /** Symbol of the tenant currency from the reference list ("DA"), null until loaded. */
  activeCurrencySymbol: string | null;
  /** Decimal places of the tenant currency from the reference list (default 2). */
  activeCurrencyMinorUnits: number;
  /** Text direction for the active locale ("ltr" or "rtl"). */
  direction: "ltr" | "rtl";
  /** True while locale preference is being loaded or changed. */
  isLoading: boolean;
  /** Last error from locale IPC, or null. */
  error: string | null;
  /** Locales supported by the backend. */
  supportedLocales: string[];
  /** Read locale preference from backend, update i18next; load currency setting. */
  initialize: () => Promise<void>;
  /**
   * Switch locale, persist via backend, update i18next. `FOLLOW_SYSTEM_LOCALE` stores the choice
   * to follow the Windows language. Works before sign-in (kept for this PC).
   */
  setLocale: (locale: string, asTenantDefault?: boolean) => Promise<void>;
  /** Apply currency from Settings (in-memory); persistence is via setSetting. */
  setCurrencyCode: (code: string) => void;
  /** Re-read `locale.currency_code` from app_settings. */
  refreshCurrency: () => Promise<void>;
}

function parseCurrencyFromSettingJson(json: string | undefined | null): string {
  if (json == null || json === "") return FALLBACK_CURRENCY_CODE;
  try {
    return normalizeCurrencyCode(JSON.parse(json));
  } catch {
    return normalizeCurrencyCode(json);
  }
}

/** Symbol and decimals of `code` from the reference currency list; none when it cannot be read. */
async function readCurrencyDisplay(
  code: string,
): Promise<{ symbol: string | null; minorUnits: number } | null> {
  try {
    const row = (await listReferenceCurrencies()).find((c) => c.code === code);
    return { symbol: row?.symbol ?? null, minorUnits: row?.minor_units ?? 2 };
  } catch {
    // The list needs a session: until it loads (or if it cannot be read), what is shown stays.
    return null;
  }
}

export const useLocaleStore = create<LocaleState>()((set) => ({
  activeLocale: DEFAULT_LOCALE,
  followsSystemLanguage: false,
  activeCurrencyCode: FALLBACK_CURRENCY_CODE,
  activeCurrencySymbol: null,
  activeCurrencyMinorUnits: 2,
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

      let currency = FALLBACK_CURRENCY_CODE;
      try {
        const row = await getSetting(LOCALE_CURRENCY_SETTING_KEY, "tenant");
        currency = parseCurrencyFromSettingJson(row?.setting_value_json);
      } catch {
        // Settings may be unavailable before auth; keep fallback until refresh.
      }

      try {
        const fallbackRow = await getSetting(FALLBACK_LANGUAGE_SETTING_KEY, "tenant");
        const raw = fallbackRow?.setting_value_json;
        if (raw != null && raw !== "") {
          try {
            const parsed = JSON.parse(raw);
            if (typeof parsed === "string") applyI18nFallbackLanguage(parsed);
          } catch {
            applyI18nFallbackLanguage(raw);
          }
        }
      } catch {
        // Optional before auth; i18n keeps its configured fallbackLng.
      }

      // The symbol and decimals of the currency need a session: `refreshCurrency` (run by the
      // shell once signed in) loads them.
      set({
        activeLocale: locale,
        followsSystemLanguage: pref.user_locale === FOLLOW_SYSTEM_LOCALE,
        activeCurrencyCode: currency,
        direction: getLocaleDirection(locale),
        supportedLocales: pref.supported_locales,
        isLoading: false,
      });
    } catch (err) {
      // Fallback to default locale if IPC fails (e.g., during dev without Tauri bridge)
      console.warn("locale-store: failed to read preference, using default", err);
      set({
        activeLocale: DEFAULT_LOCALE,
        activeCurrencyCode: FALLBACK_CURRENCY_CODE,
        direction: getLocaleDirection(DEFAULT_LOCALE),
        isLoading: false,
        error: toErrorMessage(err),
      });
    }
  },

  setLocale: async (locale: string, asTenantDefault?: boolean) => {
    try {
      set({ isLoading: true, error: null });
      const pref = await invoke<LocalePreference>("set_locale_preference", {
        payload: { locale, asTenantDefault: asTenantDefault ?? false },
      });
      const follows = locale === FOLLOW_SYSTEM_LOCALE;
      const active = follows ? pref.active_locale : locale;
      await i18n.changeLanguage(active);
      set({
        activeLocale: active,
        followsSystemLanguage: follows,
        direction: getLocaleDirection(active),
        isLoading: false,
      });
    } catch (err) {
      console.error("locale-store: failed to set preference", err);
      set({
        isLoading: false,
        error: toErrorMessage(err),
      });
    }
  },

  setCurrencyCode: (code: string) => {
    const normalized = normalizeCurrencyCode(code);
    // The previous currency's symbol and decimals must not be shown with the new code.
    set({
      activeCurrencyCode: normalized,
      activeCurrencySymbol: null,
      activeCurrencyMinorUnits: 2,
    });
    void readCurrencyDisplay(normalized).then((display) =>
      set((s) =>
        display && s.activeCurrencyCode === normalized
          ? { activeCurrencySymbol: display.symbol, activeCurrencyMinorUnits: display.minorUnits }
          : s,
      ),
    );
  },

  refreshCurrency: async () => {
    try {
      const row = await getSetting(LOCALE_CURRENCY_SETTING_KEY, "tenant");
      const code = parseCurrencyFromSettingJson(row?.setting_value_json);
      const display = await readCurrencyDisplay(code);
      set((s) => {
        // Another currency than before gets no symbol of the old one; the same currency with
        // an unreadable list keeps what was shown.
        const same = code === s.activeCurrencyCode;
        return {
          activeCurrencyCode: code,
          activeCurrencySymbol: display ? display.symbol : same ? s.activeCurrencySymbol : null,
          activeCurrencyMinorUnits: display
            ? display.minorUnits
            : same
              ? s.activeCurrencyMinorUnits
              : 2,
        };
      });
    } catch (err) {
      console.warn("locale-store: failed to refresh currency", err);
    }
  },
}));
