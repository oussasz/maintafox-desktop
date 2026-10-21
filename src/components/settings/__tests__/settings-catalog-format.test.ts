import i18next from "i18next";
import { describe, expect, it } from "vitest";

import { PLANT_SCOPE, scopeChoiceToInput } from "@/components/admin/ScopeNodeSelect";
import { formatSettingValue } from "@/components/settings/SettingsCatalogSections";
import enSettings from "@/i18n/locale-data/en/settings.json";
import frSettings from "@/i18n/locale-data/fr/settings.json";

const t = (key: string, options?: Record<string, unknown>) =>
  options && "count" in options ? `${key}:${String(options["count"])}` : key;

describe("formatSettingValue", () => {
  it("names languages and months instead of showing codes", () => {
    expect(formatSettingValue({ key: "locale.primary_language", kind: "choice" }, '"fr"', t)).toBe(
      "options.language.fr",
    );
    expect(
      formatSettingValue({ key: "locale.fiscal_year_start_month", kind: "month" }, "7", t),
    ).toBe("options.month.7");
  });

  it("adds the unit to whole numbers and keeps currency codes", () => {
    expect(formatSettingValue({ key: "wo.reopen_window_days", kind: "integer" }, "30", t)).toBe(
      "catalog.units.wo.reopen_window_days:30",
    );
    expect(formatSettingValue({ key: "locale.currency_code", kind: "currency" }, '"DZD"', t)).toBe(
      "DZD",
    );
  });

  it("shows decimal tolerances as numbers of the interface language, with their unit", async () => {
    const i18n = i18next.createInstance();
    await i18n.init({
      lng: "fr",
      fallbackLng: "en",
      ns: ["settings"],
      defaultNS: "settings",
      resources: { fr: { settings: frSettings }, en: { settings: enSettings } },
      interpolation: { escapeValue: false },
      keySeparator: ".",
    });
    const def = { key: "inventory.reconciliation_drift_break", kind: "decimal" as const };
    const fr = (key: string, opts?: Record<string, unknown>): string =>
      String(i18n.t(key as never, (opts ?? {}) as never));
    expect(formatSettingValue(def, "0.01", fr)).toBe("0,01 (unité de l'article)");
    await i18n.changeLanguage("en");
    expect(formatSettingValue(def, "2.5", fr)).toBe("2.5 (article's unit)");
  });

  it("shows a dash for a missing value (history before values were kept)", () => {
    expect(formatSettingValue({ key: "wo.reopen_window_days", kind: "integer" }, null, t)).toBe(
      "—",
    );
  });
});

describe("scopeChoiceToInput", () => {
  it("maps the plant choice to a tenant assignment without reference", () => {
    expect(scopeChoiceToInput(PLANT_SCOPE)).toEqual({
      scope_type: "tenant",
      scope_reference: null,
    });
  });

  it("maps a node id to an org-node assignment (node and below)", () => {
    expect(scopeChoiceToInput("42")).toEqual({ scope_type: "org_node", scope_reference: "42" });
  });
});
