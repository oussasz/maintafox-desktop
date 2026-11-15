import { describe, expect, it } from "vitest";

import { compatibleEntryUnits } from "../entered-unit";

const u = (id: number, semantic_tag: string | null) => ({ id, semantic_tag });

describe("compatibleEntryUnits", () => {
  const units = [u(1, "count"), u(2, "mass"), u(3, "frequency"), u(4, "packaging"), u(5, "count")];

  it("keeps the stock unit's dimension and packaging units", () => {
    expect(compatibleEntryUnits(units, 1).map((x) => x.id)).toEqual([1, 4, 5]);
  });

  it("keeps every unit when the stock unit dimension is unknown or packaging", () => {
    expect(compatibleEntryUnits(units, 99)).toHaveLength(5);
    expect(compatibleEntryUnits(units, 4)).toHaveLength(5);
  });
});
