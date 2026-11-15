/**
 * Structure tab, current parent (L2-03): only an active PARENT_CHILD / INSTALLED_IN relation is a
 * parent, as on the server; with several at once the move is not offered and the reason is shown.
 */

import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AssetStructureTab } from "@/components/assets/detail/AssetStructureTab";
import type { Asset, AssetHierarchyRow } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "count" in opts ? `${key}[${String(opts["count"])}]` : key,
    i18n: { language: "fr" },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

vi.mock("@/components/assets/AssetMoveComponentDialog", () => ({
  AssetMoveComponentDialog: () => null,
}));

vi.mock("@/components/assets/AssetStatusBadge", () => ({ AssetStatusBadge: () => null }));

const svc = vi.hoisted(() => ({
  listAssetParents: vi.fn(),
  listAssetChildren: vi.fn(),
  getAssetById: vi.fn(),
}));
vi.mock("@/services/asset-service", () => svc);

const asset = (id: number, code: string) =>
  ({ id, asset_code: code, asset_name: `Asset ${code}`, row_version: 1 }) as Asset;

const relation = (id: number, parent: number, type: string): AssetHierarchyRow => ({
  relation_id: id,
  parent_asset_id: parent,
  child_asset_id: 5,
  relation_type: type,
  effective_from: "2026-11-01T00:00:00+00:00",
  effective_to: null,
});

const ASSETS: Record<number, Asset> = {
  10: asset(10, "MAC-A"),
  11: asset(11, "MAC-C"),
  12: asset(12, "PMP-D"),
};

describe("AssetStructureTab — current parent", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.listAssetChildren.mockResolvedValue([]);
    svc.getAssetById.mockImplementation(async (id: number) => ASSETS[id]);
  });

  it("does not take a DRIVES link for the parent", async () => {
    svc.listAssetParents.mockResolvedValue([relation(1, 12, "DRIVES")]);
    render(<AssetStructureTab asset={asset(5, "MOT-01")} onOpenAsset={vi.fn()} />);
    expect(await screen.findByText("structure.noParent")).toBeInTheDocument();
    expect(screen.queryByText("PMP-D")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "moveComponent.action" })).toBeEnabled();
  });

  it("lists both parents and does not offer the move when there are two", async () => {
    svc.listAssetParents.mockResolvedValue([
      relation(1, 10, "PARENT_CHILD"),
      relation(2, 11, "INSTALLED_IN"),
    ]);
    render(<AssetStructureTab asset={asset(5, "MOT-01")} onOpenAsset={vi.fn()} />);
    expect(await screen.findByText("MAC-A")).toBeInTheDocument();
    expect(screen.getByText("MAC-C")).toBeInTheDocument();
    expect(screen.getByText("moveComponent.severalParents[2]")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "moveComponent.action" })).toBeDisabled();
  });
});
