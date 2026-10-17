import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useEquipmentTree } from "@/components/assets/tree/use-equipment-tree";
import type { AssetTreeRow } from "@/services/asset-search-service";
import type { AssetSearchFilters } from "@shared/ipc-types";

const svc = vi.hoisted(() => ({ listAssetTreeLevel: vi.fn() }));
vi.mock("@/services/asset-search-service", () => svc);

function row(id: number, code: string, childCount = 0, isMatch = true): AssetTreeRow {
  return {
    id,
    entity_sync_id: `s${id}`,
    asset_code: code,
    asset_name: code,
    class_code: null,
    class_name: null,
    family_code: null,
    family_name: null,
    subfamily_code: null,
    subfamily_name: null,
    criticality_code: null,
    status_code: "ACTIVE_IN_SERVICE",
    org_node_id: null,
    org_node_name: null,
    org_path: null,
    parent_asset_id: null,
    parent_asset_code: null,
    parent_asset_name: null,
    primary_meter_name: null,
    primary_meter_reading: null,
    primary_meter_unit: null,
    primary_meter_last_read_at: null,
    external_id_count: 0,
    row_version: 1,
    child_count: childCount,
    is_match: isMatch,
  };
}

const FILTERS = { query: null } as unknown as AssetSearchFilters;

describe("useEquipmentTree", () => {
  beforeEach(() => vi.clearAllMocks());

  it("loads the roots, then a branch's children from the database on expand", async () => {
    svc.listAssetTreeLevel.mockImplementation(async (_f: unknown, parentId: number | null) =>
      parentId == null
        ? { parent_id: null, items: [row(1, "ROOT", 2, false)], total: 1 }
        : { parent_id: 1, items: [row(2, "PUMP-1"), row(3, "PUMP-2")], total: 3 },
    );
    const { result } = renderHook(() => useEquipmentTree(FILTERS));
    await waitFor(() => expect(result.current.rootLoading).toBe(false));
    expect(result.current.rows.map((r) => r.kind)).toEqual(["asset"]);

    act(() => result.current.toggle(1));
    await waitFor(() =>
      expect(result.current.rows.filter((r) => r.kind === "asset")).toHaveLength(3),
    );
    expect(svc.listAssetTreeLevel).toHaveBeenLastCalledWith(
      expect.objectContaining({ offset: 0, limit: 100 }),
      1,
    );
    // The level says one more child exists: a "show more" row, never a silent cap.
    const more = result.current.rows.find((r) => r.kind === "status");
    expect(more).toMatchObject({ state: "more", remaining: 1, parent: 1 });

    act(() => result.current.loadMore(1));
    await waitFor(() =>
      expect(svc.listAssetTreeLevel).toHaveBeenLastCalledWith(
        expect.objectContaining({ offset: 2 }),
        1,
      ),
    );

    act(() => result.current.toggle(1));
    expect(result.current.rows.filter((r) => r.kind === "asset")).toHaveLength(1);
  });

  it("shows a failed branch as an error with retry, never as empty", async () => {
    svc.listAssetTreeLevel.mockImplementation(async (_f: unknown, parentId: number | null) => {
      if (parentId == null) return { parent_id: null, items: [row(1, "ROOT", 1)], total: 1 };
      throw { code: "DATABASE_ERROR", message: "locked" };
    });
    const { result } = renderHook(() => useEquipmentTree(FILTERS));
    await waitFor(() => expect(result.current.rootLoading).toBe(false));
    act(() => result.current.toggle(1));
    await waitFor(() =>
      expect(result.current.rows.find((r) => r.kind === "status")).toMatchObject({
        state: "error",
        parent: 1,
      }),
    );
    svc.listAssetTreeLevel.mockResolvedValue({ parent_id: 1, items: [row(2, "C")], total: 1 });
    act(() => result.current.retry(1));
    await waitFor(() =>
      expect(result.current.rows.filter((r) => r.kind === "asset")).toHaveLength(2),
    );
  });

  it("reports a failed root load", async () => {
    svc.listAssetTreeLevel.mockRejectedValue({ code: "DATABASE_ERROR", message: "x" });
    const { result } = renderHook(() => useEquipmentTree(FILTERS));
    await waitFor(() => expect(result.current.rootError).not.toBeNull());
    expect(result.current.rootEmpty).toBe(false);
  });

  it("expands every loaded branch on expand all", async () => {
    svc.listAssetTreeLevel.mockImplementation(async (_f: unknown, parentId: number | null) =>
      parentId == null
        ? { parent_id: null, items: [row(1, "A", 1), row(5, "B", 0)], total: 2 }
        : { parent_id: parentId, items: [row(10 + parentId, "child")], total: 1 },
    );
    const { result } = renderHook(() => useEquipmentTree(FILTERS));
    await waitFor(() => expect(result.current.rootLoading).toBe(false));
    act(() => result.current.expandLoaded());
    await waitFor(() =>
      expect(result.current.rows.filter((r) => r.kind === "asset")).toHaveLength(3),
    );
    act(() => result.current.collapseAll());
    expect(result.current.expanded.size).toBe(0);
  });
});
