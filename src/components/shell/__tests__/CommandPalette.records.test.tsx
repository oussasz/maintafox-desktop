/**
 * L6-23: Ctrl K finds records (work orders, requests, equipment) by number or name, and only
 * searches the kinds the user may view.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { CommandPalette } from "@/components/shell/CommandPalette";

vi.mock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: vi.fn() }));
const rights = vi.hoisted(() => ({ held: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => rights.held.has(p),
    canAny: () => true,
    canAll: () => true,
    permissions: [],
    isLoading: false,
    refresh: async () => undefined,
  }),
}));
vi.mock("@/hooks/use-module-capabilities", () => ({
  useModuleCapabilities: () => ({ capabilityMap: {}, loading: false }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
const svc = vi.hoisted(() => ({
  listWos: vi.fn(),
  listDis: vi.fn(),
  listAssets: vi.fn(),
}));
vi.mock("@/services/wo-service", () => ({ listWos: svc.listWos }));
vi.mock("@/services/di-service", () => ({ listDis: svc.listDis }));
vi.mock("@/services/asset-service", () => ({ listAssets: svc.listAssets }));

function renderPalette() {
  return render(
    <MemoryRouter>
      <CommandPalette open onOpenChange={() => undefined} />
    </MemoryRouter>,
  );
}

describe("CommandPalette records (L6-23)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    rights.held = new Set(["ot.view", "eq.view"]);
    svc.listWos.mockResolvedValue({
      items: [{ id: 7, code: "OT-2026-0007", title: "Fuite pompe" }],
    });
    svc.listDis.mockResolvedValue({ items: [] });
    svc.listAssets.mockResolvedValue([
      { id: 3, asset_code: "POMPE-01", asset_name: "Pompe de relevage" },
    ]);
  });

  it("finds a work order and an equipment by what is typed, and only asks for the kinds allowed", async () => {
    renderPalette();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "pompe" } });
    expect(await screen.findByText("OT-2026-0007 · Fuite pompe")).toBeInTheDocument();
    expect(await screen.findByText("POMPE-01 · Pompe de relevage")).toBeInTheDocument();
    expect(svc.listWos).toHaveBeenCalledWith(expect.objectContaining({ search: "pompe" }));
    // no request right: requests are not searched
    await waitFor(() => expect(svc.listAssets).toHaveBeenCalled());
    expect(svc.listDis).not.toHaveBeenCalled();
  });

  it("does not search records for a single typed character", async () => {
    renderPalette();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "p" } });
    await new Promise((r) => setTimeout(r, 400));
    expect(svc.listWos).not.toHaveBeenCalled();
  });
});
