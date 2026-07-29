import { describe, expect, it } from "vitest";

import {
  isModuleCapabilityAllowed,
  moduleCapabilityFromPermission,
  parseCapabilityMap,
} from "@/lib/module-capability";

describe("module-capability", () => {
  it("maps view and manage permissions to the same module", () => {
    expect(moduleCapabilityFromPermission("inv.view")).toBe("inventory");
    expect(moduleCapabilityFromPermission("inv.manage")).toBe("inventory");
    expect(moduleCapabilityFromPermission("plan.view")).toBe("planning");
    expect(moduleCapabilityFromPermission("adm.users")).toBeNull();
  });

  it("allows all modules when capability map is empty (soft phase)", () => {
    expect(isModuleCapabilityAllowed({}, "plan.view")).toBe(true);
    expect(isModuleCapabilityAllowed(parseCapabilityMap("{}"), "pm.view")).toBe(true);
  });

  it("hides modules explicitly set to false", () => {
    const map = parseCapabilityMap(
      JSON.stringify({ equipment: true, planning: false, pm: false }),
    );
    expect(isModuleCapabilityAllowed(map, "eq.view")).toBe(true);
    expect(isModuleCapabilityAllowed(map, "plan.view")).toBe(false);
    expect(isModuleCapabilityAllowed(map, "pm.manage")).toBe(false);
  });
});
