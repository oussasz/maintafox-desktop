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

const navigateMock = vi.hoisted(() => vi.fn());
vi.mock("react-router-dom", () => ({ useNavigate: () => navigateMock }));

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
  asset_id: 5,
  open_wo_rule: "not_yet_closed",
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

function renderModal(onClose = vi.fn()) {
  return render(
    <AssetDecommissionModal open asset={ASSET} onClose={onClose} onDecommissioned={vi.fn()} />,
  );
}

describe("AssetDecommissionModal", () => {
  it("cannot be confirmed when the open-work check failed to load", async () => {
    svc.getAssetBindingSummary.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    renderModal();
    expect(await screen.findByText("decommission.dependenciesError")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /decommission.confirm/ })).toBeDisabled();
    expect(screen.queryByLabelText("decommission.reasonLabel")).not.toBeInTheDocument();
  });

  it("needs a chosen final status: none is preset", async () => {
    svc.getAssetBindingSummary.mockResolvedValueOnce(SUMMARY);
    renderModal();
    const reason = await screen.findByPlaceholderText("decommission.reasonPlaceholder");
    fireEvent.change(reason, { target: { value: "Remplacé par PMP-201" } });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /decommission.confirm/ })).toBeDisabled(),
    );
    expect(screen.getByText("decommission.targetStatusPlaceholder")).toBeInTheDocument();
    expect(svc.decommissionAsset).not.toHaveBeenCalled();
  });

  it("blocked by open work: each blocking count opens its list and no confirm is offered", async () => {
    svc.getAssetBindingSummary.mockResolvedValueOnce({
      ...SUMMARY,
      open_di_count: count(1),
      open_wo_count: count(2),
    });
    const onClose = vi.fn();
    renderModal(onClose);

    const links = await screen.findAllByRole("button", { name: /decommission.openBlockers/ });
    expect(links).toHaveLength(2);
    // Each link names the list it opens.
    expect(links[0]).toHaveAccessibleName("decommission.openBlockers — binding.domains.di");
    expect(screen.queryByRole("button", { name: /decommission.confirm/ })).not.toBeInTheDocument();

    fireEvent.click(links[1] as HTMLElement);
    expect(onClose).toHaveBeenCalled();
    expect(navigateMock).toHaveBeenCalledWith("/work-orders?scope=not_closed&equipment=5");
    fireEvent.click(links[0] as HTMLElement);
    expect(navigateMock).toHaveBeenCalledWith("/requests?scope=open&equipment=5");
  });

  it("offers no list link for a zero count or an unknown open-work rule", async () => {
    svc.getAssetBindingSummary.mockResolvedValueOnce({
      ...SUMMARY,
      open_wo_rule: "future_rule",
      open_wo_count: count(3),
    });
    renderModal();

    expect(await screen.findByText(/decommission.blockerBanner/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /decommission.openBlockers/ })).toBeNull();
  });
});
