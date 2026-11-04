/**
 * L6-23: Ctrl K finds records (work orders, requests, equipment) by number or name, and only
 * searches the kinds the user may view.
 */
import { fireEvent, render, screen } from "@testing-library/react";
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
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", () => ({ invoke: ipc.invoke }));

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
    ipc.invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_wo") {
        return { items: [{ id: 7, code: "OT-2026-0007", title: "Fuite pompe" }] };
      }
      if (cmd === "list_di") return { items: [] };
      return [{ id: 3, asset_code: "POMPE-01", asset_name: "Pompe de relevage" }];
    });
  });

  it("finds a work order and an equipment by what is typed, and only asks for the kinds allowed", async () => {
    renderPalette();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "pompe" } });
    expect(await screen.findByText("OT-2026-0007 · Fuite pompe")).toBeInTheDocument();
    expect(await screen.findByText("POMPE-01 · Pompe de relevage")).toBeInTheDocument();
    const asked = ipc.invoke.mock.calls.map((c) => c[0]);
    expect(asked).toContain("list_wo");
    expect(asked).toContain("list_assets");
    // no request right: requests are not searched; and a search never raises the error toast
    expect(asked).not.toContain("list_di");
    expect(ipc.invoke.mock.calls.every((c) => c[2]?.notifyOnError === false)).toBe(true);
  });

  it("does not search records for a single typed character", async () => {
    renderPalette();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "p" } });
    await new Promise((r) => setTimeout(r, 400));
    expect(ipc.invoke).not.toHaveBeenCalled();
  });
});
