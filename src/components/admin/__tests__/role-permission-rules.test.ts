import { describe, expect, it } from "vitest";

import { hardRequirements, planToggle } from "@/components/admin/role-permission-rules";
import type { PermissionDependencyRow } from "@shared/ipc-types";
import registry from "@shared/rbac/permission-registry.json";

/** The registry's real dependencies, as the editor catalogue carries them. */
const DEPS: PermissionDependencyRow[] = registry.dependencies.map((d, i) => ({
  id: i + 1,
  permission_name: d.permission,
  required_permission_name: d.requires,
  dependency_type: d.dep_type,
}));
const REQUIRES = hardRequirements(DEPS);
const OFFERED = new Set(registry.permissions.map((p) => p.name));

describe("ticking a right (L5-06)", () => {
  it("ticks the view right it needs, with the right that needed it", () => {
    const plan = planToggle(["ot.create"], true, new Set(), REQUIRES, OFFERED);
    expect(plan.tick.sort()).toEqual(["ot.create", "ot.view"]);
    expect(plan.autoAdded.get("ot.view")).toEqual(["ot.create"]);
    expect(plan.notAdded).toEqual([]);
  });

  it("counts a link declared twice in the registry once", () => {
    expect(REQUIRES.get("ot.create")).toEqual(["ot.view"]);
  });

  it("follows the chain of view rights (a position needs personnel)", () => {
    const plan = planToggle(["per.position.manage"], true, new Set(), REQUIRES, OFFERED);
    expect(plan.tick.sort()).toEqual(["per.position.manage", "per.position.view", "per.view"]);
  });

  it("never ticks an action or approval right: it reports it", () => {
    const plan = planToggle(["di.convert"], true, new Set(["di.view"]), REQUIRES, OFFERED);
    expect(plan.tick).toEqual(["di.convert"]);
    expect(plan.notAdded).toEqual([{ perm: "di.convert", required: "di.approve" }]);
  });

  it("does not tick a prerequisite the role already holds", () => {
    const plan = planToggle(["ot.create"], true, new Set(["ot.view"]), REQUIRES, OFFERED);
    expect(plan.tick).toEqual(["ot.create"]);
    expect(plan.autoAdded.size).toBe(0);
  });

  it("does not report a prerequisite ticked by the same action (a whole group)", () => {
    const plan = planToggle(["ot.view", "ot.edit", "ot.close"], true, new Set(), REQUIRES, OFFERED);
    expect(plan.notAdded).toEqual([]);
    // Asked for explicitly: no "added because" note.
    expect(plan.autoAdded.size).toBe(0);
  });

  it("never ticks a prerequisite the editor does not show", () => {
    const plan = planToggle(["ot.create"], true, new Set(), REQUIRES, new Set(["ot.create"]));
    expect(plan.tick).toEqual(["ot.create"]);
    expect(plan.notAdded).toEqual([{ perm: "ot.create", required: "ot.view" }]);
  });
});

describe("unticking a right (L5-06)", () => {
  it("unticks only that right and lists the ticked rights that need it", () => {
    const active = new Set(["ot.view", "ot.create", "ot.edit", "eq.view"]);
    const plan = planToggle(["ot.view"], false, active, REQUIRES, OFFERED);
    expect(plan.untick).toEqual(["ot.view"]);
    expect(plan.tick).toEqual([]);
    expect(plan.dependents).toEqual([
      { perm: "ot.create", required: "ot.view" },
      { perm: "ot.edit", required: "ot.view" },
    ]);
  });

  it("warns of nothing when the dependents go with it", () => {
    const active = new Set(["ot.view", "ot.create"]);
    const plan = planToggle(["ot.view", "ot.create"], false, active, REQUIRES, OFFERED);
    expect(plan.dependents).toEqual([]);
  });
});
