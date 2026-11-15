import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetTransferDialog } from "@/components/assets/AssetTransferDialog";
import { localIsoDate } from "@/components/documentation/document-validity";
import type { Asset } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && "count" in opts ? `${key}:${String(opts["count"])}` : key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

vi.mock("@/lib/app-error", () => ({
  appErrorReason: (err: { details?: { message: string }[] }) =>
    `reason:${err.details?.[0]?.message ?? "?"}`,
}));

const svc = vi.hoisted(() => ({ previewAssetTransfer: vi.fn(), transferAsset: vi.fn() }));
vi.mock("@/services/asset-transfer-service", () => svc);

// The searchable list has its own tests: here a plain select over the same options.
vi.mock("@/components/assets/EquipmentSearchSelect", () => ({
  EquipmentSearchSelect: (props: {
    id: string;
    options: { value: string; label: string }[];
    value: string;
    onChange: (v: string) => void;
  }) => (
    <select
      data-testid={props.id}
      value={props.value}
      onChange={(e) => props.onChange(e.target.value)}
    >
      <option value="">-</option>
      {props.options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  ),
}));

const ASSET = {
  id: 7,
  asset_code: "PMP-101",
  asset_name: "Pompe",
  org_node_id: 1,
  org_node_name: "Ligne 1",
  row_version: 4,
} as unknown as Asset;

const PREVIEW = {
  asset_id: 7,
  from_org_node_id: 1,
  impact: {
    active_pm_plan_count: 2,
    spare_part_line_count: 3,
    critical_spare_part_count: 1,
    open_wo_count: 1,
  },
  destinations: [{ id: 2, code: "L2", name: "Ligne 2", node_type_label: "Ligne" }],
};

function renderDialog(onTransferred = vi.fn()) {
  render(
    <AssetTransferDialog open asset={ASSET} onClose={vi.fn()} onTransferred={onTransferred} />,
  );
  return onTransferred;
}

beforeEach(() => {
  vi.clearAllMocks();
  svc.previewAssetTransfer.mockResolvedValue(PREVIEW);
});

describe("AssetTransferDialog", () => {
  it("warns with the counts of what stays attached before confirming", async () => {
    renderDialog();
    const impact = await screen.findByTestId("transfer-impact");
    expect(svc.previewAssetTransfer).toHaveBeenCalledWith(7);
    expect(impact).toHaveTextContent("transfer.impact.pmPlans:2");
    expect(impact).toHaveTextContent("transfer.impact.spareParts:3");
    expect(impact).toHaveTextContent("transfer.impact.openWo:1");
    expect(impact).toHaveTextContent("transfer.impact.notChanged");
    // Only the destinations the server allows are offered.
    const options = Array.from(
      (screen.getByTestId("transfer-target") as HTMLSelectElement).options,
    ).map((o) => o.textContent);
    expect(options).toEqual(["-", "L2 — Ligne 2"]);
  });

  it("says nothing is attached when every count is zero", async () => {
    svc.previewAssetTransfer.mockResolvedValue({
      ...PREVIEW,
      impact: {
        active_pm_plan_count: 0,
        spare_part_line_count: 0,
        critical_spare_part_count: 0,
        open_wo_count: 0,
      },
    });
    renderDialog();
    expect(await screen.findByText("transfer.impact.none")).toBeInTheDocument();
  });

  it("requires a destination and a reason", async () => {
    renderDialog();
    await screen.findByTestId("transfer-impact");
    fireEvent.click(screen.getByRole("button", { name: /transfer\.confirm/ }));
    expect(await screen.findByText("transfer.targetRequired")).toBeInTheDocument();
    expect(screen.getByText("transfer.reasonRequired")).toBeInTheDocument();
    expect(svc.transferAsset).not.toHaveBeenCalled();
  });

  it("sends the destination, today's date, the reason and the version seen", async () => {
    const moved = { ...ASSET, org_node_id: 2, org_node_name: "Ligne 2", row_version: 5 };
    svc.transferAsset.mockResolvedValue({ asset: moved, event: {}, impact: PREVIEW.impact });
    const onTransferred = renderDialog();
    await screen.findByTestId("transfer-impact");

    fireEvent.change(screen.getByTestId("transfer-target"), { target: { value: "2" } });
    fireEvent.change(screen.getByLabelText(/transfer\.reason/), {
      target: { value: "  Réaffectation  " },
    });
    fireEvent.click(screen.getByRole("button", { name: /transfer\.confirm/ }));

    await waitFor(() => expect(onTransferred).toHaveBeenCalledWith(moved));
    expect(svc.transferAsset).toHaveBeenCalledWith({
      asset_id: 7,
      to_org_node_id: 2,
      effective_date: localIsoDate(new Date()),
      reason: "Réaffectation",
      expected_row_version: 4,
    });
  });

  it("shows the server's refusal and keeps the dialog open", async () => {
    svc.transferAsset.mockRejectedValue({
      code: "VALIDATION_FAILED",
      details: [{ message: "GATE_ASSET_TRANSFER_SAME_NODE" }],
    });
    const onTransferred = renderDialog();
    await screen.findByTestId("transfer-impact");
    fireEvent.change(screen.getByTestId("transfer-target"), { target: { value: "2" } });
    fireEvent.change(screen.getByLabelText(/transfer\.reason/), { target: { value: "motif" } });
    fireEvent.click(screen.getByRole("button", { name: /transfer\.confirm/ }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "reason:GATE_ASSET_TRANSFER_SAME_NODE",
    );
    expect(onTransferred).not.toHaveBeenCalled();
  });
});
