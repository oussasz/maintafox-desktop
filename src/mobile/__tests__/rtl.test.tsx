import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { LanguageSwitch } from "../components/LanguageSwitch";
import { MOBILE_LANGUAGE_STORAGE_KEY, createMobileI18n } from "../i18n";

import { renderMobile } from "./helpers";

describe("mobile language and direction", () => {
  it("starts in French, left to right", () => {
    createMobileI18n();
    expect(document.documentElement.lang).toBe("fr");
    expect(document.documentElement.dir).toBe("ltr");
  });

  it("sets dir=rtl and lang=ar for Arabic, and back to ltr for French and English", async () => {
    const i18n = createMobileI18n("fr");
    await i18n.changeLanguage("ar");
    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");
    await i18n.changeLanguage("en");
    expect(document.documentElement.dir).toBe("ltr");
    expect(document.documentElement.lang).toBe("en");
    await i18n.changeLanguage("fr");
    expect(document.documentElement.dir).toBe("ltr");
    expect(document.documentElement.lang).toBe("fr");
  });

  it("switches language from the FR / AR / EN buttons and remembers the choice", async () => {
    renderMobile(<LanguageSwitch />);
    expect(screen.getByRole("button", { name: "Français" }).getAttribute("aria-pressed")).toBe(
      "true",
    );

    fireEvent.click(screen.getByRole("button", { name: "العربية" }));
    expect(document.documentElement.dir).toBe("rtl");
    expect(document.documentElement.lang).toBe("ar");
    expect(screen.getByRole("group", { name: "اللغة" })).toBeTruthy();
    expect(localStorage.getItem(MOBILE_LANGUAGE_STORAGE_KEY)).toBe("ar");

    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(document.documentElement.dir).toBe("ltr");
    expect(screen.getByRole("group", { name: "Language" })).toBeTruthy();
  });
});
