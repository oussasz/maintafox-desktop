/**
 * L4-01: Ctrl K offers the same modules as the menu. A module outside the licence (here training)
 * used to be found by the palette only, then refused every list with "Permission refusée".
 */
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { CommandPalette } from "@/components/shell/CommandPalette";

vi.mock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: vi.fn() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: () => true,
    canAny: () => true,
    canAll: () => true,
    permissions: [],
    isLoading: false,
    refresh: async () => undefined,
  }),
}));
const capabilities = vi.hoisted(() => ({ map: {} as Record<string, boolean> }));
vi.mock("@/hooks/use-module-capabilities", () => ({
  useModuleCapabilities: () => ({ capabilityMap: capabilities.map, loading: false }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

function renderPalette() {
  return render(
    <MemoryRouter>
      <CommandPalette open onOpenChange={() => undefined} />
    </MemoryRouter>,
  );
}

describe("CommandPalette and the licence (L4-01)", () => {
  it("does not offer a module the licence leaves out", () => {
    capabilities.map = { permits: true, personnel: true, training: false };
    renderPalette();
    expect(screen.queryByText("nav.training")).toBeNull();
    expect(screen.getByText("nav.permits")).toBeInTheDocument();
  });

  it("offers it once the licence includes it", () => {
    capabilities.map = { permits: true, personnel: true, training: true };
    renderPalette();
    expect(screen.getByText("nav.training")).toBeInTheDocument();
  });
});
