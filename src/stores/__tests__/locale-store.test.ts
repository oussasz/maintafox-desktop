/**
 * locale-store — behaviour tests.
 *
 * Boundaries mocked: the Tauri core `invoke` (global mock, via mockInvoke) for
 * the locale preference commands, and settings-service `getSetting` for the
 * tenant currency / fallback-language settings. The real i18next instance is used.
 */

import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { getSetting } from "@/services/settings-service";
import { mockInvoke } from "@/test/mocks/tauri";
import type { AppSetting, LocalePreference } from "@shared/ipc-types";

import { useLocaleStore } from "../locale-store";

vi.mock("@/services/settings-service", () => ({
  getSetting: vi.fn(),
}));

const mockGetSetting = vi.mocked(getSetting);

const CURRENCY_KEY = "locale.currency_code";
const FALLBACK_KEY = "locale.fallback_language";

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makePref(overrides: Partial<LocalePreference> = {}): LocalePreference {
  return {
    active_locale: "en",
    user_locale: "en",
    tenant_locale: "fr",
    os_locale: "en-US",
    supported_locales: ["fr", "en"],
    ...overrides,
  };
}

function makeSetting(
  key: string,
  valueJson: string,
  overrides: Partial<AppSetting> = {},
): AppSetting {
  return {
    id: 1,
    setting_key: key,
    setting_scope: "tenant",
    setting_value_json: valueJson,
    category: "locale",
    setting_risk: "low",
    validation_status: "valid",
    secret_ref_id: null,
    last_modified_by_id: null,
    last_modified_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

/** Route getSetting by key: undefined entries resolve to null. */
function settingsByKey(values: Partial<Record<string, string | Error>>) {
  mockGetSetting.mockImplementation(async (key: string) => {
    const v = values[key];
    if (v instanceof Error) throw v;
    return v === undefined ? null : makeSetting(key, v);
  });
}

function deferred<T>() {
  let resolve: (v: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const initialState = useLocaleStore.getState();
let originalFallback: Exclude<typeof i18n.options.fallbackLng, undefined> = "en";

function resetStore() {
  useLocaleStore.setState(initialState, true);
}

beforeAll(async () => {
  initI18n();
  const fb = i18n.options.fallbackLng;
  if (fb !== undefined) originalFallback = fb;
  await i18n.changeLanguage("fr");
});

afterAll(async () => {
  i18n.options.fallbackLng = originalFallback;
  await i18n.changeLanguage("fr");
});

beforeEach(async () => {
  vi.clearAllMocks();
  mockInvoke.mockReset();
  mockGetSetting.mockReset();
  mockGetSetting.mockResolvedValue(null);
  resetStore();
  i18n.options.fallbackLng = "en";
  await i18n.changeLanguage("fr");
  vi.spyOn(console, "warn").mockImplementation(() => undefined);
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe("locale-store", () => {
  it("starts with French, DZD, ltr and loading=true", () => {
    const s = useLocaleStore.getState();
    expect(s.activeLocale).toBe("fr");
    expect(s.activeCurrencyCode).toBe("DZD");
    expect(s.direction).toBe("ltr");
    expect(s.isLoading).toBe(true);
    expect(s.error).toBeNull();
    expect(s.supportedLocales).toEqual(["fr", "en"]);
  });

  describe("initialize", () => {
    it("reads the preference, switches i18n and loads tenant currency", async () => {
      mockInvoke.mockResolvedValueOnce(makePref({ supported_locales: ["fr", "en", "ar"] }));
      settingsByKey({ [CURRENCY_KEY]: '"eur"' });

      await useLocaleStore.getState().initialize();

      const s = useLocaleStore.getState();
      expect(mockInvoke).toHaveBeenCalledWith("get_locale_preference", undefined);
      expect(mockGetSetting).toHaveBeenCalledWith(CURRENCY_KEY, "tenant");
      expect(mockGetSetting).toHaveBeenCalledWith(FALLBACK_KEY, "tenant");
      expect(i18n.language).toBe("en");
      expect(s.activeLocale).toBe("en");
      expect(s.activeCurrencyCode).toBe("EUR");
      expect(s.direction).toBe("ltr");
      expect(s.supportedLocales).toEqual(["fr", "en", "ar"]);
      expect(s.isLoading).toBe(false);
      expect(s.error).toBeNull();
    });

    it("keeps isLoading=true and clears error while the preference is in flight", async () => {
      const d = deferred<LocalePreference>();
      mockInvoke.mockReturnValueOnce(d.promise);
      useLocaleStore.setState({ isLoading: false, error: "old" });

      const pending = useLocaleStore.getState().initialize();
      expect(useLocaleStore.getState().isLoading).toBe(true);
      expect(useLocaleStore.getState().error).toBeNull();

      d.resolve(makePref({ active_locale: "fr" }));
      await pending;
      expect(useLocaleStore.getState().isLoading).toBe(false);
    });

    it("sets rtl direction for an RTL locale", async () => {
      mockInvoke.mockResolvedValueOnce(makePref({ active_locale: "ar" }));

      await useLocaleStore.getState().initialize();

      expect(useLocaleStore.getState().activeLocale).toBe("ar");
      expect(useLocaleStore.getState().direction).toBe("rtl");
    });

    it("accepts a non-JSON raw currency value", async () => {
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [CURRENCY_KEY]: "usd" });

      await useLocaleStore.getState().initialize();

      expect(useLocaleStore.getState().activeCurrencyCode).toBe("USD");
    });

    it.each([
      ["missing setting", undefined],
      ["empty value", ""],
      ["invalid code", '"EURO"'],
      ["non-string JSON", "42"],
    ])("falls back to DZD for %s", async (_label, value) => {
      useLocaleStore.setState({ activeCurrencyCode: "EUR" });
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [CURRENCY_KEY]: value });

      await useLocaleStore.getState().initialize();

      expect(useLocaleStore.getState().activeCurrencyCode).toBe("DZD");
    });

    it("keeps DZD and still succeeds when the currency setting cannot be read", async () => {
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [CURRENCY_KEY]: new Error("not authenticated") });

      await useLocaleStore.getState().initialize();

      const s = useLocaleStore.getState();
      expect(s.activeCurrencyCode).toBe("DZD");
      expect(s.activeLocale).toBe("en");
      expect(s.error).toBeNull();
      expect(s.isLoading).toBe(false);
    });

    it("applies a JSON-string fallback language to i18next", async () => {
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [FALLBACK_KEY]: '"fr"' });

      await useLocaleStore.getState().initialize();

      expect(i18n.options.fallbackLng).toBe("fr");
    });

    it("applies a raw (non-JSON) fallback language value", async () => {
      i18n.options.fallbackLng = "fr";
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [FALLBACK_KEY]: "EN" });

      await useLocaleStore.getState().initialize();

      expect(i18n.options.fallbackLng).toBe("en");
    });

    it("maps an unsupported fallback language to en", async () => {
      i18n.options.fallbackLng = "fr";
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [FALLBACK_KEY]: '"de"' });

      await useLocaleStore.getState().initialize();

      expect(i18n.options.fallbackLng).toBe("en");
    });

    it("ignores a non-string JSON fallback value and a failing fallback lookup", async () => {
      i18n.options.fallbackLng = "fr";
      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [FALLBACK_KEY]: "123" });

      await useLocaleStore.getState().initialize();
      expect(i18n.options.fallbackLng).toBe("fr");

      mockInvoke.mockResolvedValueOnce(makePref());
      settingsByKey({ [FALLBACK_KEY]: new Error("not authenticated") });

      await useLocaleStore.getState().initialize();
      expect(i18n.options.fallbackLng).toBe("fr");
      expect(useLocaleStore.getState().error).toBeNull();
    });

    it("falls back to defaults and records the error when the preference IPC fails", async () => {
      useLocaleStore.setState({
        activeLocale: "en",
        activeCurrencyCode: "EUR",
        direction: "rtl",
        supportedLocales: ["fr", "en", "ar"],
      });
      mockInvoke.mockRejectedValueOnce(new Error("bridge unavailable"));

      await expect(useLocaleStore.getState().initialize()).resolves.toBeUndefined();

      const s = useLocaleStore.getState();
      expect(s.activeLocale).toBe("fr");
      expect(s.activeCurrencyCode).toBe("DZD");
      expect(s.direction).toBe("ltr");
      expect(s.isLoading).toBe(false);
      expect(s.error).toBe("bridge unavailable");
      // supportedLocales is not reset on failure.
      expect(s.supportedLocales).toEqual(["fr", "en", "ar"]);
      expect(mockGetSetting).not.toHaveBeenCalled();
      expect(console.warn).toHaveBeenCalled();
    });
  });

  describe("setLocale", () => {
    it("persists via IPC (user scope by default) and switches i18n", async () => {
      mockInvoke.mockResolvedValueOnce(makePref());

      await useLocaleStore.getState().setLocale("en");

      const s = useLocaleStore.getState();
      expect(mockInvoke).toHaveBeenCalledWith("set_locale_preference", {
        payload: { locale: "en", asTenantDefault: false },
      });
      expect(i18n.language).toBe("en");
      expect(s.activeLocale).toBe("en");
      expect(s.direction).toBe("ltr");
      expect(s.isLoading).toBe(false);
      expect(s.error).toBeNull();
    });

    it("follows the Windows language only when asked, and applies what the backend resolved", async () => {
      mockInvoke.mockResolvedValueOnce(makePref({ active_locale: "en", user_locale: "system" }));

      await useLocaleStore.getState().setLocale("system");

      expect(mockInvoke).toHaveBeenCalledWith("set_locale_preference", {
        payload: { locale: "system", asTenantDefault: false },
      });
      expect(i18n.language).toBe("en");
      expect(useLocaleStore.getState().activeLocale).toBe("en");
      expect(useLocaleStore.getState().followsSystemLanguage).toBe(true);

      mockInvoke.mockResolvedValueOnce(makePref({ active_locale: "fr", user_locale: "fr" }));
      await useLocaleStore.getState().setLocale("fr");
      expect(useLocaleStore.getState().followsSystemLanguage).toBe(false);
    });

    it("passes asTenantDefault=true through", async () => {
      mockInvoke.mockResolvedValueOnce(makePref({ active_locale: "fr" }));

      await useLocaleStore.getState().setLocale("fr", true);

      expect(mockInvoke).toHaveBeenCalledWith("set_locale_preference", {
        payload: { locale: "fr", asTenantDefault: true },
      });
    });

    it("sets rtl for an RTL locale", async () => {
      mockInvoke.mockResolvedValueOnce(makePref({ active_locale: "ar" }));

      await useLocaleStore.getState().setLocale("ar");

      expect(useLocaleStore.getState().direction).toBe("rtl");
    });

    it("sets isLoading while in flight", async () => {
      const d = deferred<LocalePreference>();
      mockInvoke.mockReturnValueOnce(d.promise);
      useLocaleStore.setState({ isLoading: false, error: "old" });

      const pending = useLocaleStore.getState().setLocale("en");
      expect(useLocaleStore.getState().isLoading).toBe(true);
      expect(useLocaleStore.getState().error).toBeNull();

      d.resolve(makePref());
      await pending;
      expect(useLocaleStore.getState().isLoading).toBe(false);
    });

    it("keeps the current locale and records the error when persisting fails", async () => {
      useLocaleStore.setState({ activeLocale: "fr", isLoading: false });
      mockInvoke.mockRejectedValueOnce(new Error("write failed"));

      await expect(useLocaleStore.getState().setLocale("en")).resolves.toBeUndefined();

      const s = useLocaleStore.getState();
      expect(s.activeLocale).toBe("fr");
      expect(s.direction).toBe("ltr");
      expect(s.isLoading).toBe(false);
      expect(s.error).toBe("write failed");
      expect(i18n.language).toBe("fr");
      expect(console.error).toHaveBeenCalled();
    });
  });

  describe("currency", () => {
    it("setCurrencyCode normalises the code in memory", () => {
      useLocaleStore.getState().setCurrencyCode(" eur ");
      expect(useLocaleStore.getState().activeCurrencyCode).toBe("EUR");

      useLocaleStore.getState().setCurrencyCode("not-a-code");
      expect(useLocaleStore.getState().activeCurrencyCode).toBe("DZD");
      expect(mockGetSetting).not.toHaveBeenCalled();
    });

    it("refreshCurrency re-reads the tenant setting", async () => {
      settingsByKey({ [CURRENCY_KEY]: '"MAD"' });

      await useLocaleStore.getState().refreshCurrency();

      expect(mockGetSetting).toHaveBeenCalledWith(CURRENCY_KEY, "tenant");
      expect(useLocaleStore.getState().activeCurrencyCode).toBe("MAD");
    });

    it("refreshCurrency falls back to DZD when the setting is absent", async () => {
      useLocaleStore.setState({ activeCurrencyCode: "EUR" });
      settingsByKey({});

      await useLocaleStore.getState().refreshCurrency();

      expect(useLocaleStore.getState().activeCurrencyCode).toBe("DZD");
    });

    it("refreshCurrency swallows errors and keeps the current currency", async () => {
      useLocaleStore.setState({ activeCurrencyCode: "EUR" });
      settingsByKey({ [CURRENCY_KEY]: new Error("offline") });

      await expect(useLocaleStore.getState().refreshCurrency()).resolves.toBeUndefined();

      const s = useLocaleStore.getState();
      expect(s.activeCurrencyCode).toBe("EUR");
      expect(s.error).toBeNull();
      expect(console.warn).toHaveBeenCalled();
    });
  });
});
