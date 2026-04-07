/**
 * Supervisor Verification — Sprint S3
 *
 * V1 — Permission split: org.view-only user can list but not create;
 *       org.manage user can create nodes and assign responsibilities.
 * V2 — Tree reload after mutation: loadTree() returns hierarchy-ordered
 *       rows; refreshSelectedNodeContext() reloads after rename.
 * V3 — Version conflict path: stale row-version surfaces as
 *       VersionConflictError and leaves the store recoverable.
 */

import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { useOrgNodeStore } from "../org-node-store";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const rootNode = {
  id: 1,
  sync_id: "aaa-111",
  code: "SITE-001",
  name: "Usine Principale",
  node_type_id: 1,
  parent_id: null,
  ancestor_path: "/1/",
  depth: 0,
  description: null,
  cost_center_code: null,
  external_reference: null,
  status: "active",
  effective_from: null,
  effective_to: null,
  erp_reference: null,
  notes: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  deleted_at: null,
  row_version: 1,
  origin_machine_id: null,
  last_synced_checkpoint: null,
};

const childNode = {
  ...rootNode,
  id: 2,
  sync_id: "bbb-222",
  code: "WS-001",
  name: "Atelier Mécanique",
  node_type_id: 2,
  parent_id: 1,
  ancestor_path: "/1/2/",
  depth: 1,
};

const rootTreeRow = {
  node: rootNode,
  node_type_code: "SITE",
  node_type_label: "Site",
  can_host_assets: true,
  can_own_work: true,
  can_carry_cost_center: true,
  can_aggregate_kpis: true,
  can_receive_permits: false,
  child_count: 1,
};

const childTreeRow = {
  node: childNode,
  node_type_code: "WORKSHOP",
  node_type_label: "Atelier",
  can_host_assets: true,
  can_own_work: true,
  can_carry_cost_center: false,
  can_aggregate_kpis: false,
  can_receive_permits: false,
  child_count: 0,
};

const permissionDeniedError = {
  code: "PERMISSION_DENIED",
  message: "Permission denied: action 'org.manage' on resource 'global'",
};

const versionConflictError = {
  code: "VALIDATION_FAILED",
  message: 'Validation failed: ["row version mismatch: expected 1, actual 2"]',
};

function resetStore() {
  useOrgNodeStore.setState({
    treeRows: [],
    selectedNodeId: null,
    selectedNode: null,
    responsibilities: [],
    bindings: [],
    loading: false,
    saving: false,
    error: null,
  });
}

// ── V1 — Permission split ────────────────────────────────────────────────────

describe("V1 — Permission split", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("org.view user: list_org_tree succeeds", async () => {
    mockInvoke.mockResolvedValueOnce([rootTreeRow]);

    await useOrgNodeStore.getState().loadTree();

    expect(mockInvoke).toHaveBeenCalledWith("list_org_tree");
    const state = useOrgNodeStore.getState();
    expect(state.treeRows).toHaveLength(1);
    expect(state.error).toBeNull();
  });

  it("org.view user: create_org_node is rejected with PERMISSION_DENIED", async () => {
    // listOrgTree succeeds (view permission)
    mockInvoke.mockResolvedValueOnce([]);
    await useOrgNodeStore.getState().loadTree();
    expect(useOrgNodeStore.getState().error).toBeNull();

    // createOrgNode rejected (manage permission)
    mockInvoke.mockRejectedValueOnce(permissionDeniedError);

    const { createOrgNode } = await import("@/services/org-node-service");
    await expect(
      createOrgNode({ code: "SITE-001", name: "Test", node_type_id: 1, parent_id: null }),
    ).rejects.toEqual(permissionDeniedError);
  });

  it("org.manage user: create_org_node succeeds", async () => {
    mockInvoke.mockResolvedValueOnce(rootNode);

    const { createOrgNode } = await import("@/services/org-node-service");
    const result = await createOrgNode({
      code: "SITE-001",
      name: "Usine Principale",
      node_type_id: 1,
      parent_id: null,
    });

    expect(result.id).toBe(1);
    expect(result.code).toBe("SITE-001");
  });

  it("org.manage user: assign_org_node_responsibility succeeds", async () => {
    const responsibility = {
      id: 1,
      node_id: 1,
      responsibility_type: "maintenance_owner",
      person_id: 1,
      team_id: null,
      valid_from: null,
      valid_to: null,
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    };
    mockInvoke.mockResolvedValueOnce(responsibility);

    const { assignOrgNodeResponsibility } = await import("@/services/org-node-service");
    const result = await assignOrgNodeResponsibility({
      node_id: 1,
      responsibility_type: "maintenance_owner",
      person_id: 1,
    });

    expect(result.responsibility_type).toBe("maintenance_owner");
    expect(result.person_id).toBe(1);
  });
});

// ── V2 — Tree reload after mutation ──────────────────────────────────────────

describe("V2 — Tree reload after mutation", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("loadTree() returns two rows in ancestor_path order", async () => {
    // Rows are returned by Rust sorted by ancestor_path ASC
    mockInvoke.mockResolvedValueOnce([rootTreeRow, childTreeRow]);

    await useOrgNodeStore.getState().loadTree();

    const state = useOrgNodeStore.getState();
    expect(state.treeRows).toHaveLength(2);
    // Root comes first (ancestor_path: /1/)
    expect(state.treeRows[0]?.node.id).toBe(1);
    expect(state.treeRows[0]?.node.depth).toBe(0);
    // Child comes second (ancestor_path: /1/2/)
    expect(state.treeRows[1]?.node.id).toBe(2);
    expect(state.treeRows[1]?.node.depth).toBe(1);
    expect(state.treeRows[1]?.node.parent_id).toBe(1);
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it("selectNode() loads details, responsibilities, and bindings in parallel", async () => {
    const binding = {
      id: 1,
      node_id: 2,
      binding_type: "site_reference",
      external_system: "erp",
      external_id: "PLANT-100",
      is_primary: true,
      valid_from: null,
      valid_to: null,
      created_at: "2026-01-01T00:00:00Z",
    };

    // getOrgNode, listOrgNodeResponsibilities, listOrgEntityBindings — in parallel
    mockInvoke
      .mockResolvedValueOnce(childNode) // get_org_node
      .mockResolvedValueOnce([]) // list_org_node_responsibilities
      .mockResolvedValueOnce([binding]); // list_org_entity_bindings

    await useOrgNodeStore.getState().selectNode(2);

    const state = useOrgNodeStore.getState();
    expect(state.selectedNodeId).toBe(2);
    expect(state.selectedNode?.code).toBe("WS-001");
    expect(state.responsibilities).toHaveLength(0);
    expect(state.bindings).toHaveLength(1);
    expect(state.bindings[0]?.is_primary).toBe(true);
  });

  it("refreshSelectedNodeContext() reloads tree + node context after rename", async () => {
    // First, select the child node
    mockInvoke
      .mockResolvedValueOnce(childNode) // get_org_node
      .mockResolvedValueOnce([]) // responsibilities
      .mockResolvedValueOnce([]); // bindings
    await useOrgNodeStore.getState().selectNode(2);

    expect(useOrgNodeStore.getState().selectedNode?.name).toBe("Atelier Mécanique");

    // Simulate rename: the child now has a different name and row_version bumped
    const renamedChild = { ...childNode, name: "Atelier Électrique", row_version: 2 };
    const renamedChildTreeRow = { ...childTreeRow, node: renamedChild };

    // refreshSelectedNodeContext calls 4 things in parallel
    mockInvoke
      .mockResolvedValueOnce([rootTreeRow, renamedChildTreeRow]) // list_org_tree
      .mockResolvedValueOnce(renamedChild) // get_org_node
      .mockResolvedValueOnce([]) // responsibilities
      .mockResolvedValueOnce([]); // bindings

    await useOrgNodeStore.getState().refreshSelectedNodeContext();

    const state = useOrgNodeStore.getState();
    // Tree reflects the rename
    expect(state.treeRows[1]?.node.name).toBe("Atelier Électrique");
    // Selected node context reflects the rename
    expect(state.selectedNode?.name).toBe("Atelier Électrique");
    expect(state.selectedNode?.row_version).toBe(2);
    expect(state.loading).toBe(false);
    expect(state.error).toBeNull();
  });

  it("selectNode(null) clears all selected context", async () => {
    // Pre-populate
    useOrgNodeStore.setState({
      selectedNodeId: 2,
      selectedNode: childNode,
      responsibilities: [],
      bindings: [],
    });

    await useOrgNodeStore.getState().selectNode(null);

    const state = useOrgNodeStore.getState();
    expect(state.selectedNodeId).toBeNull();
    expect(state.selectedNode).toBeNull();
    expect(state.responsibilities).toHaveLength(0);
    expect(state.bindings).toHaveLength(0);
  });
});

// ── V3 — Version conflict path ───────────────────────────────────────────────

describe("V3 — Version conflict path", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("stale updateOrgNodeMetadata surfaces error in the store, store remains recoverable", async () => {
    // Load tree with one root
    mockInvoke.mockResolvedValueOnce([rootTreeRow]);
    await useOrgNodeStore.getState().loadTree();

    // Select the node
    mockInvoke
      .mockResolvedValueOnce(rootNode) // get_org_node
      .mockResolvedValueOnce([]) // responsibilities
      .mockResolvedValueOnce([]); // bindings
    await useOrgNodeStore.getState().selectNode(1);

    expect(useOrgNodeStore.getState().selectedNode?.row_version).toBe(1);

    // Simulate: another session updated the node (row_version is now 2)
    // Our stale update with expected_row_version=1 is rejected
    const { updateOrgNodeMetadata } = await import("@/services/org-node-service");
    mockInvoke.mockRejectedValueOnce(versionConflictError);

    // updateOrgNodeMetadata propagates the raw IPC error (unlike move/deactivate
    // which wrap it in VersionConflictError). The caller catches and sets store error.
    await expect(
      updateOrgNodeMetadata({
        node_id: 1,
        name: "New Name",
        expected_row_version: 1,
      }),
    ).rejects.toEqual(versionConflictError);

    // Store state is still intact — tree and selection remain
    const state = useOrgNodeStore.getState();
    expect(state.treeRows).toHaveLength(1);
    expect(state.selectedNodeId).toBe(1);
    expect(state.selectedNode).not.toBeNull();
  });

  it("after conflict, store can recover by refreshing", async () => {
    // Pre-populate store with stale data
    useOrgNodeStore.setState({
      treeRows: [rootTreeRow],
      selectedNodeId: 1,
      selectedNode: rootNode,
      responsibilities: [],
      bindings: [],
    });

    // The node was updated externally — now row_version=2 and name changed
    const updatedNode = { ...rootNode, name: "Renamed Externally", row_version: 2 };
    const updatedTreeRow = { ...rootTreeRow, node: updatedNode };

    // Refresh after conflict
    mockInvoke
      .mockResolvedValueOnce([updatedTreeRow]) // list_org_tree
      .mockResolvedValueOnce(updatedNode) // get_org_node
      .mockResolvedValueOnce([]) // responsibilities
      .mockResolvedValueOnce([]); // bindings

    await useOrgNodeStore.getState().refreshSelectedNodeContext();

    const state = useOrgNodeStore.getState();
    expect(state.selectedNode?.row_version).toBe(2);
    expect(state.selectedNode?.name).toBe("Renamed Externally");
    expect(state.treeRows[0]?.node.row_version).toBe(2);
    expect(state.error).toBeNull();
    expect(state.loading).toBe(false);
  });

  it("moveOrgNode rejects stale version with VersionConflictError", async () => {
    mockInvoke.mockRejectedValueOnce(versionConflictError);

    const { moveOrgNode, VersionConflictError } = await import("@/services/org-node-service");
    await expect(
      moveOrgNode({ node_id: 1, new_parent_id: 2, expected_row_version: 1 }),
    ).rejects.toThrow(VersionConflictError);
  });

  it("deactivateOrgNode rejects stale version with VersionConflictError", async () => {
    mockInvoke.mockRejectedValueOnce(versionConflictError);

    const { deactivateOrgNode, VersionConflictError } = await import("@/services/org-node-service");
    await expect(deactivateOrgNode(1, 1)).rejects.toThrow(VersionConflictError);
  });
});
