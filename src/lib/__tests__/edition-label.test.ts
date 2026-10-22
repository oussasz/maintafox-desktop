import { beforeAll, describe, expect, it } from "vitest";

import { i18n, initI18n } from "@/i18n";

import { editionLabel } from "../edition-label";

beforeAll(async () => {
  initI18n();
  await i18n.changeLanguage("fr");
});

describe("editionLabel", () => {
  it("translates the system editions", () => {
    expect(editionLabel("development")).toBe("Démo / Développement");
    expect(editionLabel(" Core ")).toBe("Essentiel");
    expect(editionLabel("enterprise")).toBe("Entreprise");
  });

  it("shows a custom edition as issued and nothing for an empty one", () => {
    expect(editionLabel("sur-mesure")).toBe("sur-mesure");
    expect(editionLabel(null)).toBeNull();
    expect(editionLabel("  ")).toBeNull();
  });
});
