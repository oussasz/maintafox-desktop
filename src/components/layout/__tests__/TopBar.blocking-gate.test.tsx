/**
 * Ctrl+K opens the command palette from anywhere, except while a blocking screen is shown (the
 * forced lock removal acknowledgement): the real TopBar, the real blocking-gate flag and the real
 * palette (inside the real permission provider); only the translations are stubbed, and the
 * Tauri bridge answers nothing (`invoke` as in every test, `src/test/setup.ts`; event listeners
 * never fire).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PermissionProvider } from "@/contexts/PermissionContext";
import { setBlockingGateActive } from "@/lib/blocking-gate";

import { TopBar } from "../TopBar";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en", changeLanguage: vi.fn() },
  }),
}));

function pressCtrlK() {
  fireEvent.keyDown(document, { key: "k", ctrlKey: true });
}

describe("TopBar Ctrl+K under a blocking screen", () => {
  afterEach(() => {
    setBlockingGateActive(false);
  });

  it("opens nothing while the blocking gate is active, and the palette once it is gone", async () => {
    render(
      <MemoryRouter>
        <PermissionProvider>
          <TopBar />
        </PermissionProvider>
      </MemoryRouter>,
    );

    setBlockingGateActive(true);
    pressCtrlK();
    // Give React the chance to render a palette, if the shortcut went through.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    setBlockingGateActive(false);
    pressCtrlK();
    await waitFor(() => expect(screen.getByRole("dialog")).toBeInTheDocument());
  });

  it("has no sync centre button for a user without sync.view (AU-45)", async () => {
    render(
      <MemoryRouter>
        <PermissionProvider>
          <TopBar />
        </PermissionProvider>
      </MemoryRouter>,
    );
    // The bridge answers no permission: the button that opens the conflict list is not offered.
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(screen.queryByRole("button", { name: "sync.syncCenter" })).not.toBeInTheDocument();
  });
});
