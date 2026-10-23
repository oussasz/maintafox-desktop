import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { toDateTimeLocal } from "@/components/planning/planning-model";
import { WoCreateForm } from "@/components/wo/WoCreateForm";
import { formatIsoDateForInput } from "@/lib/date-input";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

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
  WoPermitRequirementField: ({
    disabled,
    disabledReason,
  }: {
    disabled?: boolean;
    disabledReason?: string | null;
  }) => (
    <input
      type="checkbox"
      data-testid="permit-box"
      disabled={disabled}
      aria-description={disabledReason ?? ""}
    />
  ),
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

describe("WoCreateForm planned dates", () => {
  it("shows the stored instants in local time and saves the typed local time as an instant", async () => {
    const updateDraft = vi.fn(() => Promise.resolve());
    useWoStore.setState({ updateDraft } as unknown as Partial<
      ReturnType<typeof useWoStore.getState>
    >);
    const initial = {
      id: 5,
      row_version: 3,
      title: "Replace breaker",
      description: null,
      type_code: "",
      subtype_code: "",
      urgency_code: "",
      equipment_id: null,
      planned_start: "2026-10-20T06:00:00Z",
      planned_end: "2026-10-20T12:30:00Z",
      expected_duration_hours: null,
      notes: null,
      requires_permit: false,
    } as unknown as WorkOrder;
    const { container, getByText } = render(
      <WoCreateForm initial={initial} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    const field = (id: string) => container.querySelector<HTMLInputElement>(`#${id}`);
    const shown = (id: string) => `${field(id)?.value} ${field(`${id}-time`)?.value}`;
    const expected = (instant: string) => {
      const [date = "", time = ""] = toDateTimeLocal(instant).split("T");
      return `${formatIsoDateForInput(date, "fr")} ${time}`;
    };
    expect(shown("wo-planned-start")).toBe(expected("2026-10-20T06:00:00Z"));
    expect(shown("wo-planned-end")).toBe(expected("2026-10-20T12:30:00Z"));

    fireEvent.change(field("wo-planned-end") as HTMLInputElement, {
      target: { value: "21/10/2026" },
    });
    fireEvent.change(field("wo-planned-end-time") as HTMLInputElement, {
      target: { value: "09:15" },
    });
    const save = getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(save);
    await waitFor(() => expect(updateDraft).toHaveBeenCalledTimes(1));
    expect(updateDraft).toHaveBeenCalledWith(
      expect.objectContaining({
        planned_start: "2026-10-20T06:00:00.000Z",
        // Typed in local time; sent as the instant it names, never as a zone-less text.
        planned_end: new Date("2026-10-21T09:15").toISOString(),
      }),
    );
  });
});

describe("WoCreateForm permit box in edit mode", () => {
  const draft = (overrides: Partial<WorkOrder>) =>
    ({
      id: 9,
      row_version: 1,
      title: "Fuite pompe",
      description: null,
      type_code: "",
      subtype_code: "",
      urgency_code: "",
      equipment_id: null,
      planned_start: null,
      planned_end: null,
      expected_duration_hours: null,
      notes: null,
      requires_permit: true,
      permit_rule_code: null,
      permit_rule_permit_type: null,
      permit_rule_cleared_by_id: null,
      permit_rule_cleared_at: null,
      permit_rule_clear_reason: null,
      ...overrides,
    }) as unknown as WorkOrder;

  it("is locked for a requirement from a safety request or a plant rule, free otherwise", () => {
    const { getByTestId, rerender } = render(
      <WoCreateForm
        initial={draft({ permit_from_safety_di: true })}
        onSubmitted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(getByTestId("permit-box")).toBeDisabled();
    expect(getByTestId("permit-box")).toHaveAttribute("aria-description", "permitRule.clearHint");

    rerender(
      <WoCreateForm
        initial={draft({ permit_rule_code: "PUMPS_LOTO" })}
        onSubmitted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(getByTestId("permit-box")).toBeDisabled();

    rerender(
      <WoCreateForm
        initial={draft({
          permit_from_safety_di: true,
          permit_rule_cleared_at: "2026-10-22T10:00:00Z",
        })}
        onSubmitted={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(getByTestId("permit-box")).not.toBeDisabled();
  });
});
