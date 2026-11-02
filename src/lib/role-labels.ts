/**
 * Plain-language names of the roles shipped with the application (CMMS Advisor decision L5-03).
 *
 * A shipped role keeps its internal name in the database: that name is a fixed key the backend
 * checks (`name IN ('Administrator', 'Superadmin')` and others), so it is never renamed. The
 * screens show it by its translated name and description (`admin:roles.shipped.<key>`), keyed by
 * that unchanged internal name. A role created by the plant keeps its own name.
 *
 * Two generations of shipped roles cover the same jobs: the baseline seed (Administrator,
 * Supervisor, Operator, Readonly) and migration 028 (Superadmin, Maintenance Supervisor,
 * Maintenance Technician, Read Only Observer). The later one of each pair is a "former role": it is
 * left out of the pickers that give a role while the role it stands for is there, and shown apart
 * ("Anciens rôles") in the role editor. Their rights can drift apart (Supervisor and Operator are
 * editable since migration 109, their former twins are not), so a former role says "identique à X"
 * only when its rights are compared and equal; "proche de X : vérifiez ses droits" when they
 * differ; "proche de X" when the screen does not have the rights to compare. Nobody loses it; no
 * data changes.
 */
import type { TFunction } from "i18next";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

type AdminT = TFunction<"admin">;

/** Translation key of each shipped role, by its unchanged internal name. */
const SHIPPED_ROLE_KEYS: Readonly<Record<string, string>> = {
  Administrator: "administrator",
  Supervisor: "supervisor",
  Operator: "technician",
  Readonly: "readonly",
  "Planner/Scheduler": "planner",
  "Finance Manager": "financeManager",
};

/** Former shipped role → the shipped role with the same rights that stays visible. */
const FORMER_ROLE_OF: Readonly<Record<string, string>> = {
  Superadmin: "Administrator",
  "Maintenance Supervisor": "Supervisor",
  "Maintenance Technician": "Operator",
  "Read Only Observer": "Readonly",
};

/** The visible shipped role a former role duplicates; null for any other role. */
export function formerRoleOf(name: string): string | null {
  return FORMER_ROLE_OF[name] ?? null;
}

/** Whether a role name is one of the roles shipped with the application. */
export function isShippedRole(name: string): boolean {
  return name in SHIPPED_ROLE_KEYS || name in FORMER_ROLE_OF;
}

/**
 * How a former role's rights compare with the role it stands for: `same` or `different` when the
 * screen compared them, `undefined` when it cannot (it has no rights to compare).
 */
export type FormerRoleMatch = "same" | "different" | undefined;

const FORMER_KEY_SUFFIX: Record<"same" | "different" | "unknown", string> = {
  same: "Same",
  different: "Different",
  unknown: "Unknown",
};

/**
 * Compares each former role of `roles` with the role it stands for, by their rights. A pair with
 * one side missing from the list is not compared.
 */
export function formerRoleMatches(
  roles: readonly { name: string; permissions: readonly string[] }[],
): Map<string, "same" | "different"> {
  const byName = new Map(roles.map((r) => [r.name, new Set(r.permissions)] as const));
  const result = new Map<string, "same" | "different">();
  for (const [former, visible] of Object.entries(FORMER_ROLE_OF)) {
    const a = byName.get(former);
    const b = byName.get(visible);
    if (!a || !b) continue;
    const same = a.size === b.size && [...a].every((p) => b.has(p));
    result.set(former, same ? "same" : "different");
  }
  return result;
}

/** Name shown for a role: translated for a shipped role, its own name otherwise. */
export function roleLabel(t: AdminT, name: string, match?: FormerRoleMatch): string {
  const visible = FORMER_ROLE_OF[name];
  if (visible) {
    const suffix = FORMER_KEY_SUFFIX[match ?? "unknown"];
    return String(
      t(`roles.shipped.formerRole${suffix}`, { ns: "admin", role: roleLabel(t, visible) }),
    );
  }
  const key = SHIPPED_ROLE_KEYS[name];
  if (!key) return name;
  return String(t(`roles.shipped.${key}.name`, { ns: "admin", defaultValue: name }));
}

/**
 * Description shown for a role: translated for a shipped role (a former role says which role it
 * duplicates), the stored description otherwise.
 */
export function roleDescription(
  t: AdminT,
  name: string,
  stored: string | null,
  match?: FormerRoleMatch,
): string | null {
  const visible = FORMER_ROLE_OF[name];
  if (visible) {
    const suffix = FORMER_KEY_SUFFIX[match ?? "unknown"];
    return String(
      t(`roles.shipped.formerRoleDescription${suffix}`, {
        ns: "admin",
        role: roleLabel(t, visible),
      }),
    );
  }
  const key = SHIPPED_ROLE_KEYS[name];
  if (!key) return stored;
  return String(t(`roles.shipped.${key}.description`, { ns: "admin", defaultValue: stored ?? "" }));
}

/**
 * Roles offered in a list or a picker: a former role is left out while the role it duplicates is
 * in the list. `keepId` keeps one role whatever it is (the role a form already holds, so its value
 * still shows).
 */
export function withoutFormerRoles<T extends { id: number; name: string }>(
  roles: readonly T[],
  keepId?: number | null,
): T[] {
  const names = new Set(roles.map((r) => r.name));
  return roles.filter((r) => {
    if (keepId != null && r.id === keepId) return true;
    const visible = FORMER_ROLE_OF[r.name];
    return !visible || !names.has(visible);
  });
}

/**
 * Translation key of each role model shipped with the application (L5-04), by its unchanged
 * stored name (`role_templates.name`, the key of the registry's `role_templates`).
 */
const SHIPPED_TEMPLATE_KEYS: Readonly<Record<string, string>> = {
  "Supervisor Template": "supervisor",
  "Technician Template": "technician",
  "Planner Template": "planner",
  "Observer Template": "observer",
  "Finance Manager Template": "financeManager",
  "Storekeeper Template": "storekeeper",
  "Buyer Template": "buyer",
  "HSE Manager Template": "hseManager",
  "Maintenance Manager Template": "maintenanceManager",
  "Cost Controller Template": "costController",
  "Production Operator Template": "productionOperator",
};

/** Name shown for a role model: translated for a shipped one, its own name otherwise. */
export function templateLabel(t: AdminT, name: string): string {
  const key = SHIPPED_TEMPLATE_KEYS[name];
  if (!key) return name;
  return String(t(`roles.templates.${key}.name`, { ns: "admin", defaultValue: name }));
}

/** Description shown for a role model: translated for a shipped one, the stored one otherwise. */
export function templateDescription(t: AdminT, name: string, stored: string | null): string | null {
  const key = SHIPPED_TEMPLATE_KEYS[name];
  if (!key) return stored;
  return String(
    t(`roles.templates.${key}.description`, { ns: "admin", defaultValue: stored ?? "" }),
  );
}

/** The role helpers bound to the admin namespace of the current language. */
export function useRoleText() {
  const { t } = useTranslation("admin");
  return useMemo(
    () => ({
      label: (name: string, match?: FormerRoleMatch) => roleLabel(t, name, match),
      description: (name: string, stored: string | null, match?: FormerRoleMatch) =>
        roleDescription(t, name, stored, match),
      templateLabel: (name: string) => templateLabel(t, name),
      templateDescription: (name: string, stored: string | null) =>
        templateDescription(t, name, stored),
    }),
    [t],
  );
}
