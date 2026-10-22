import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { AuthLocaleSwitch } from "@/components/auth/shell/AuthPrimitives";
import { i18n, initI18n } from "@/i18n";
import { useLocaleStore } from "@/stores/locale-store";
import { mockInvoke } from "@/test/mocks/tauri";

beforeAll(async () => {
  initI18n();
  await i18n.changeLanguage("fr");
});

beforeEach(async () => {
  await i18n.changeLanguage("fr");
  vi.clearAllMocks();
  useLocaleStore.setState({
    activeLocale: "fr",
    followsSystemLanguage: false,
    supportedLocales: ["fr", "en"],
  });
});

const pref = (active: string, user: string) => ({
  active_locale: active,
  user_locale: user,
  tenant_locale: null,
  os_locale: "en",
  supported_locales: ["fr", "en"],
});

describe("AuthLocaleSwitch", () => {
  it("saves the picked language for this PC through the backend", async () => {
    mockInvoke.mockResolvedValueOnce(pref("en", "en"));
    render(<AuthLocaleSwitch />);
    fireEvent.click(screen.getByRole("button", { name: "EN" }));
    await waitFor(() => expect(useLocaleStore.getState().activeLocale).toBe("en"));
    expect(mockInvoke).toHaveBeenCalledWith("set_locale_preference", {
      payload: { locale: "en", asTenantDefault: false },
    });
  });

  it("offers 'follow the Windows language' as an explicit choice", async () => {
    mockInvoke.mockResolvedValueOnce(pref("en", "system"));
    render(<AuthLocaleSwitch />);
    expect(screen.getByRole("button", { name: "FR" })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "Suivre la langue de Windows" }));
    await waitFor(() => expect(useLocaleStore.getState().followsSystemLanguage).toBe(true));
    expect(screen.getByRole("button", { name: "FR" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "Suivre la langue de Windows" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
