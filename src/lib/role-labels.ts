/**
 * Plain-language names of the roles shipped with the application (CMMS Advisor decision L5-03).
 *
 * A shipped role keeps its internal name in the database: that name is a fixed key the backend
 * checks (`name IN ('Administrator', 'Superadmin')` and others), so it is never renamed. The
 * screens show it by its translated name and description (`admin:roles.shipped.<key>`), keyed by
 * that unchanged internal name. A role created by the plant keeps its own name.
 *
 * Two generations of shipped roles carry the same rights: the baseline seed (Administrator,
 * Supervisor, Operator, Readonly) and migration 028 (Superadmin, Maintenance Supervisor,
 * Maintenance Technician, Read Only Observer). The later one of each pair is a "former role": it is
 * hidden from the role list and from the pickers that give a role, while the role it duplicates
 * is there, and wherever it is still shown (an account that holds it) it reads
 * "Ancien rôle, identique à X". Nobody loses it; no data changes.
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

/** Name shown for a role: translated for a shipped role, its own name otherwise. */
export function roleLabel(t: AdminT, name: string): string {
  const visible = FORMER_ROLE_OF[name];
  if (visible) {
    return String(t("roles.shipped.formerRole", { ns: "admin", role: roleLabel(t, visible) }));
  }
  const key = SHIPPED_ROLE_KEYS[name];
  if (!key) return name;
  return String(t(`roles.shipped.${key}.name`, { ns: "admin", defaultValue: name }));
}

/**
 * Description shown for a role: translated for a shipped role (a former role says which role it
 * duplicates), the stored description otherwise.
 */
export function roleDescription(t: AdminT, name: string, stored: string | null): string | null {
  const visible = FORMER_ROLE_OF[name];
  if (visible) {
    return String(
      t("roles.shipped.formerRoleDescription", { ns: "admin", role: roleLabel(t, visible) }),
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

/** The role helpers bound to the admin namespace of the current language. */
export function useRoleText() {
  const { t } = useTranslation("admin");
  return useMemo(
    () => ({
      label: (name: string) => roleLabel(t, name),
      description: (name: string, stored: string | null) => roleDescription(t, name, stored),
    }),
    [t],
  );
}
