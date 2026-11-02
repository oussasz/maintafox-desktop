/**
 * What ticking or unticking rights in the role editor does to their prerequisites (CMMS Advisor
 * decision Q3, L5-06), from the declared dependencies of the permission registry.
 *
 * - Ticking a right also ticks the VIEW rights it requires (hard dependencies, followed down the
 *   chain), each with the note "ajouté car nécessaire à …". Nothing else is ever ticked on the
 *   user's behalf: a required action, approval or permit/LOTO right is reported instead, for the
 *   user to tick if it is meant.
 * - Unticking a right reports the ticked rights that require it. Nothing is unticked with it.
 *
 * The screen only proposes: the backend still validates every dependency on save, and refuses a
 * role whose hard dependencies are missing.
 */
import type { PermissionDependencyRow } from "@shared/ipc-types";

/** A view right: the only kind ticked automatically as a prerequisite. */
export function isViewRight(name: string): boolean {
  return name.endsWith(".view");
}

export interface DependencyLink {
  /** The ticked right that needs `required`. */
  perm: string;
  required: string;
}

export interface TogglePlan {
  /** Rights to tick: those asked for and their view prerequisites. */
  tick: string[];
  /** Rights to untick: those asked for, nothing more. */
  untick: string[];
  /** View right ticked automatically → the rights that needed it. */
  autoAdded: Map<string, string[]>;
  /** Required rights that are not view rights, left unticked (a warning). */
  notAdded: DependencyLink[];
  /** Ticked rights that require one of the rights just unticked (a warning). */
  dependents: DependencyLink[];
}

/** Hard dependencies by right. */
export function hardRequirements(
  dependencies: readonly PermissionDependencyRow[],
): Map<string, string[]> {
  const map = new Map<string, string[]>();
  for (const d of dependencies) {
    if (d.dependency_type !== "hard") continue;
    const known = map.get(d.permission_name) ?? [];
    // The registry may declare the same link twice: it counts once.
    if (!known.includes(d.required_permission_name)) {
      map.set(d.permission_name, [...known, d.required_permission_name]);
    }
  }
  return map;
}

/**
 * The effect of ticking (`enabled`) or unticking the rights `perms` on a role that holds `active`.
 * `offered` are the rights the editor shows: a prerequisite it does not show is never ticked.
 */
export function planToggle(
  perms: readonly string[],
  enabled: boolean,
  active: ReadonlySet<string>,
  requires: ReadonlyMap<string, readonly string[]>,
  offered: ReadonlySet<string>,
): TogglePlan {
  if (!enabled) {
    const removed = new Set(perms);
    const dependents: DependencyLink[] = [];
    for (const perm of active) {
      if (removed.has(perm)) continue;
      for (const required of requires.get(perm) ?? []) {
        if (removed.has(required)) dependents.push({ perm, required });
      }
    }
    return { tick: [], untick: [...removed], autoAdded: new Map(), notAdded: [], dependents };
  }

  const tick = new Set(perms);
  const autoAdded = new Map<string, string[]>();
  const notAdded: DependencyLink[] = [];
  const queue = [...perms];
  while (queue.length > 0) {
    const perm = queue.shift() as string;
    for (const required of requires.get(perm) ?? []) {
      if (active.has(required)) continue;
      if (tick.has(required)) {
        if (autoAdded.has(required)) autoAdded.get(required)?.push(perm);
        continue;
      }
      if (isViewRight(required) && offered.has(required)) {
        tick.add(required);
        autoAdded.set(required, [perm]);
        queue.push(required);
      } else {
        notAdded.push({ perm, required });
      }
    }
  }
  return {
    tick: [...tick],
    untick: [],
    autoAdded,
    // A required right the same action ticks (a whole group) is not missing.
    notAdded: notAdded.filter((l) => !tick.has(l.required)),
    dependents: [],
  };
}
