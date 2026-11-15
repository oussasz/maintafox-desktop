import { describe, expect, it } from "vitest";

import { summarizeSupplierPrices } from "../supplier-price-history";

const row = (
  purchase_order_id: number,
  supplier_id: number | null,
  unit_price: number | null,
  ordered_at: string | null,
) => ({
  purchase_order_id,
  supplier_id,
  supplier_name: supplier_id ? `S${supplier_id}` : null,
  unit_price,
  ordered_at,
});

describe("summarizeSupplierPrices", () => {
  it("gives last, previous, change, min and max per supplier, most recent first", () => {
    const out = summarizeSupplierPrices([
      row(1, 1, 100, "2026-01-10T00:00:00Z"),
      row(2, 1, 110, "2026-05-10T00:00:00Z"),
      row(3, 1, 90, "2026-03-10T00:00:00Z"),
      row(4, 2, 50, "2026-06-01T00:00:00Z"),
    ]);
    expect(out.map((s) => s.supplierId)).toEqual([2, 1]);
    const [s2, s1] = out;
    if (!s1 || !s2) throw new Error("two summaries expected");
    expect(s1.lastPrice).toBe(110);
    expect(s1.previousPrice).toBe(90);
    expect(s1.changePct).toBeCloseTo(22.22, 1);
    expect(s1.minPrice).toBe(90);
    expect(s1.maxPrice).toBe(110);
    expect(s1.purchases).toBe(3);
    expect(s2.changePct).toBeNull();
  });

  it("ignores lines without a price", () => {
    expect(summarizeSupplierPrices([row(1, 1, null, "2026-01-01T00:00:00Z")])).toEqual([]);
  });
});
