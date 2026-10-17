import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { IshikawaDiagramCanvas } from "@/components/reliability/IshikawaDiagramCanvas";

// Stable like the real hook (the diagram reloads when `t` changes).
const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));

const svc = vi.hoisted(() => ({
  listRamIshikawaDiagrams: vi.fn(),
  upsertRamIshikawaDiagram: vi.fn(),
}));
vi.mock("@/services/reliability-service", () => svc);
vi.mock("@/services/wo-service", () => ({
  createWo: vi.fn(),
  listWorkOrderTypes: vi.fn(() => Promise.resolve([])),
}));

beforeEach(() => {
  vi.clearAllMocks();
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
});
