import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  listOrgTree,
  getOrgNode,
  createOrgNode,
  moveOrgNode,
  deactivateOrgNode,
  listOrgNodeResponsibilities,
  assignOrgNodeResponsibility,
  listOrgEntityBindings,
  upsertOrgEntityBinding,
  OrgTreeRowSchema,
  OrgNodeSchema,
  VersionConflictError,
} from "../org-node-service";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const baseNode = {
  id: 1,
  sync_id: "aaa-bbb-ccc",
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

const baseTreeRow = {
  node: baseNode,
  node_type_code: "SITE",
  node_type_label: "Site",
  can_host_assets: true,
  can_own_work: true,
  can_carry_cost_center: true,
  can_aggregate_kpis: true,
  can_receive_permits: false,
  child_count: 0,
};

const baseResponsibility = {
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

const baseBinding = {
  id: 1,
  node_id: 1,
  binding_type: "site_reference",
  external_system: "erp",
  external_id: "PLANT-100",
  is_primary: true,
  valid_from: null,
  valid_to: null,
  created_at: "2026-01-01T00:00:00Z",
};

// ── listOrgTree ───────────────────────────────────────────────────────────────

describe("listOrgTree", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("returns an empty array on a fresh tenant", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    const result = await listOrgTree();
    expect(result).toEqual([]);
    expect(mockInvoke).toHaveBeenCalledWith("list_org_tree");
  });

  it("returns validated tree rows", async () => {
    mockInvoke.mockResolvedValueOnce([baseTreeRow]);
    const result = await listOrgTree();
    expect(result).toHaveLength(1);
    expect(result[0]?.node.code).toBe("SITE-001");
  });

  it("throws a ZodError when Rust returns a malformed response", async () => {
    mockInvoke.mockResolvedValueOnce([{ node: { id: "not-a-number" } }]);
    await expect(listOrgTree()).rejects.toThrow();
  });
});

// ── getOrgNode ────────────────────────────────────────────────────────────────

describe("getOrgNode", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls get_org_node with the correct nodeId", async () => {
    mockInvoke.mockResolvedValueOnce(baseNode);
    await getOrgNode(1);
    expect(mockInvoke).toHaveBeenCalledWith("get_org_node", { nodeId: 1 });
  });
});

// ── createOrgNode ─────────────────────────────────────────────────────────────

describe("createOrgNode", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls create_org_node with the payload", async () => {
    mockInvoke.mockResolvedValueOnce(baseNode);
    const payload = { code: "SITE-001", name: "Usine", node_type_id: 1, parent_id: null };
    await createOrgNode(payload);
    expect(mockInvoke).toHaveBeenCalledWith("create_org_node", { payload });
  });
});

// ── moveOrgNode — version conflict ────────────────────────────────────────────

describe("moveOrgNode", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("throws VersionConflictError on stale row version", async () => {
    mockInvoke.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: 'Validation failed: ["row version mismatch: expected 1, actual 2"]',
    });
    const payload = { node_id: 1, new_parent_id: 2, expected_row_version: 1 };
    await expect(moveOrgNode(payload)).rejects.toThrow(VersionConflictError);
  });

  it("re-throws non-version errors as-is", async () => {
    mockInvoke.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: 'Validation failed: ["cannot move a node under its own descendant"]',
    });
    const payload = { node_id: 1, new_parent_id: 2, expected_row_version: 1 };
    await expect(moveOrgNode(payload)).rejects.not.toThrow(VersionConflictError);
  });
});

// ── deactivateOrgNode — version conflict ──────────────────────────────────────

describe("deactivateOrgNode", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("throws VersionConflictError on stale row version", async () => {
    mockInvoke.mockRejectedValueOnce({
      code: "VALIDATION_FAILED",
      message: 'Validation failed: ["row version mismatch: expected 1, actual 3"]',
    });
    await expect(deactivateOrgNode(1, 1)).rejects.toThrow(VersionConflictError);
  });

  it("returns validated node on success", async () => {
    mockInvoke.mockResolvedValueOnce({ ...baseNode, status: "inactive", row_version: 2 });
    const result = await deactivateOrgNode(1, 1);
    expect(result.status).toBe("inactive");
  });
});

// ── Responsibility commands ───────────────────────────────────────────────────

describe("listOrgNodeResponsibilities", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("passes includeInactive flag", async () => {
    mockInvoke.mockResolvedValueOnce([baseResponsibility]);
    await listOrgNodeResponsibilities(1, true);
    expect(mockInvoke).toHaveBeenCalledWith("list_org_node_responsibilities", {
      nodeId: 1,
      includeInactive: true,
    });
  });
});

describe("assignOrgNodeResponsibility", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("returns validated responsibility", async () => {
    mockInvoke.mockResolvedValueOnce(baseResponsibility);
    const result = await assignOrgNodeResponsibility({
      node_id: 1,
      responsibility_type: "maintenance_owner",
      person_id: 1,
    });
    expect(result.responsibility_type).toBe("maintenance_owner");
  });
});

// ── Entity binding commands ───────────────────────────────────────────────────

describe("listOrgEntityBindings", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("returns validated bindings array", async () => {
    mockInvoke.mockResolvedValueOnce([baseBinding]);
    const result = await listOrgEntityBindings(1);
    expect(result).toHaveLength(1);
    expect(result[0]?.is_primary).toBe(true);
  });
});

describe("upsertOrgEntityBinding", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("calls upsert_org_entity_binding with the payload", async () => {
    mockInvoke.mockResolvedValueOnce(baseBinding);
    const payload = {
      node_id: 1,
      binding_type: "site_reference",
      external_system: "erp",
      external_id: "PLANT-100",
      is_primary: true,
    };
    await upsertOrgEntityBinding(payload);
    expect(mockInvoke).toHaveBeenCalledWith("upsert_org_entity_binding", { payload });
  });
});

// ── Zod schema exports ────────────────────────────────────────────────────────

describe("Zod schema exports", () => {
  it("OrgNodeSchema validates a complete node", () => {
    expect(OrgNodeSchema.safeParse(baseNode).success).toBe(true);
  });

  it("OrgTreeRowSchema validates a tree row", () => {
    expect(OrgTreeRowSchema.safeParse(baseTreeRow).success).toBe(true);
  });

  it("OrgNodeSchema rejects missing fields", () => {
    expect(OrgNodeSchema.safeParse({ id: 1 }).success).toBe(false);
  });
});
