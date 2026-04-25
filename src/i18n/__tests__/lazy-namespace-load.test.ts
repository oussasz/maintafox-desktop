import { beforeAll, describe, expect, it } from "vitest";

import { initI18n, i18n } from "@/i18n";

describe("i18n lazy namespace loading", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
  });

  it("loads the org namespace and resolves designer keys", async () => {
    await i18n.loadNamespaces("org");

    expect(i18n.hasResourceBundle("fr", "org")).toBe(true);

    const value = i18n.t("designer.title", { ns: "org" });
    expect(value).toBe("Concepteur d'organisation");
  });
});
