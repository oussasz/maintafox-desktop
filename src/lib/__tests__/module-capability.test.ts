import { describe, expect, it } from "vitest";

import {
  isModuleCapabilityAllowed,
  moduleCapabilityFromPermission,
  parseCapabilityMap,
} from "@/lib/module-capability";
import { P } from "@shared/rbac/permissions.generated";

describe("module-capability", () => {
  it("maps view and manage permissions to the same module", () => {
    expect(moduleCapabilityFromPermission(P.INV_VIEW)).toBe("inventory");
    expect(moduleCapabilityFromPermission(P.INV_MANAGE)).toBe("inventory");
    expect(moduleCapabilityFromPermission(P.PLAN_VIEW)).toBe("planning");
    expect(moduleCapabilityFromPermission(P.ADM_USERS)).toBeNull();
  });

  it("allows all modules when capability map is empty (soft phase)", () => {
    expect(isModuleCapabilityAllowed({}, P.PLAN_VIEW)).toBe(true);
    expect(isModuleCapabilityAllowed(parseCapabilityMap("{}"), P.PM_VIEW)).toBe(true);
  });

  it("hides modules explicitly set to false", () => {
    const map = parseCapabilityMap(JSON.stringify({ equipment: true, planning: false, pm: false }));
    expect(isModuleCapabilityAllowed(map, P.EQ_VIEW)).toBe(true);
    expect(isModuleCapabilityAllowed(map, P.PLAN_VIEW)).toBe(false);
    expect(isModuleCapabilityAllowed(map, P.PM_EDIT)).toBe(false);
  });
});
