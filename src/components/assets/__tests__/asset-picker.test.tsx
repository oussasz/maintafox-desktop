import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { assetQrPayload } from "@/lib/asset-qr";
import type { AssetSearchResult } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && "total" in opts ? `${key}:${String(opts["shown"])}/${String(opts["total"])}` : key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const svc = vi.hoisted(() => ({
  searchAssetsPage: vi.fn(),
  suggestPickerAssets: vi.fn(),
}));
vi.mock("@/services/asset-search-service", () => svc);
vi.mock("@/components/assets/AssetStatusBadge", () => ({ AssetStatusBadge: () => null }));
vi.mock("@/components/assets/CriticalityBadge", () => ({
  CriticalityBadge: ({ criticality }: { criticality: string | null }) => (
    <span>crit:{criticality}</span>
  ),
}));

function asset(id: number, code: string): AssetSearchResult {
  return {
    id,
    asset_code: code,
    asset_name: `Pompe ${code}`,
    status_code: "ACTIVE_IN_SERVICE",
  } as AssetSearchResult;
}

beforeEach(() => {
  vi.clearAllMocks();
  svc.suggestPickerAssets.mockResolvedValue({ items: [], mode: "recent" });
});

function type(text: string) {
  const box = screen.getByRole("combobox");
  fireEvent.change(box, { target: { value: text } });
  return box;
}

function permissionsWithout(...denied: PermissionName[]): PermissionContextValue {
  const can = (p: PermissionName) => !denied.includes(p);
  return {
    permissions: [],
    isLoading: false,
    can,
    canAny: (...ps) => ps.some(can),
    canAll: (...ps) => ps.every(can),
    refresh: vi.fn(),
  };
}

describe("AssetPicker", () => {
  it("names the missing right instead of searching when the user may not read assets", () => {
    render(
      <PermissionContext.Provider value={permissionsWithout(P.EQ_VIEW)}>
        <AssetPicker value={null} onChange={vi.fn()} />
      </PermissionContext.Provider>,
    );
    expect(screen.getByTestId("list-access-assets")).toHaveTextContent("listAccess.assets");
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(svc.suggestPickerAssets).not.toHaveBeenCalled();
    expect(svc.searchAssetsPage).not.toHaveBeenCalled();
  });

  it("shows a failed search with a retry instead of 'no asset found'", async () => {
    svc.searchAssetsPage.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "busy" });
    render(<AssetPicker value={null} onChange={vi.fn()} />);
    type("PMP");
    expect(await screen.findByRole("alert")).toHaveTextContent("assetPicker.searchFailed");
    expect(screen.queryByText("assetPicker.empty")).not.toBeInTheDocument();

    svc.searchAssetsPage.mockResolvedValueOnce({ items: [asset(1, "PMP-101")], total: 1 });
    fireEvent.click(screen.getByRole("button", { name: "assetPicker.retry" }));
    expect(await screen.findByRole("option")).toHaveTextContent("Pompe PMP-101");
  });

  it("says when more assets match than are listed", async () => {
    svc.searchAssetsPage.mockResolvedValueOnce({
      items: [asset(1, "PMP-101"), asset(2, "PMP-102")],
      total: 57,
    });
    render(<AssetPicker value={null} onChange={vi.fn()} />);
    type("PMP");
    expect(await screen.findByText("assetPicker.more:2/57")).toBeInTheDocument();
  });

  it("selects the asset of a scanned label by its code", async () => {
    const onChange = vi.fn();
    svc.searchAssetsPage.mockResolvedValue({ items: [asset(7, "PMP-101/A")], total: 1 });
    render(<AssetPicker value={null} onChange={onChange} />);
    const box = type(assetQrPayload("PMP-101/A"));
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(asset(7, "PMP-101/A")));
    expect(svc.searchAssetsPage).toHaveBeenCalledWith(
      expect.objectContaining({ query: "PMP-101/A" }),
    );
  });

  it("refuses a label of the former id format instead of guessing an asset", async () => {
    const onChange = vi.fn();
    render(<AssetPicker value={null} onChange={onChange} />);
    const box = type("maintafox://asset/42");
    fireEvent.keyDown(box, { key: "Enter" });
    expect(await screen.findByText("assetPicker.legacyLabel")).toBeInTheDocument();
    expect(svc.searchAssetsPage).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("marks a criticality from the old list to review in the options and the chosen asset", async () => {
    svc.searchAssetsPage.mockResolvedValueOnce({
      items: [
        { ...asset(1, "PMP-101"), criticality_code: "A", criticality_to_review: true },
        { ...asset(2, "PMP-102"), criticality_code: "B", criticality_to_review: false },
      ],
      total: 2,
    });
    const { rerender } = render(<AssetPicker value={null} onChange={vi.fn()} />);
    type("PMP");
    const options = await screen.findAllByRole("option");
    expect(options[0]).toHaveTextContent("crit:A");
    expect(options[0]).toHaveTextContent("listValue.review");
    expect(options[1]).toHaveTextContent("crit:B");
    expect(options[1]).not.toHaveTextContent("listValue.review");

    rerender(
      <AssetPicker
        value={{ ...asset(1, "PMP-101"), criticality_code: "A", criticality_to_review: true }}
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByText("listValue.review").closest("[title]")).toHaveAttribute(
      "title",
      "criticalityReview.hint",
    );
  });
});
