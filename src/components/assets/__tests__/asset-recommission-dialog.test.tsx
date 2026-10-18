import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetRecommissionDialog } from "@/components/assets/AssetRecommissionDialog";
import { useEquipmentTaxonomyStore } from "@/stores/equipment-taxonomy-store";
import type { Asset, EquipmentTaxonomyCatalog } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

// The coded refusals are translated by the shared error reason (equipment errors); its own tests
// cover the translation, here only that the dialog uses it.
vi.mock("@/lib/app-error", () => ({
  appErrorReason: (err: { details?: { message: string }[] }) =>
    `reason:${err.details?.[0]?.message ?? "?"}`,
}));

const svc = vi.hoisted(() => ({ recordLifecycleEvent: vi.fn() }));
vi.mock("@/services/asset-lifecycle-service", () => svc);

const status = (code: string, semantic_tag: string) => ({
  id: code.length,
  code,
  label: code,
  parent_id: null,
  color_hex: null,
  semantic_tag,
  is_system: true,
});

const CATALOG: EquipmentTaxonomyCatalog = {
  statuses: [
    status("ACTIVE_IN_SERVICE", "up"),
    status("IN_STOCK", "not_installed"),
    status("DECOMMISSIONED", "retired"),
    status("SCRAPPED", "retired"),
  ],
  criticalities: [],
  classes: [],
  families: [],
  subfamilies: [],
};

const ASSET = {
  id: 5,
  asset_code: "PMP-101",
  status_code: "DECOMMISSIONED",
  row_version: 3,
} as unknown as Asset;

function open(onRecommissioned = vi.fn()) {
  render(
    <AssetRecommissionDialog
      open
      asset={ASSET}
      onClose={vi.fn()}
      onRecommissioned={onRecommissioned}
    />,
  );
  return onRecommissioned;
}

describe("AssetRecommissionDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useEquipmentTaxonomyStore.setState({ catalog: CATALOG, loading: false, error: null });
    svc.recordLifecycleEvent.mockResolvedValue({ id: 1 });
  });

  it("asks for a reason before anything is recorded", async () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "recommission.confirm" }));
    expect(await screen.findByText("recommission.reasonRequired")).toBeInTheDocument();
    expect(svc.recordLifecycleEvent).not.toHaveBeenCalled();
  });

  it("records the return to service with the chosen status and the reason", async () => {
    const done = open();
    fireEvent.change(screen.getByLabelText(/recommission\.reason/), {
      target: { value: "Overhauled, back on line 2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "recommission.confirm" }));
    await waitFor(() =>
      expect(svc.recordLifecycleEvent).toHaveBeenCalledWith({
        asset_id: 5,
        event_type: "RECOMMISSIONED",
        to_status_code: "ACTIVE_IN_SERVICE",
        notes: "Overhauled, back on line 2",
        expected_row_version: 3,
      }),
    );
    await waitFor(() => expect(done).toHaveBeenCalled());
  });

  it("never offers a retired status as the status after recommission", () => {
    open();
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
    expect(screen.queryByRole("option", { name: /DECOMMISSIONED|SCRAPPED/ })).toBeNull();
  });

  it("shows the server's refusal in words and stays open", async () => {
    svc.recordLifecycleEvent.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: "Validation failed: ASSET_STALE_VERSION",
      details: [{ message: "ASSET_STALE_VERSION" }],
    });
    const done = open();
    fireEvent.change(screen.getByLabelText(/recommission\.reason/), {
      target: { value: "Back" },
    });
    fireEvent.click(screen.getByRole("button", { name: "recommission.confirm" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("reason:ASSET_STALE_VERSION");
    expect(done).not.toHaveBeenCalled();
  });
});
