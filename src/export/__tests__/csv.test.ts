import { describe, expect, it } from "vitest";

import { buildCsv } from "@/export/csv";

describe("buildCsv", () => {
  it("separates with semicolons and quotes cells that need it", () => {
    const csv = buildCsv(
      ["code", "nom"],
      [
        ["P-1", 'Pompe "A"; neuve'],
        ["P-2", null],
      ],
    );
    expect(csv).toBe('code;nom\r\nP-1;"Pompe ""A""; neuve"\r\nP-2;');
  });

  it("defuses formulas typed into text cells", () => {
    expect(buildCsv(["x"], [["=SUM(A1)"]])).toBe("x\r\n'=SUM(A1)");
  });
});
