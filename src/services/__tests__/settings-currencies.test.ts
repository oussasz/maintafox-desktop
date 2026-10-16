/**
 * Currencies of Données de référence › Devises as the Settings picker reads them.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { currencyOptionLabel } from "@/components/settings/SettingsCatalogSections";
import { listReferenceCurrencies } from "@/services/settings-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

afterEach(() => {
  ipc.invoke.mockReset();
});

const dzd = {
  code: "DZD",
  label: "Dinar algérien",
  label_en: "Algerian dinar",
  symbol: "DA",
  minor_units: 2,
  is_active: true,
};

describe("reference currencies", () => {
  it("parses the published list", async () => {
    ipc.invoke.mockResolvedValueOnce([dzd]);
    await expect(listReferenceCurrencies()).resolves.toEqual([dzd]);
    expect(ipc.invoke).toHaveBeenCalledWith("list_reference_currencies");
  });

  it("refuses a malformed currency", async () => {
    ipc.invoke.mockResolvedValueOnce([{ ...dzd, minor_units: "2" }]);
    await expect(listReferenceCurrencies()).rejects.toThrow();
  });

  it("labels a currency in the interface language", () => {
    expect(currencyOptionLabel(dzd, "fr")).toBe("DZD — Dinar algérien (DA)");
    expect(currencyOptionLabel(dzd, "en")).toBe("DZD — Algerian dinar (DA)");
    expect(currencyOptionLabel({ ...dzd, label_en: null, symbol: null }, "en")).toBe(
      "DZD — Dinar algérien",
    );
  });
});
