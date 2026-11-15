/**
 * L2-22: the reliability profile of the asset record shows the failure count and MTBF of the RAMS
 * calculation (one source), with a visible link to RAMS; without the RAMS right it shows neither.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const svc = vi.hoisted(() => ({ getReliabilityAssetFigures: vi.fn() }));
vi.mock("@/services/reliability-study-service", () => svc);
vi.mock("@/components/assets/AssetBindingSummary", () => ({ AssetBindingSummary: () => null }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && Object.keys(opts).length > 0 ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "fr" },
  }),
}));
let denied: string[] = [];
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => !denied.includes(p) }),
}));
vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

const { AssetOverviewTab } = await import("@/components/assets/detail/AssetOverviewTab");
const { P } = await import("@shared/rbac/permissions.generated");

const asset = {
  id: 7,
  asset_code: "PMP-101",
  asset_name: "Pompe",
  rams_schedule_reference_value_name: null,
  rams_utilization_factor: 1,
} as unknown as Parameters<typeof AssetOverviewTab>[0]["asset"];

beforeEach(() => {
  svc.getReliabilityAssetFigures.mockReset();
  denied = [];
});

describe("AssetOverviewTab reliability profile", () => {
  it("shows the RAMS failure count and MTBF with a link to RAMS", async () => {
    svc.getReliabilityAssetFigures.mockResolvedValue({
      equipment_id: 7,
      period_start: "2025-11-01T00:00:00Z",
      period_end: "2026-11-01T00:00:00Z",
      window_too_short: false,
      failures: 7,
      downtime_hours: 30,
      mtbf_hours: 1250,
      mttr_hours: 4,
    });
    render(
      <MemoryRouter>
        <AssetOverviewTab asset={asset} onEditCommercial={() => {}} />
      </MemoryRouter>,
    );
    await waitFor(() => expect(screen.getByText(/1.?250\s*rams\.hoursUnit/)).toBeInTheDocument());
    expect(svc.getReliabilityAssetFigures).toHaveBeenCalledWith(
      7,
      expect.any(String),
      expect.any(String),
    );
    expect(screen.getByText("7")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "rams.open" })).toBeInTheDocument();
  });

  it("asks nothing and shows no figure without the RAMS right", () => {
    denied = [P.RAM_VIEW];
    render(
      <MemoryRouter>
        <AssetOverviewTab asset={asset} onEditCommercial={() => {}} />
      </MemoryRouter>,
    );
    expect(svc.getReliabilityAssetFigures).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "rams.open" })).not.toBeInTheDocument();
  });
});
