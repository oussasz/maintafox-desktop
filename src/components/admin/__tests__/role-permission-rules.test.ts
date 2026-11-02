import { describe, expect, it } from "vitest";

import { hardRequirements, planToggle } from "@/components/admin/role-permission-rules";
import type { PermissionDependencyRow } from "@shared/ipc-types";
import registry from "@shared/rbac/permission-registry.json";
import { P } from "@shared/rbac/permissions.generated";

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
    const plan = planToggle([P.OT_CREATE], true, new Set(), REQUIRES, OFFERED);
    expect(plan.tick.sort()).toEqual([P.OT_CREATE, P.OT_VIEW]);
    expect(plan.autoAdded.get(P.OT_VIEW)).toEqual([P.OT_CREATE]);
    expect(plan.notAdded).toEqual([]);
  });

  it("counts a link declared twice in the registry once", () => {
    expect(REQUIRES.get(P.OT_CREATE)).toEqual([P.OT_VIEW]);
  });

  it("follows the chain of view rights (a position needs personnel)", () => {
    const plan = planToggle([P.PER_POSITION_MANAGE], true, new Set(), REQUIRES, OFFERED);
    expect(plan.tick.sort()).toEqual([P.PER_POSITION_MANAGE, P.PER_POSITION_VIEW, P.PER_VIEW]);
  });

  it("never ticks an action or approval right: it reports it", () => {
    const plan = planToggle([P.DI_CONVERT], true, new Set([P.DI_VIEW]), REQUIRES, OFFERED);
    expect(plan.tick).toEqual([P.DI_CONVERT]);
    expect(plan.notAdded).toEqual([{ perm: P.DI_CONVERT, required: P.DI_APPROVE }]);
  });

  it("does not tick a prerequisite the role already holds", () => {
    const plan = planToggle([P.OT_CREATE], true, new Set([P.OT_VIEW]), REQUIRES, OFFERED);
    expect(plan.tick).toEqual([P.OT_CREATE]);
    expect(plan.autoAdded.size).toBe(0);
  });

  it("does not report a prerequisite ticked by the same action (a whole group)", () => {
    const plan = planToggle([P.OT_VIEW, P.OT_EDIT, P.OT_CLOSE], true, new Set(), REQUIRES, OFFERED);
    expect(plan.notAdded).toEqual([]);
    // Asked for explicitly: no "added because" note.
    expect(plan.autoAdded.size).toBe(0);
  });

  it("never ticks a prerequisite the editor does not show", () => {
    const plan = planToggle([P.OT_CREATE], true, new Set(), REQUIRES, new Set([P.OT_CREATE]));
    expect(plan.tick).toEqual([P.OT_CREATE]);
    expect(plan.notAdded).toEqual([{ perm: P.OT_CREATE, required: P.OT_VIEW }]);
  });
});

describe("unticking a right (L5-06)", () => {
  it("unticks only that right and lists the ticked rights that need it", () => {
    const active = new Set([P.OT_VIEW, P.OT_CREATE, P.OT_EDIT, P.EQ_VIEW]);
    const plan = planToggle([P.OT_VIEW], false, active, REQUIRES, OFFERED);
    expect(plan.untick).toEqual([P.OT_VIEW]);
    expect(plan.tick).toEqual([]);
    expect(plan.dependents).toEqual([
      { perm: P.OT_CREATE, required: P.OT_VIEW },
      { perm: P.OT_EDIT, required: P.OT_VIEW },
    ]);
  });

  it("warns of nothing when the dependents go with it", () => {
    const active = new Set([P.OT_VIEW, P.OT_CREATE]);
    const plan = planToggle([P.OT_VIEW, P.OT_CREATE], false, active, REQUIRES, OFFERED);
    expect(plan.dependents).toEqual([]);
  });
});
