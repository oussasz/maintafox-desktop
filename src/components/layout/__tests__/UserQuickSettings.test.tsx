import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UserQuickSettings } from "@/components/layout/UserQuickSettings";
import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { useLocaleStore } from "@/stores/locale-store";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

describe("UserQuickSettings", () => {
  const setLocale = vi.fn().mockResolvedValue(undefined);

  beforeEach(() => {
    setLocale.mockClear();
    useLocaleStore.setState({
      activeLocale: "fr",
      supportedLocales: ["fr", "en"],
      followsSystemLanguage: false,
      setLocale,
    });
  });

  it("switches the language in one click and marks the active one", () => {
    render(<UserQuickSettings />);
    expect(screen.getByText("FR").getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByText("EN"));
    expect(setLocale).toHaveBeenCalledWith("en");
  });

  it("offers the colour mode when a theme provider is present, and not otherwise", () => {
    const { unmount } = render(<UserQuickSettings />);
    expect(screen.queryByLabelText("user.themeDark")).toBeNull();
    unmount();
    render(
      <ThemeProvider>
        <UserQuickSettings />
      </ThemeProvider>,
    );
    fireEvent.click(screen.getByLabelText("user.themeDark"));
    expect(screen.getByLabelText("user.themeDark").getAttribute("aria-pressed")).toBe("true");
  });
});
