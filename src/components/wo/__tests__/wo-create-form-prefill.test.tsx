import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { WoCreateForm } from "@/components/wo/WoCreateForm";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));

const assetSvc = vi.hoisted(() => ({ getAssetByIdSilent: vi.fn() }));
vi.mock("@/services/asset-service", () => assetSvc);
vi.mock("@/services/wo-service", () => ({
  listWorkOrderSubtypes: vi.fn(() => Promise.resolve([])),
  listWorkOrderTypes: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({ value }: { value: { asset_code: string } | null }) => (
    <div data-testid="asset-picker">{value?.asset_code ?? "none"}</div>
  ),
}));
vi.mock("@/components/common/urgency/UrgencySelect", () => ({ UrgencySelect: () => null }));
vi.mock("@/components/wo/WoPermitRequirementField", () => ({
  WoPermitRequirementField: () => null,
}));

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
});

describe("WoCreateForm equipment pre-fill", () => {
  it("opens with no equipment when only a selection left by the reliability screen exists", async () => {
    // A value the former RAMS hand-off left in the browser storage.
    localStorage.setItem("maintafox.rams.selectedEquipmentId", "42");
    const { getByTestId } = render(
      <WoCreateForm initial={null} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    await waitFor(() => expect(getByTestId("asset-picker")).toHaveTextContent("none"));
    expect(assetSvc.getAssetByIdSilent).not.toHaveBeenCalled();
  });

  it("uses the equipment the caller passes", async () => {
    assetSvc.getAssetByIdSilent.mockResolvedValueOnce(null);
    render(
      <WoCreateForm
        initial={null}
        prefillEquipmentId={7}
        onSubmitted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    await waitFor(() => expect(assetSvc.getAssetByIdSilent).toHaveBeenCalledWith(7));
  });
});
