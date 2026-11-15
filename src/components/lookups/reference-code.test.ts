import { describe, expect, it } from "vitest";

import { codeFromLabel } from "./reference-code";

describe("codeFromLabel", () => {
  it("turns a French label into a valid code", () => {
    expect(codeFromLabel("Pompe centrifuge")).toBe("POMPE_CENTRIFUGE");
    expect(codeFromLabel("Échangeur de chaleur")).toBe("ECHANGEUR_DE_CHALEUR");
    expect(codeFromLabel("Cœur d'œuvre")).toBe("COEUR_D_OEUVRE");
  });

  it("starts with a letter and drops trailing separators", () => {
    expect(codeFromLabel("  3 voies - vanne ! ")).toBe("VOIES_VANNE");
    expect(codeFromLabel("123")).toBe("");
  });

  it("keeps at most 64 characters", () => {
    expect(codeFromLabel("a ".repeat(80)).length).toBeLessThanOrEqual(64);
    expect(codeFromLabel("a ".repeat(80)).endsWith("_")).toBe(false);
  });
});
