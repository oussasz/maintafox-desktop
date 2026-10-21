import { render, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, describe, expect, it } from "vitest";

import { NegativeStockBadge, isNegativeStock } from "@/components/inventory/NegativeStockBadge";
import enInventory from "@/i18n/locale-data/en/inventory.json";

describe("NegativeStockBadge", () => {
  beforeAll(async () => {
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory } },
        interpolation: { escapeValue: false },
      });
    }
  });

  it("flags a negative on-hand or a negative available quantity, never a zero or positive one", () => {
    expect(isNegativeStock(-1, -1)).toBe(true);
    expect(isNegativeStock(3, -0.5)).toBe(true);
    expect(isNegativeStock(0, 0)).toBe(false);
    expect(isNegativeStock(2, 1)).toBe(false);
    // Float noise is not a negative balance.
    expect(isNegativeStock(0.1, -1e-12)).toBe(false);
  });

  it("shows a text label with the tooltip, and nothing for a sound balance", () => {
    const { rerender } = render(<NegativeStockBadge onHand={-2} available={-2} />);
    expect(screen.getByText("Negative stock")).toBeInTheDocument();
    expect(
      screen.getByTitle(
        "To be corrected: another PC may have recorded a movement. Record a count in “Stock to reconcile” (Inventory › Controls).",
      ),
    ).toBeTruthy();
    rerender(<NegativeStockBadge onHand={2} available={1} />);
    expect(screen.queryByText("Negative stock")).toBeNull();
  });
});
