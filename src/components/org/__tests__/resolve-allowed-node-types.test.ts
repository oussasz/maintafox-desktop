import { describe, expect, it } from "vitest";

import { resolveAllowedNodeTypes } from "@/components/org/resolve-allowed-node-types";
import type { OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";

function type(partial: Partial<OrgNodeType> & Pick<OrgNodeType, "id" | "code">): OrgNodeType {
  return {
    sync_id: `s-${partial.id}`,
    structure_model_id: partial.structure_model_id ?? 2,
    label: partial.label ?? partial.code,
    icon_key: null,
    color: null,
    depth_hint: null,
    is_root_type: false,
    can_host_assets: false,
    can_own_work: false,
    can_carry_cost_center: false,
    can_aggregate_kpis: false,
    can_receive_permits: false,
    is_active: true,
    created_at: "",
    updated_at: "",
    ...partial,
  };
}

function rule(
  partial: Pick<OrgRelationshipRule, "id" | "parent_type_id" | "child_type_id"> &
    Partial<OrgRelationshipRule>,
): OrgRelationshipRule {
  return {
    structure_model_id: 2,
    min_children: null,
    max_children: null,
    created_at: "",
    ...partial,
  };
}

describe("resolveAllowedNodeTypes", () => {
  it("uses same-model parentTypeId for draft tree parents", () => {
    const draftTypes = [
      type({ id: 100, code: "ORG_ROOT", is_root_type: true }),
      type({ id: 101, code: "DEPARTEMENT" }),
      type({ id: 102, code: "ATELIER" }),
    ];
    const draftRules = [
      rule({ id: 1, parent_type_id: 100, child_type_id: 101 }),
      rule({ id: 2, parent_type_id: 101, child_type_id: 102 }),
    ];

    const allowed = resolveAllowedNodeTypes({
      mode: "child",
      types: draftTypes,
      rules: draftRules,
      parentTypeId: 100,
    });

    expect(allowed.map((t) => t.code)).toEqual(["DEPARTEMENT"]);
  });

  it("falls back to parentTypeCode when id not provided", () => {
    const draftTypes = [
      type({ id: 100, code: "ORG_ROOT", is_root_type: true }),
      type({ id: 101, code: "DEPARTEMENT" }),
    ];
    const draftRules = [rule({ id: 1, parent_type_id: 100, child_type_id: 101 })];

    expect(
      resolveAllowedNodeTypes({
        mode: "child",
        types: draftTypes,
        rules: draftRules,
        parentTypeCode: "ORG_ROOT",
      }).map((t) => t.code),
    ).toEqual(["DEPARTEMENT"]);
  });

  it("lists root types for root mode", () => {
    const types = [
      type({ id: 1, code: "ORG_ROOT", is_root_type: true }),
      type({ id: 2, code: "DEPT" }),
    ];
    expect(
      resolveAllowedNodeTypes({
        mode: "root",
        types,
        rules: [],
      }).map((t) => t.code),
    ).toEqual(["ORG_ROOT"]);
  });
});
