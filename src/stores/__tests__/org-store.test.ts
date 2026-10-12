/**
 * Behaviour tests for org-store (active org structure model configuration).
 * Only the org-service boundary is mocked.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getActiveOrgStructureModel,
  listOrgNodeTypes,
  listOrgRelationshipRules,
} from "@/services/org-service";
import type { OrgNodeType, OrgRelationshipRule, OrgStructureModel } from "@shared/ipc-types";

import { useOrgStore } from "../org-store";

vi.mock("@/services/org-service", () => ({
  getActiveOrgStructureModel: vi.fn(),
  listOrgNodeTypes: vi.fn(),
  listOrgRelationshipRules: vi.fn(),
}));

const mockGetModel = vi.mocked(getActiveOrgStructureModel);
const mockListTypes = vi.mocked(listOrgNodeTypes);
const mockListRules = vi.mocked(listOrgRelationshipRules);

function makeModel(overrides: Partial<OrgStructureModel> = {}): OrgStructureModel {
  return {
    id: 3,
    entity_sync_id: "model-3",
    version_number: 2,
    status: "active",
    description: "Plant structure",
    activated_at: "2026-01-01T00:00:00Z",
    activated_by_id: 1,
    superseded_at: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeNodeType(overrides: Partial<OrgNodeType> = {}): OrgNodeType {
  return {
    id: 10,
    entity_sync_id: "type-10",
    structure_model_id: 3,
    code: "SITE",
    label: "Site",
    icon_key: null,
    color: null,
    depth_hint: 0,
    can_host_assets: true,
    can_own_work: true,
    can_carry_cost_center: false,
    can_aggregate_kpis: true,
    can_receive_permits: false,
    is_root_type: true,
    is_active: true,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeRule(overrides: Partial<OrgRelationshipRule> = {}): OrgRelationshipRule {
  return {
    id: 20,
    structure_model_id: 3,
    parent_type_id: 10,
    child_type_id: 11,
    min_children: null,
    max_children: null,
    created_at: "2026-01-01T00:00:00Z",
    parent_type_label: "Site",
    child_type_label: "Area",
    ...overrides,
  };
}

const initial = useOrgStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockGetModel.mockReset();
  mockListTypes.mockReset();
  mockListRules.mockReset();
  useOrgStore.setState(initial, true);
});

describe("org-store", () => {
  it("starts with no active model", () => {
    const s = useOrgStore.getState();
    expect(s.activeModel).toBeNull();
    expect(s.nodeTypes).toEqual([]);
    expect(s.relationshipRules).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  describe("loadActiveModelConfig", () => {
    it("loads model, then node types and rules for that model id", async () => {
      const model = makeModel();
      const types = [makeNodeType()];
      const rules = [makeRule()];
      let resolveModel: (m: OrgStructureModel | null) => void = () => undefined;
      mockGetModel.mockReturnValueOnce(
        new Promise<OrgStructureModel | null>((r) => {
          resolveModel = r;
        }),
      );
      mockListTypes.mockResolvedValueOnce(types);
      mockListRules.mockResolvedValueOnce(rules);
      useOrgStore.setState({ error: "old" });

      const p = useOrgStore.getState().loadActiveModelConfig();
      expect(useOrgStore.getState().loading).toBe(true);
      expect(useOrgStore.getState().error).toBeNull();

      resolveModel(model);
      await p;

      expect(mockListTypes).toHaveBeenCalledWith(3);
      expect(mockListRules).toHaveBeenCalledWith(3);
      const s = useOrgStore.getState();
      expect(s.activeModel).toEqual(model);
      expect(s.nodeTypes).toEqual(types);
      expect(s.relationshipRules).toEqual(rules);
      expect(s.loading).toBe(false);
    });

    it("clears model, types and rules when there is no active model", async () => {
      useOrgStore.setState({
        activeModel: makeModel(),
        nodeTypes: [makeNodeType()],
        relationshipRules: [makeRule()],
      });
      mockGetModel.mockResolvedValueOnce(null);

      await useOrgStore.getState().loadActiveModelConfig();

      expect(mockListTypes).not.toHaveBeenCalled();
      expect(mockListRules).not.toHaveBeenCalled();
      const s = useOrgStore.getState();
      expect(s.activeModel).toBeNull();
      expect(s.nodeTypes).toEqual([]);
      expect(s.relationshipRules).toEqual([]);
      expect(s.loading).toBe(false);
    });

    it("swallows errors and strips the 'Validation failed:' prefix", async () => {
      mockGetModel.mockRejectedValueOnce({ message: "Validation failed: model broken" });

      await expect(useOrgStore.getState().loadActiveModelConfig()).resolves.toBeUndefined();

      const s = useOrgStore.getState();
      expect(s.error).toBe("model broken");
      expect(s.loading).toBe(false);
      expect(s.activeModel).toBeNull();
    });

    it("joins uncoded IPC validation detail messages into the error", async () => {
      mockGetModel.mockResolvedValueOnce(makeModel());
      mockListTypes.mockRejectedValueOnce({
        message: "Validation failed",
        details: [{ message: "first issue" }, { message: "second issue" }],
      });
      mockListRules.mockResolvedValueOnce([]);

      await useOrgStore.getState().loadActiveModelConfig();

      const s = useOrgStore.getState();
      expect(s.error).toBe("first issue; second issue");
      // Partial failure leaves the previous (empty) config untouched.
      expect(s.activeModel).toBeNull();
      expect(s.nodeTypes).toEqual([]);
      expect(s.loading).toBe(false);
    });
  });

  it("setActiveModel replaces only the active model", () => {
    const types = [makeNodeType()];
    useOrgStore.setState({ nodeTypes: types });
    const next = makeModel({ id: 4, version_number: 3 });

    useOrgStore.getState().setActiveModel(next);

    const s = useOrgStore.getState();
    expect(s.activeModel).toEqual(next);
    expect(s.nodeTypes).toBe(types);
  });
});
