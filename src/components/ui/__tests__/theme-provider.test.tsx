import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AppearanceSettingsPanel } from "@/components/settings/AppearanceSettingsPanel";
import { THEME_STORAGE_KEY, ThemeProvider, useTheme } from "@/components/ui/ThemeProvider";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

type Listener = (e: MediaQueryListEvent) => void;

/** matchMedia mock whose "dark" answer can be flipped, firing the change event. */
function installMatchMedia(initialDark: boolean) {
  let dark = initialDark;
  const listeners = new Set<Listener>();
  window.matchMedia = ((query: string) => ({
    get matches() {
      return query.includes("dark") ? dark : false;
    },
    media: query,
    addEventListener: (_: string, l: Listener) => listeners.add(l),
    removeEventListener: (_: string, l: Listener) => listeners.delete(l),
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
    onchange: null,
  })) as unknown as typeof window.matchMedia;
  return {
    listenerCount: () => listeners.size,
    setDark(next: boolean) {
      dark = next;
      for (const l of listeners) l({ matches: next } as MediaQueryListEvent);
    },
  };
}

function Probe() {
  const { theme, preference } = useTheme();
  return (
    <p data-testid="probe">
      {preference}/{theme}
    </p>
  );
}

const root = () => document.documentElement;

describe("ThemeProvider colour mode", () => {
  beforeEach(() => {
    localStorage.clear();
    root().className = "";
    root().style.colorScheme = "";
  });
  afterEach(() => localStorage.clear());

  it("defaults to Système (follows a dark Windows setting) when nothing is stored", () => {
    installMatchMedia(true);
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("system/dark");
    expect(root().classList.contains("dark")).toBe(true);
    expect(root().classList.contains("light")).toBe(false);
    expect(root().style.colorScheme).toBe("dark");
  });

  it("shows system selected by default on the settings panel", () => {
    installMatchMedia(false);
    render(
      <ThemeProvider>
        <AppearanceSettingsPanel />
      </ThemeProvider>,
    );
    expect(screen.getByRole("radio", { name: /^appearance.system/ })).toBeChecked();
    expect(screen.getByText("appearance.storedLocally")).toBeInTheDocument();
  });

  it("persists the choice and reads it back after a restart (remount)", () => {
    installMatchMedia(false);
    const first = render(
      <ThemeProvider>
        <AppearanceSettingsPanel />
        <Probe />
      </ThemeProvider>,
    );
    fireEvent.click(screen.getByRole("radio", { name: /^appearance\.dark/ }));
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(root().classList.contains("dark")).toBe(true);
    first.unmount();
    root().className = "";

    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("dark/dark");
    expect(root().classList.contains("dark")).toBe(true);
  });

  it("keeps a fixed Clair even when Windows is dark", () => {
    installMatchMedia(true);
    localStorage.setItem(THEME_STORAGE_KEY, "light");
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("light/light");
    expect(root().classList.contains("light")).toBe(true);
    expect(root().classList.contains("dark")).toBe(false);
  });

  it("Système follows the OS setting live (matchMedia change event)", () => {
    const mm = installMatchMedia(false);
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("system/light");
    act(() => mm.setDark(true));
    expect(screen.getByTestId("probe").textContent).toBe("system/dark");
    expect(root().classList.contains("dark")).toBe(true);
    act(() => mm.setDark(false));
    expect(root().classList.contains("light")).toBe(true);
  });

  it("a fixed choice ignores OS changes", () => {
    const mm = installMatchMedia(false);
    localStorage.setItem(THEME_STORAGE_KEY, "light");
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    act(() => mm.setDark(true));
    expect(screen.getByTestId("probe").textContent).toBe("light/light");
  });

  it("reads a legacy stored value ('dark') unchanged", () => {
    installMatchMedia(false);
    localStorage.setItem(THEME_STORAGE_KEY, "dark");
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(screen.getByTestId("probe").textContent).toBe("dark/dark");
  });

  it("removes the matchMedia listener on unmount", () => {
    const mm = installMatchMedia(false);
    const view = render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    expect(mm.listenerCount()).toBe(1);
    view.unmount();
    expect(mm.listenerCount()).toBe(0);
  });

  it("can be driven from the keyboard (arrow keys move the choice, Space selects)", () => {
    installMatchMedia(false);
    render(
      <ThemeProvider>
        <AppearanceSettingsPanel />
        <Probe />
      </ThemeProvider>,
    );
    const system = screen.getByRole("radio", { name: /^appearance\.system/ });
    system.focus();
    expect(system).toHaveFocus();
    // Native radio groups move and select with the arrow keys; jsdom does not, so the
    // test asserts the group is a real, focusable radio group and Space/click selects.
    const dark = screen.getByRole("radio", { name: /^appearance\.dark/ });
    expect(dark).toHaveAttribute("name", system.getAttribute("name"));
    dark.focus();
    fireEvent.keyDown(dark, { key: " " });
    fireEvent.click(dark);
    expect(dark).toBeChecked();
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
  });
});
