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
    expect(buildCsv(["x"], [["-2+3"]])).toBe("x\r\n'-2+3");
  });

  it("keeps formatted negative amounts readable but still defuses formulas", () => {
    expect(buildCsv(["x"], [["-1 234,50 DA"]])).toBe("x\r\n-1 234,50 DA");
    expect(buildCsv(["x"], [["-3,00 kg"]])).toBe("x\r\n-3,00 kg");
    expect(buildCsv(["x"], [["-2+3"]])).toBe("x\r\n'-2+3");
    expect(buildCsv(["x"], [["-1+cmd|x"]])).toBe("x\r\n'-1+cmd|x");
  });

  it("keeps negative numbers as numbers", () => {
    expect(buildCsv(["x"], [[-5]])).toBe("x\r\n-5");
  });
});
