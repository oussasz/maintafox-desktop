/**
 * Asset record tabs: criticality assessment (method preview, save writes the new class) and the
 * spare-parts list (stock state, critical-spare warning, removal through a confirmation).
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const svc = {
  getAssetCriticalityMethod: vi.fn(),
  listAssetCriticalityAssessments: vi.fn(),
  previewAssetCriticality: vi.fn(),
  assessAssetCriticality: vi.fn(),
  listAssetSpareParts: vi.fn(),
  removeAssetSparePart: vi.fn(),
  addAssetSparePart: vi.fn(),
  updateAssetSparePart: vi.fn(),
};

vi.mock("@/services/asset-registry-service", () => svc);
vi.mock("@/services/inventory-service", () => ({
  listInventoryArticles: vi.fn().mockResolvedValue([]),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && Object.keys(opts).length > 0 ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "fr" },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true }),
}));

const { AssetCriticalityTab } = await import("@/components/assets/detail/AssetCriticalityTab");
const { AssetSparePartsTab } = await import("@/components/assets/detail/AssetSparePartsTab");

const asset = {
  id: 7,
  entity_sync_id: "eq-7",
  asset_code: "PMP-101",
  asset_name: "Pompe d'alimentation",
  class_id: 1,
  class_code: "PUMP",
  class_name: "Pompe",
  family_code: null,
  family_name: null,
  subfamily_code: null,
  subfamily_name: null,
  criticality_value_id: 3,
  criticality_code: "C",
  status_code: "ACTIVE_IN_SERVICE",
  manufacturer: null,
  model: null,
  serial_number: null,
  maintainable_boundary: true,
  rams_schedule_reference_value_id: null,
  rams_schedule_reference_value_name: null,
  rams_utilization_factor: 1,
  org_node_id: 2,
  org_node_name: "Chaufferie",
  commissioned_at: null,
  decommissioned_at: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-02T00:00:00Z",
  deleted_at: null,
  row_version: 4,
};

const method = {
  code: "RISK_5X5_V1",
  score_min: 1,
  score_max: 5,
  class_a_min_risk: 15,
  class_b_min_risk: 8,
  class_c_min_risk: 4,
  hse_override_score: 5,
};

beforeEach(() => {
  for (const fn of Object.values(svc)) fn.mockReset();
});

describe("AssetCriticalityTab", () => {
  it("previews the class from the backend and saves the assessment with the row version", async () => {
    svc.getAssetCriticalityMethod.mockResolvedValue(method);
    svc.listAssetCriticalityAssessments.mockResolvedValue([]);
    svc.previewAssetCriticality.mockImplementation(async (scores: { production: number }) => ({
      method_code: "RISK_5X5_V1",
      consequence: scores.production,
      risk_score: scores.production,
      resulting_class: scores.production >= 5 ? "C" : "D",
      hse_override: false,
    }));
    const updated = { ...asset, criticality_code: "A", row_version: 5 };
    svc.assessAssetCriticality.mockResolvedValue({ assessment: {}, asset: updated });
    const onAssetChanged = vi.fn();

    render(<AssetCriticalityTab asset={asset} onAssetChanged={onAssetChanged} />);

    // Six score rows of 1–5: the method thresholds come from the backend.
    const production = await screen.findByRole("radiogroup", {
      name: "critMethod.categories.production",
    });
    expect(screen.getAllByRole("radiogroup")).toHaveLength(6);
    fireEvent.click(production.querySelectorAll("button")[4] as HTMLButtonElement);

    await waitFor(() =>
      expect(svc.previewAssetCriticality).toHaveBeenLastCalledWith(
        expect.objectContaining({ production: 5 }),
      ),
    );
    fireEvent.change(screen.getByLabelText("critMethod.rationale"), {
      target: { value: "Seule pompe" },
    });
    fireEvent.click(screen.getByRole("button", { name: "critMethod.save" }));

    await waitFor(() =>
      expect(svc.assessAssetCriticality).toHaveBeenCalledWith(
        7,
        expect.objectContaining({ production: 5, likelihood: 1 }),
        "Seule pompe",
        4,
      ),
    );
    await waitFor(() => expect(onAssetChanged).toHaveBeenCalledWith(updated));
  });
});

const line = {
  id: 11,
  asset_id: 7,
  article_id: 31,
  article_code: "BRG-6205",
  article_name: "Roulement 6205",
  unit_label: "U",
  article_is_active: true,
  quantity: 2,
  position_label: "Palier côté accouplement",
  is_critical_spare: true,
  notes: null,
  on_hand_qty: 0,
  available_qty: 0,
  min_stock: 2,
  row_version: 3,
};

describe("AssetSparePartsTab", () => {
  it("flags a critical spare out of stock and removes a line after confirmation", async () => {
    svc.listAssetSpareParts.mockResolvedValueOnce([line]).mockResolvedValueOnce([]);
    svc.removeAssetSparePart.mockResolvedValue(undefined);

    render(<AssetSparePartsTab asset={asset} />);

    expect(await screen.findByText("BRG-6205")).toBeTruthy();
    expect(screen.getByText(/spares.criticalShort/)).toBeTruthy();
    expect(screen.getByText(/spares.stockState.out/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: /spares.removeLine/ }));
    fireEvent.click(await screen.findByRole("button", { name: "spares.remove" }));

    await waitFor(() => expect(svc.removeAssetSparePart).toHaveBeenCalledWith(11, 3));
    expect(await screen.findByText("spares.empty")).toBeTruthy();
  });
});
