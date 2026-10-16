import { describe, expect, it } from "vitest";

import { PLANT_SCOPE, scopeChoiceToInput } from "@/components/admin/ScopeNodeSelect";
import { formatSettingValue } from "@/components/settings/SettingsCatalogSections";

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
