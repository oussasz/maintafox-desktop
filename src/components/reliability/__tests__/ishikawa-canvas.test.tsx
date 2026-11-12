import { render as rtlRender, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { IshikawaDiagramCanvas } from "@/components/reliability/IshikawaDiagramCanvas";
import { P } from "@shared/rbac/permissions.generated";

// Stable like the real hook (the diagram reloads when `t` changes).
const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
let deniedRights: string[] = [];
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => !deniedRights.includes(p),
    canAny: () => true,
    isLoading: false,
  }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));

const svc = vi.hoisted(() => ({
  listRamIshikawaDiagrams: vi.fn(),
  upsertRamIshikawaDiagram: vi.fn(),
}));
vi.mock("@/services/reliability-service", () => svc);
vi.mock("@/services/asset-service", () => ({
  getAssetById: vi.fn(() => Promise.resolve({ id: 10, asset_code: "P-101", asset_name: "Pump" })),
}));
const render = (ui: ReactElement) => rtlRender(<MemoryRouter>{ui}</MemoryRouter>);
vi.mock("@/services/wo-service", () => ({
  createWo: vi.fn(),
  listWorkOrderTypes: vi.fn(() => Promise.resolve([])),
}));

beforeEach(() => {
  vi.clearAllMocks();
  deniedRights = [];
});

describe("IshikawaDiagramCanvas", () => {
  it("shows a failed load with a retry and offers nothing to save over the stored diagram", async () => {
    svc.listRamIshikawaDiagrams.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    render(<IshikawaDiagramCanvas equipmentId={10} />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("governance.loadError")).toBeInTheDocument();
    expect(screen.queryByText("governance.save")).not.toBeInTheDocument();
    expect(svc.upsertRamIshikawaDiagram).not.toHaveBeenCalled();
  });

  it("keeps an unreadable stored diagram as it is instead of replacing it", async () => {
    svc.listRamIshikawaDiagrams.mockResolvedValueOnce([
      { id: 4, equipment_id: 10, row_version: 2, title: "x", flow_json: "{not json" },
    ]);
    render(<IshikawaDiagramCanvas equipmentId={10} />);
    expect(await screen.findByText("governance.unreadable")).toBeInTheDocument();
    expect(screen.queryByText("governance.save")).not.toBeInTheDocument();
    expect(svc.upsertRamIshikawaDiagram).not.toHaveBeenCalled();
  });

  it("disables adding a cause and saving, with the right they need, for someone without ram.manage (L2-24)", async () => {
    svc.listRamIshikawaDiagrams.mockResolvedValueOnce([]);
    deniedRights = [P.RAM_MANAGE];
    render(<IshikawaDiagramCanvas equipmentId={10} />);
    const save = await screen.findByRole("button", { name: "governance.save" });
    expect(save).toBeDisabled();
    expect(save).toHaveAttribute("title", expect.stringMatching(/^rams\.rights\.required/));
    const add = screen.getAllByRole("button", { name: /governance\.addCause/ });
    expect(add.length).toBeGreaterThan(0);
    for (const b of add) expect(b).toBeDisabled();
    expect(screen.getByRole("note")).toHaveTextContent("rams.rights.readOnly");
  });

  it("offers a visible promote button that says what to do first (L2-19)", async () => {
    svc.listRamIshikawaDiagrams.mockResolvedValueOnce([]);
    render(<IshikawaDiagramCanvas equipmentId={10} />);
    const promote = await screen.findByRole("button", { name: "governance.promoteMenu" });
    expect(promote).toBeDisabled();
    expect(screen.getByText("governance.promoteSelectFirst")).toBeInTheDocument();
    expect(promote).toHaveAttribute("aria-describedby", "ishikawa-promote-reason");
  });

  it("leaves the buttons enabled with ram.manage", async () => {
    svc.listRamIshikawaDiagrams.mockResolvedValueOnce([]);
    render(<IshikawaDiagramCanvas equipmentId={10} />);
    expect(await screen.findByRole("button", { name: "governance.save" })).toBeEnabled();
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
  });
});
