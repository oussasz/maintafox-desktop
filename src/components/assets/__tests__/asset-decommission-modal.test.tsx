import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetDecommissionModal } from "@/components/assets/AssetDecommissionModal";
import { useEquipmentTaxonomyStore } from "@/stores/equipment-taxonomy-store";
import type { Asset, AssetBindingSummary, EquipmentTaxonomyCatalog } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const svc = vi.hoisted(() => ({
  decommissionAsset: vi.fn(),
  getAssetBindingSummary: vi.fn(),
}));
vi.mock("@/services/asset-lifecycle-service", () => svc);

const opt = (id: number, code: string, tag: string) => ({
  id,
  code,
  label: code,
  parent_id: null,
  color_hex: null,
  semantic_tag: tag,
  is_system: true,
});
const CATALOG: EquipmentTaxonomyCatalog = {
  statuses: [
    opt(1, "ACTIVE_IN_SERVICE", "up"),
    opt(2, "DECOMMISSIONED", "retired"),
    opt(3, "SCRAPPED", "retired"),
  ],
  criticalities: [],
  classes: [],
  families: [],
  subfamilies: [],
};

const ASSET = {
  id: 5,
  asset_code: "PMP-101",
  asset_name: "Pompe",
  class_name: "Pompe",
  status_code: "ACTIVE_IN_SERVICE",
} as Asset;

const count = (n: number) => ({ count: n, status: "available" });
const SUMMARY = {
  open_di_count: count(0),
  open_wo_count: count(0),
  linked_pm_plan_count: count(0),
  linked_iot_signal_count: count(0),
  linked_document_count: count(0),
} as unknown as AssetBindingSummary;

beforeEach(() => {
  vi.clearAllMocks();
  useEquipmentTaxonomyStore.setState({ catalog: CATALOG, loading: false, error: null });
});

describe("AssetDecommissionModal", () => {
  it("cannot be confirmed when the open-work check failed to load", async () => {
    svc.getAssetBindingSummary.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    render(
      <AssetDecommissionModal open asset={ASSET} onClose={vi.fn()} onDecommissioned={vi.fn()} />,
    );
    expect(await screen.findByText("decommission.dependenciesError")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /decommission.confirm/ })).toBeDisabled();
    expect(screen.queryByLabelText("decommission.reasonLabel")).not.toBeInTheDocument();
  });

  it("needs a chosen final status: none is preset", async () => {
    svc.getAssetBindingSummary.mockResolvedValueOnce(SUMMARY);
    render(
      <AssetDecommissionModal open asset={ASSET} onClose={vi.fn()} onDecommissioned={vi.fn()} />,
    );
    const reason = await screen.findByPlaceholderText("decommission.reasonPlaceholder");
    fireEvent.change(reason, { target: { value: "Remplacé par PMP-201" } });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /decommission.confirm/ })).toBeDisabled(),
    );
    expect(screen.getByText("decommission.targetStatusPlaceholder")).toBeInTheDocument();
    expect(svc.decommissionAsset).not.toHaveBeenCalled();
  });
});
