import { describe, expect, it } from "vitest";

import { resolveReferenceValueColumns } from "@/components/lookups/reference-value-columns";
import type { ReferenceDomain } from "@shared/ipc-types";

function domain(
  partial: Partial<ReferenceDomain> & Pick<ReferenceDomain, "code" | "structure_type">,
): ReferenceDomain {
  return {
    id: 1,
    name: "Test",
    governance_level: "tenant_managed",
    governance_category: "operational_dictionary",
    is_extendable: true,
    validation_rules_json: null,
    created_at: "",
    updated_at: "",
    ...partial,
  };
}

describe("resolveReferenceValueColumns", () => {
  it("hides parent and shows schedule for ORG.SCHEDULE_CLASS", () => {
    const flags = resolveReferenceValueColumns(
      domain({ code: "ORG.SCHEDULE_CLASS", structure_type: "flat" }),
    );
    expect(flags.showParent).toBe(false);
    expect(flags.showSchedule).toBe(true);
    expect(flags.crossDomainParentCode).toBeNull();
  });

  it("shows cross-domain parent for EQUIPMENT.FAMILY", () => {
    const flags = resolveReferenceValueColumns(
      domain({ code: "EQUIPMENT.FAMILY", structure_type: "hierarchical" }),
    );
    expect(flags.showParent).toBe(true);
    expect(flags.crossDomainParentCode).toBe("EQUIPMENT.CLASS");
    expect(flags.sameSetHierarchy).toBe(false);
    expect(flags.showSchedule).toBe(false);
  });

  it("uses same-set hierarchy for hierarchical domains without registry parent", () => {
    const flags = resolveReferenceValueColumns(
      domain({ code: "PERSONNEL.SKILLS", structure_type: "hierarchical" }),
    );
    expect(flags.showParent).toBe(true);
    expect(flags.sameSetHierarchy).toBe(true);
    expect(flags.crossDomainParentCode).toBeNull();
  });

  it("hides parent for flat domains without registry parent", () => {
    const flags = resolveReferenceValueColumns(
      domain({ code: "EQUIPMENT.CRITICALITY", structure_type: "flat" }),
    );
    expect(flags.showParent).toBe(false);
    expect(flags.showSchedule).toBe(false);
  });
});
