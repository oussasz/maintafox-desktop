import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetMoveComponentDialog } from "@/components/assets/AssetMoveComponentDialog";
import { AssetTimelineList } from "@/components/assets/history/AssetTimelineList";
import { localIsoDate } from "@/components/documentation/document-validity";
import type { Asset, AssetHistoryEvent } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && ("parent" in opts || "component" in opts || "code" in opts)
      ? `${key}[${String(opts["date"] ?? "")}|${String(opts["parent"] ?? opts["component"] ?? opts["code"])}]`
      : key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

vi.mock("@/lib/app-error", () => ({
  appErrorReason: (err: { details?: { message: string }[] }) =>
    `reason:${err.details?.[0]?.message ?? "?"}`,
}));

const svc = vi.hoisted(() => ({ moveAssetComponent: vi.fn() }));
vi.mock("@/services/asset-lifecycle-service", () => svc);

// The hierarchy below the component: 5 → 6 → 7.
const assets = vi.hoisted(() => ({
  listAssetChildren: vi.fn(async (id: number) =>
    id === 5
      ? [{ relation_id: 1, parent_asset_id: 5, child_asset_id: 6 }]
      : id === 6
        ? [{ relation_id: 2, parent_asset_id: 6, child_asset_id: 7 }]
        : [],
  ),
}));
vi.mock("@/services/asset-service", () => assets);

// The parent picker searches the registry; here it offers two machines and shows what it hides.
const picker = vi.hoisted(() => ({ excluded: [] as number[] }));
vi.mock("@/components/assets/EquipmentParentPicker", () => ({
  EquipmentParentPicker: ({
    onChange,
    excludeIds,
  }: {
    onChange: (id: number | null) => void;
    excludeIds?: Set<number>;
  }) => {
    picker.excluded = [...(excludeIds ?? [])].sort();
    return (
      <div>
        <button type="button" onClick={() => onChange(20)}>
          pick-20
        </button>
        <button type="button" onClick={() => onChange(null)}>
          pick-none
        </button>
      </div>
    );
  },
}));

const MOTOR = { id: 5, asset_code: "MOT-01", asset_name: "Moteur", row_version: 4 } as Asset;
const MACHINE_A = { id: 10, asset_code: "MAC-A", asset_name: "Machine A" } as Asset;

function open(currentParent: Asset | null, onMoved = vi.fn()) {
  render(
    <AssetMoveComponentDialog
      open
      asset={MOTOR}
      currentParent={currentParent}
      onClose={vi.fn()}
      onMoved={onMoved}
    />,
  );
  return onMoved;
}

describe("AssetMoveComponentDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.moveAssetComponent.mockResolvedValue({});
  });

  it("moves the component to the chosen parent from today, with the note", async () => {
    const onMoved = open(MACHINE_A);
    await waitFor(() => expect(picker.excluded).toEqual([5, 6, 7]));
    fireEvent.click(screen.getByRole("button", { name: "pick-20" }));
    fireEvent.change(screen.getByLabelText("moveComponent.notes"), {
      target: { value: "  Échange standard  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "moveComponent.confirm" }));

    await waitFor(() => expect(onMoved).toHaveBeenCalled());
    expect(svc.moveAssetComponent).toHaveBeenCalledWith({
      asset_id: 5,
      new_parent_asset_id: 20,
      effective_date: localIsoDate(new Date()),
      notes: "Échange standard",
      expected_row_version: 4,
    });
  });

  it("removes an installed component to stock when no parent is chosen", async () => {
    open(MACHINE_A);
    fireEvent.click(screen.getByRole("button", { name: "moveComponent.confirmRemove" }));
    await waitFor(() => expect(svc.moveAssetComponent).toHaveBeenCalled());
    expect(svc.moveAssetComponent.mock.calls[0]?.[0]).toMatchObject({
      new_parent_asset_id: null,
      notes: null,
    });
  });

  it("does not send a component in stock back to stock", async () => {
    open(null);
    fireEvent.click(screen.getByRole("button", { name: "moveComponent.confirmRemove" }));
    expect(await screen.findByText("moveComponent.alreadyInStock")).toBeInTheDocument();
    expect(svc.moveAssetComponent).not.toHaveBeenCalled();
  });

  it("shows the server's refusal", async () => {
    svc.moveAssetComponent.mockRejectedValue({
      details: [{ message: "GATE_ASSET_MOVE_DATE_BEFORE_INSTALL" }],
    });
    open(MACHINE_A);
    fireEvent.click(screen.getByRole("button", { name: "pick-20" }));
    fireEvent.click(screen.getByRole("button", { name: "moveComponent.confirm" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "reason:GATE_ASSET_MOVE_DATE_BEFORE_INSTALL",
    );
  });
});

describe("AssetTimelineList — component moves", () => {
  const movement = (
    id: string,
    event_type: string,
    movement: string,
    key: "parent" | "component",
    code: string,
  ): AssetHistoryEvent =>
    ({
      id,
      asset_id: 5,
      event_type,
      occurred_at: "2026-11-05T00:00:00+00:00",
      title: "Component installed",
      description: null,
      actor_label: null,
      duration_minutes: null,
      status_label: null,
      metadata: { movement, [`${key}_code`]: code, [`${key}_name`]: "Machine" },
      ref: null,
    }) as unknown as AssetHistoryEvent;

  it("says posé / déposé with the date and the other asset", () => {
    render(
      <AssetTimelineList
        events={[
          movement("a", "parent_changed", "installed", "parent", "MAC-B"),
          movement("b", "parent_changed", "removed", "parent", "MAC-A"),
          movement("c", "child_changed", "installed", "component", "MOT-01"),
        ]}
        onOpenRef={vi.fn()}
      />,
    );
    expect(
      screen.getByText(/history\.titles\.component_installed\[.+\|MAC-B — Machine\]/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/history\.titles\.component_removed\[.+\|MAC-A — Machine\]/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/history\.titles\.child_installed\[.+\|MOT-01 — Machine\]/),
    ).toBeInTheDocument();
  });
});
