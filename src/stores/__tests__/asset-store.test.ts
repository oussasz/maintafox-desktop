/**
 * Behaviour tests for the asset registry store (list, detail, hierarchy, forms, tree).
 * Only the asset service (IPC boundary) is mocked.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createAsset,
  getAssetById,
  linkAssetHierarchy,
  listAssetChildren,
  listAssets,
  moveAssetOrgNode,
  unlinkAssetHierarchy,
  updateAssetIdentity,
} from "@/services/asset-service";
import type {
  Asset,
  AssetHierarchyRow,
  CreateAssetPayload,
  UpdateAssetIdentityPayload,
} from "@shared/ipc-types";

import { useAssetStore } from "../asset-store";

vi.mock("@/services/asset-service", () => ({
  listAssets: vi.fn(),
  getAssetById: vi.fn(),
  createAsset: vi.fn(),
  updateAssetIdentity: vi.fn(),
  listAssetChildren: vi.fn(),
  linkAssetHierarchy: vi.fn(),
  unlinkAssetHierarchy: vi.fn(),
  moveAssetOrgNode: vi.fn(),
}));

const mockListAssets = vi.mocked(listAssets);
const mockGetAssetById = vi.mocked(getAssetById);
const mockCreateAsset = vi.mocked(createAsset);
const mockUpdateAssetIdentity = vi.mocked(updateAssetIdentity);
const mockListAssetChildren = vi.mocked(listAssetChildren);
const mockLinkAssetHierarchy = vi.mocked(linkAssetHierarchy);
const mockUnlinkAssetHierarchy = vi.mocked(unlinkAssetHierarchy);
const mockMoveAssetOrgNode = vi.mocked(moveAssetOrgNode);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeAsset(overrides: Partial<Asset> = {}): Asset {
  return {
    id: 1,
    entity_sync_id: "asset-sync-1",
    asset_code: "PMP-001",
    asset_name: "Pompe principale",
    class_id: 3,
    class_code: "PUMP",
    class_name: "Pompe",
    family_code: "ROT",
    family_name: "Rotating",
    subfamily_code: null,
    subfamily_name: null,
    criticality_value_id: 7,
    criticality_code: "A",
    status_code: "ACTIVE",
    manufacturer: "Grundfos",
    model: "CR-10",
    serial_number: "SN-123",
    maintainable_boundary: true,
    rams_schedule_reference_value_id: null,
    rams_schedule_reference_value_name: null,
    rams_utilization_factor: 1,
    org_node_id: 5,
    org_node_name: "Atelier",
    commissioned_at: "2025-01-01",
    decommissioned_at: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
    row_version: 1,
    ...overrides,
  };
}

function makeRelation(overrides: Partial<AssetHierarchyRow> = {}): AssetHierarchyRow {
  return {
    relation_id: 100,
    parent_asset_id: 1,
    child_asset_id: 2,
    relation_type: "PARENT_CHILD",
    effective_from: "2026-01-01",
    effective_to: null,
    ...overrides,
  };
}

function makeCreatePayload(overrides: Partial<CreateAssetPayload> = {}): CreateAssetPayload {
  return {
    asset_code: "PMP-002",
    asset_name: "Pompe secours",
    class_code: "PUMP",
    criticality_code: "B",
    status_code: "ACTIVE",
    maintainable_boundary: true,
    org_node_id: 5,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  let reject: (reason: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const initial = useAssetStore.getState();

function resetStore() {
  useAssetStore.setState(
    { ...initial, treeExpandedIds: new Set<number>(), treeChildren: new Map<number, Asset[]>() },
    true,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  resetStore();
});

// ── loadAssets ────────────────────────────────────────────────────────────────

describe("loadAssets", () => {
  it("passes the filters to the service and stores the list", async () => {
    const assets = [makeAsset(), makeAsset({ id: 2, asset_code: "PMP-002" })];
    mockListAssets.mockResolvedValueOnce(assets);

    await useAssetStore.getState().loadAssets("ACTIVE", 5, "pump");

    expect(mockListAssets).toHaveBeenCalledWith("ACTIVE", 5, "pump");
    expect(useAssetStore.getState().list).toEqual(assets);
    expect(useAssetStore.getState().loading).toBe(false);
    expect(useAssetStore.getState().error).toBeNull();
  });

  it("sets loading while the request is in flight and clears a previous error", async () => {
    useAssetStore.setState({ error: "old" });
    const d = deferred<Asset[]>();
    mockListAssets.mockReturnValueOnce(d.promise);

    const p = useAssetStore.getState().loadAssets();
    expect(useAssetStore.getState().loading).toBe(true);
    expect(useAssetStore.getState().error).toBeNull();

    d.resolve([]);
    await p;
    expect(useAssetStore.getState().loading).toBe(false);
  });

  it("swallows a failure into error and keeps the previous list", async () => {
    const existing = [makeAsset()];
    useAssetStore.setState({ list: existing });
    mockListAssets.mockRejectedValueOnce({ code: "DB", message: "db down" });

    await expect(useAssetStore.getState().loadAssets()).resolves.toBeUndefined();

    expect(useAssetStore.getState().error).toBe("db down");
    expect(useAssetStore.getState().list).toBe(existing);
    expect(useAssetStore.getState().loading).toBe(false);
  });
});

// ── selectAsset ───────────────────────────────────────────────────────────────

describe("selectAsset", () => {
  it("null clears selection, hierarchy and error without calling the service", async () => {
    useAssetStore.setState({
      selectedAsset: makeAsset(),
      hierarchy: [makeRelation()],
      error: "x",
    });

    await useAssetStore.getState().selectAsset(null);

    const s = useAssetStore.getState();
    expect(s.selectedAsset).toBeNull();
    expect(s.hierarchy).toEqual([]);
    expect(s.error).toBeNull();
    expect(mockGetAssetById).not.toHaveBeenCalled();
    expect(mockListAssetChildren).not.toHaveBeenCalled();
  });

  it("loads the asset detail and its children in parallel", async () => {
    const asset = makeAsset({ id: 9 });
    const rows = [makeRelation({ parent_asset_id: 9 })];
    const dAsset = deferred<Asset>();
    mockGetAssetById.mockReturnValueOnce(dAsset.promise);
    mockListAssetChildren.mockResolvedValueOnce(rows);

    const p = useAssetStore.getState().selectAsset(9);
    expect(useAssetStore.getState().loading).toBe(true);
    // Both requests were issued before the first one resolved.
    expect(mockGetAssetById).toHaveBeenCalledWith(9);
    expect(mockListAssetChildren).toHaveBeenCalledWith(9);

    dAsset.resolve(asset);
    await p;

    const s = useAssetStore.getState();
    expect(s.selectedAsset).toEqual(asset);
    expect(s.hierarchy).toEqual(rows);
    expect(s.loading).toBe(false);
  });

  it("swallows a failure into error and leaves the previous selection", async () => {
    const prev = makeAsset({ id: 1 });
    useAssetStore.setState({ selectedAsset: prev });
    mockGetAssetById.mockRejectedValueOnce(new Error("not found"));
    mockListAssetChildren.mockResolvedValueOnce([]);

    await useAssetStore.getState().selectAsset(2);

    const s = useAssetStore.getState();
    expect(s.error).toBe("not found");
    expect(s.selectedAsset).toBe(prev);
    expect(s.loading).toBe(false);
  });
});

// ── createAsset ───────────────────────────────────────────────────────────────

describe("createAsset", () => {
  it("creates, reloads the list, selects the new asset and returns it", async () => {
    const created = makeAsset({ id: 20 });
    const reloaded = [makeAsset(), created];
    const d = deferred<Asset>();
    mockCreateAsset.mockReturnValueOnce(d.promise);
    mockListAssets.mockResolvedValueOnce(reloaded);
    const payload = makeCreatePayload();

    const p = useAssetStore.getState().createAsset(payload);
    expect(useAssetStore.getState().saving).toBe(true);
    d.resolve(created);
    const result = await p;

    expect(result).toEqual(created);
    expect(mockCreateAsset).toHaveBeenCalledWith(payload);
    expect(mockLinkAssetHierarchy).not.toHaveBeenCalled();
    expect(mockListAssets).toHaveBeenCalledWith();
    const s = useAssetStore.getState();
    expect(s.list).toEqual(reloaded);
    expect(s.selectedAsset).toEqual(created);
    expect(s.saving).toBe(false);
  });

  it("links the new asset under the given parent before reloading", async () => {
    const created = makeAsset({ id: 20 });
    mockCreateAsset.mockResolvedValueOnce(created);
    mockLinkAssetHierarchy.mockResolvedValueOnce(makeRelation({ parent_asset_id: 4 }));
    mockListAssets.mockResolvedValueOnce([created]);

    await useAssetStore.getState().createAsset(makeCreatePayload(), { parentAssetId: 4 });

    expect(mockLinkAssetHierarchy).toHaveBeenCalledWith({
      parent_asset_id: 4,
      child_asset_id: 20,
      relation_type: "PARENT_CHILD",
    });
    const linkOrder = mockLinkAssetHierarchy.mock.invocationCallOrder[0];
    const listOrder = mockListAssets.mock.invocationCallOrder[0];
    expect(linkOrder).toBeLessThan(listOrder ?? 0);
  });

  it.each([
    ["null parent", null],
    ["zero parent", 0],
    ["negative parent", -1],
    ["parent equal to the new asset", 20],
  ])("does not link with %s", async (_label, parentAssetId) => {
    mockCreateAsset.mockResolvedValueOnce(makeAsset({ id: 20 }));
    mockListAssets.mockResolvedValueOnce([]);

    await useAssetStore.getState().createAsset(makeCreatePayload(), { parentAssetId });

    expect(mockLinkAssetHierarchy).not.toHaveBeenCalled();
  });

  it("sets error, clears saving and rethrows when creation fails", async () => {
    const err = new Error("duplicate code");
    mockCreateAsset.mockRejectedValueOnce(err);

    await expect(useAssetStore.getState().createAsset(makeCreatePayload())).rejects.toBe(err);

    const s = useAssetStore.getState();
    expect(s.error).toBe("duplicate code");
    expect(s.saving).toBe(false);
    expect(mockListAssets).not.toHaveBeenCalled();
  });

  it("rethrows a link failure after the asset was created (list not reloaded)", async () => {
    mockCreateAsset.mockResolvedValueOnce(makeAsset({ id: 20 }));
    mockLinkAssetHierarchy.mockRejectedValueOnce("cycle detected");

    await expect(
      useAssetStore.getState().createAsset(makeCreatePayload(), { parentAssetId: 3 }),
    ).rejects.toBe("cycle detected");

    expect(useAssetStore.getState().error).toBe("cycle detected");
    expect(useAssetStore.getState().selectedAsset).toBeNull();
    expect(mockListAssets).not.toHaveBeenCalled();
  });
});

// ── updateAsset / moveAssetOrgNode ────────────────────────────────────────────

describe("updateAsset", () => {
  it("updates identity with the row version, reloads list and selects the result", async () => {
    const updated = makeAsset({ asset_name: "Renamed", row_version: 2 });
    mockUpdateAssetIdentity.mockResolvedValueOnce(updated);
    mockListAssets.mockResolvedValueOnce([updated]);
    const payload: UpdateAssetIdentityPayload = { asset_name: "Renamed" };

    const result = await useAssetStore.getState().updateAsset(1, payload, 1);

    expect(result).toEqual(updated);
    expect(mockUpdateAssetIdentity).toHaveBeenCalledWith(1, payload, 1);
    expect(useAssetStore.getState().list).toEqual([updated]);
    expect(useAssetStore.getState().selectedAsset).toEqual(updated);
    expect(useAssetStore.getState().saving).toBe(false);
  });

  it("sets error and rethrows on a version conflict", async () => {
    const err = { code: "VERSION_CONFLICT", message: "stale row" };
    mockUpdateAssetIdentity.mockRejectedValueOnce(err);

    await expect(useAssetStore.getState().updateAsset(1, {}, 1)).rejects.toBe(err);
    expect(useAssetStore.getState().error).toBe("stale row");
    expect(useAssetStore.getState().saving).toBe(false);
  });
});

describe("moveAssetOrgNode", () => {
  it("moves, reloads the list and selects the moved asset", async () => {
    const moved = makeAsset({ org_node_id: 8, row_version: 3 });
    const d = deferred<Asset>();
    mockMoveAssetOrgNode.mockReturnValueOnce(d.promise);
    mockListAssets.mockResolvedValueOnce([moved]);

    const p = useAssetStore.getState().moveAssetOrgNode(1, 8, 2);
    expect(useAssetStore.getState().saving).toBe(true);
    d.resolve(moved);
    await expect(p).resolves.toEqual(moved);

    expect(mockMoveAssetOrgNode).toHaveBeenCalledWith(1, 8, 2);
    expect(useAssetStore.getState().selectedAsset).toEqual(moved);
    expect(useAssetStore.getState().list).toEqual([moved]);
    expect(useAssetStore.getState().saving).toBe(false);
  });

  it("sets error and rethrows on failure", async () => {
    mockMoveAssetOrgNode.mockRejectedValueOnce(new Error("forbidden"));
    await expect(useAssetStore.getState().moveAssetOrgNode(1, 8, 2)).rejects.toThrow("forbidden");
    expect(useAssetStore.getState().error).toBe("forbidden");
    expect(useAssetStore.getState().saving).toBe(false);
  });
});

// ── linkChild / unlinkChild ───────────────────────────────────────────────────

describe("linkChild", () => {
  it("links and reloads the hierarchy of the selected asset", async () => {
    useAssetStore.setState({ selectedAsset: makeAsset({ id: 1 }) });
    const link = makeRelation();
    const rows = [link];
    mockLinkAssetHierarchy.mockResolvedValueOnce(link);
    mockListAssetChildren.mockResolvedValueOnce(rows);
    const payload = { parent_asset_id: 1, child_asset_id: 2, relation_type: "PARENT_CHILD" };

    const result = await useAssetStore.getState().linkChild(payload);

    expect(result).toEqual(link);
    expect(mockLinkAssetHierarchy).toHaveBeenCalledWith(payload);
    expect(mockListAssetChildren).toHaveBeenCalledWith(1);
    expect(useAssetStore.getState().hierarchy).toEqual(rows);
    expect(useAssetStore.getState().saving).toBe(false);
  });

  it("does not reload the hierarchy when nothing is selected", async () => {
    mockLinkAssetHierarchy.mockResolvedValueOnce(makeRelation());

    await useAssetStore
      .getState()
      .linkChild({ parent_asset_id: 1, child_asset_id: 2, relation_type: "PARENT_CHILD" });

    expect(mockListAssetChildren).not.toHaveBeenCalled();
    expect(useAssetStore.getState().hierarchy).toEqual([]);
  });

  it("sets error and rethrows on failure", async () => {
    mockLinkAssetHierarchy.mockRejectedValueOnce(new Error("cycle"));
    await expect(
      useAssetStore
        .getState()
        .linkChild({ parent_asset_id: 1, child_asset_id: 1, relation_type: "PARENT_CHILD" }),
    ).rejects.toThrow("cycle");
    expect(useAssetStore.getState().error).toBe("cycle");
    expect(useAssetStore.getState().saving).toBe(false);
  });
});

describe("unlinkChild", () => {
  it("unlinks with the effective date and reloads the selected asset's hierarchy", async () => {
    useAssetStore.setState({ selectedAsset: makeAsset({ id: 1 }), hierarchy: [makeRelation()] });
    const unlinked = makeRelation({ effective_to: "2026-06-01" });
    mockUnlinkAssetHierarchy.mockResolvedValueOnce(unlinked);
    mockListAssetChildren.mockResolvedValueOnce([]);

    const result = await useAssetStore.getState().unlinkChild(100, "2026-06-01");

    expect(result).toEqual(unlinked);
    expect(mockUnlinkAssetHierarchy).toHaveBeenCalledWith(100, "2026-06-01");
    expect(useAssetStore.getState().hierarchy).toEqual([]);
  });

  it("does not reload when nothing is selected", async () => {
    mockUnlinkAssetHierarchy.mockResolvedValueOnce(makeRelation());
    await useAssetStore.getState().unlinkChild(100);
    expect(mockUnlinkAssetHierarchy).toHaveBeenCalledWith(100, undefined);
    expect(mockListAssetChildren).not.toHaveBeenCalled();
  });

  it("sets error and rethrows on failure", async () => {
    mockUnlinkAssetHierarchy.mockRejectedValueOnce(new Error("gone"));
    await expect(useAssetStore.getState().unlinkChild(100)).rejects.toThrow("gone");
    expect(useAssetStore.getState().error).toBe("gone");
    expect(useAssetStore.getState().saving).toBe(false);
  });
});

// ── Form actions ──────────────────────────────────────────────────────────────

describe("form actions", () => {
  it("openCreateForm opens create, closes edit and stores the parent", () => {
    const parent = makeAsset({ id: 4 });
    useAssetStore.setState({ showEditForm: true, editingAsset: makeAsset() });

    useAssetStore.getState().openCreateForm(parent);

    const s = useAssetStore.getState();
    expect(s.showCreateForm).toBe(true);
    expect(s.showEditForm).toBe(false);
    expect(s.editingAsset).toBeNull();
    expect(s.parentPreFill).toEqual(parent);
  });

  it("openCreateForm without a parent sets parentPreFill to null", () => {
    useAssetStore.setState({ parentPreFill: makeAsset() });
    useAssetStore.getState().openCreateForm();
    expect(useAssetStore.getState().parentPreFill).toBeNull();
  });

  it("closeCreateForm hides the form and clears the parent", () => {
    useAssetStore.getState().openCreateForm(makeAsset());
    useAssetStore.getState().closeCreateForm();
    expect(useAssetStore.getState().showCreateForm).toBe(false);
    expect(useAssetStore.getState().parentPreFill).toBeNull();
  });

  it("openEditForm opens edit with the asset and closes create", () => {
    const asset = makeAsset({ id: 3 });
    useAssetStore.getState().openCreateForm();
    useAssetStore.getState().openEditForm(asset);
    const s = useAssetStore.getState();
    expect(s.showEditForm).toBe(true);
    expect(s.showCreateForm).toBe(false);
    expect(s.editingAsset).toEqual(asset);
  });

  it("closeEditForm hides edit and clears the editing asset", () => {
    useAssetStore.getState().openEditForm(makeAsset());
    useAssetStore.getState().closeEditForm();
    expect(useAssetStore.getState().showEditForm).toBe(false);
    expect(useAssetStore.getState().editingAsset).toBeNull();
  });
});

// ── Tree actions ──────────────────────────────────────────────────────────────

describe("tree actions", () => {
  it("setTreeMode(true) loads roots when none are loaded", async () => {
    const roots = [makeAsset()];
    mockListAssets.mockResolvedValueOnce(roots);

    useAssetStore.getState().setTreeMode(true);
    expect(useAssetStore.getState().treeMode).toBe(true);
    expect(useAssetStore.getState().treeLoading).toBe(true);

    await vi.waitFor(() => expect(useAssetStore.getState().treeLoading).toBe(false));
    expect(mockListAssets).toHaveBeenCalledWith(null, null, null);
    expect(useAssetStore.getState().treeRoots).toEqual(roots);
  });

  it("setTreeMode(true) does not reload when roots exist", () => {
    useAssetStore.setState({ treeRoots: [makeAsset()] });
    useAssetStore.getState().setTreeMode(true);
    expect(mockListAssets).not.toHaveBeenCalled();
  });

  it("setTreeMode(false) only toggles the flag", () => {
    useAssetStore.setState({ treeMode: true });
    useAssetStore.getState().setTreeMode(false);
    expect(useAssetStore.getState().treeMode).toBe(false);
    expect(mockListAssets).not.toHaveBeenCalled();
  });

  it("loadTreeRoots records the error and clears treeLoading on failure", async () => {
    mockListAssets.mockRejectedValueOnce(new Error("offline"));
    await useAssetStore.getState().loadTreeRoots();
    expect(useAssetStore.getState().error).toBe("offline");
    expect(useAssetStore.getState().treeLoading).toBe(false);
    expect(useAssetStore.getState().treeRoots).toEqual([]);
  });

  it("loadTreeChildren resolves each child row to a full asset and caches by parent", async () => {
    const existing = new Map<number, Asset[]>([[99, [makeAsset({ id: 98 })]]]);
    useAssetStore.setState({ treeChildren: existing });
    mockListAssetChildren.mockResolvedValueOnce([
      makeRelation({ relation_id: 1, child_asset_id: 2 }),
      makeRelation({ relation_id: 2, child_asset_id: 3 }),
    ]);
    const child2 = makeAsset({ id: 2 });
    const child3 = makeAsset({ id: 3 });
    mockGetAssetById.mockImplementation((id) => Promise.resolve(id === 2 ? child2 : child3));

    await useAssetStore.getState().loadTreeChildren(1);

    expect(mockListAssetChildren).toHaveBeenCalledWith(1);
    expect(mockGetAssetById.mock.calls).toEqual([[2], [3]]);
    const map = useAssetStore.getState().treeChildren;
    expect(map).not.toBe(existing);
    expect(map.get(1)).toEqual([child2, child3]);
    expect(map.get(99)).toEqual([makeAsset({ id: 98 })]);
  });

  it("loadTreeChildren swallows failures into error", async () => {
    mockListAssetChildren.mockRejectedValueOnce(new Error("boom"));
    await expect(useAssetStore.getState().loadTreeChildren(1)).resolves.toBeUndefined();
    expect(useAssetStore.getState().error).toBe("boom");
    expect(useAssetStore.getState().treeChildren.has(1)).toBe(false);
  });

  it("toggleTreeExpand expands and lazy-loads children the first time", async () => {
    mockListAssetChildren.mockResolvedValueOnce([]);

    useAssetStore.getState().toggleTreeExpand(5);

    expect(useAssetStore.getState().treeExpandedIds.has(5)).toBe(true);
    expect(mockListAssetChildren).toHaveBeenCalledWith(5);
    await vi.waitFor(() => expect(useAssetStore.getState().treeChildren.get(5)).toEqual([]));
  });

  it("toggleTreeExpand does not reload children already cached", () => {
    useAssetStore.setState({ treeChildren: new Map([[5, [makeAsset()]]]) });
    useAssetStore.getState().toggleTreeExpand(5);
    expect(useAssetStore.getState().treeExpandedIds.has(5)).toBe(true);
    expect(mockListAssetChildren).not.toHaveBeenCalled();
  });

  it("toggleTreeExpand collapses an expanded node with a new Set instance", () => {
    const before = new Set([5, 6]);
    useAssetStore.setState({ treeExpandedIds: before });
    useAssetStore.getState().toggleTreeExpand(5);
    const after = useAssetStore.getState().treeExpandedIds;
    expect(after).not.toBe(before);
    expect([...after]).toEqual([6]);
    expect(mockListAssetChildren).not.toHaveBeenCalled();
  });

  it("selectTreeNode sets and clears the selected id", () => {
    useAssetStore.getState().selectTreeNode(7);
    expect(useAssetStore.getState().treeSelectedId).toBe(7);
    useAssetStore.getState().selectTreeNode(null);
    expect(useAssetStore.getState().treeSelectedId).toBeNull();
  });
});
