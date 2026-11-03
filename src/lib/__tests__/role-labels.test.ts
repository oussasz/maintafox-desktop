import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import fr from "@/i18n/locale-data/fr/admin.json";
import {
  formerRoleMatches,
  formerRoleOf,
  isShippedRole,
  roleDescription,
  roleLabel,
  templateDescription,
  templateLabel,
  withoutFormerRoles,
} from "@/lib/role-labels";
import { ROLE_TEMPLATES } from "@shared/rbac/permissions.generated";

/** `t` reading the French admin file, with i18next's `{{x}}` interpolation and defaultValue. */
const t = ((key: string, opts?: Record<string, unknown>) => {
  let node: unknown = fr;
  for (const part of key.split(".")) {
    node = node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined;
  }
  const text = typeof node === "string" ? node : String(opts?.["defaultValue"] ?? key);
  return text.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
}) as unknown as TFunction<"admin">;

const role = (id: number, name: string) => ({ id, name });

describe("shipped role names (L5-03)", () => {
  it("shows each shipped role by its French name, keyed by the unchanged internal name", () => {
    expect(roleLabel(t, "Administrator")).toBe("Administrateur");
    expect(roleLabel(t, "Supervisor")).toBe("Superviseur maintenance");
    expect(roleLabel(t, "Operator")).toBe("Technicien de maintenance");
    expect(roleLabel(t, "Readonly")).toBe("Consultation seule");
    expect(roleLabel(t, "Planner/Scheduler")).toBe("Planificateur");
    expect(roleLabel(t, "Finance Manager")).toBe("Responsable financier");
  });

  it("says 'identique' for a former role only when its rights were compared and are equal", () => {
    expect(roleLabel(t, "Superadmin", "same")).toBe("Ancien rôle, identique à Administrateur");
    expect(roleLabel(t, "Maintenance Supervisor", "different")).toBe(
      "Ancien rôle, proche de Superviseur maintenance : vérifiez ses droits",
    );
    // Not compared (the screen has no rights): never claims they are the same.
    expect(roleLabel(t, "Maintenance Technician")).toBe(
      "Ancien rôle, proche de Technicien de maintenance",
    );
    expect(roleLabel(t, "Read Only Observer")).toBe("Ancien rôle, proche de Consultation seule");
    expect(formerRoleOf("Superadmin")).toBe("Administrator");
  });

  it("compares each former role with the role it stands for, by rights", () => {
    const matches = formerRoleMatches([
      { name: "Administrator", permissions: ["a.view", "a.edit"] },
      { name: "Superadmin", permissions: ["a.edit", "a.view"] },
      { name: "Supervisor", permissions: ["a.view", "a.edit"] },
      { name: "Maintenance Supervisor", permissions: ["a.view"] },
      // Its twin is not in the list: not compared.
      { name: "Read Only Observer", permissions: [] },
    ]);
    expect(matches.get("Superadmin")).toBe("same");
    expect(matches.get("Maintenance Supervisor")).toBe("different");
    expect(matches.has("Read Only Observer")).toBe(false);
  });

  it("keeps a role created by the plant under its own name and description", () => {
    expect(roleLabel(t, "Chef d'équipe nuit")).toBe("Chef d'équipe nuit");
    expect(roleDescription(t, "Chef d'équipe nuit", "Équipe de nuit")).toBe("Équipe de nuit");
    expect(isShippedRole("Chef d'équipe nuit")).toBe(false);
  });

  it("describes a shipped role in French, never by its stored English text", () => {
    const text = roleDescription(t, "Readonly", "Read-only access to all operational modules.");
    expect(text).toBe("Consulte les écrans de maintenance sans rien pouvoir modifier.");
  });

  it("hides a former role while the role it duplicates is offered", () => {
    const roles = [
      role(1, "Administrator"),
      role(2, "Superadmin"),
      role(3, "Read Only Observer"),
      role(4, "Chef d'équipe"),
    ];
    // Readonly is not in this list: its former twin stays, so nobody is left without a choice.
    expect(withoutFormerRoles(roles).map((r) => r.id)).toEqual([1, 3, 4]);
    // The role a screen has open stays, whatever it is.
    expect(withoutFormerRoles(roles, 2).map((r) => r.id)).toEqual([1, 2, 3, 4]);
  });
});

describe("role models (L5-04)", () => {
  it("names and describes every model of the registry in French", () => {
    const names = Object.keys(ROLE_TEMPLATES);
    expect(names).toHaveLength(11);
    for (const name of names) {
      expect(templateLabel(t, name)).not.toBe(name);
      expect(templateDescription(t, name, null)).not.toBe("");
    }
    expect(templateLabel(t, "Storekeeper Template")).toBe("Magasinier");
    expect(templateLabel(t, "Production Operator Template")).toBe("Opérateur de production");
  });

  it("keeps a model made by the plant under its own name", () => {
    expect(templateLabel(t, "Modèle atelier")).toBe("Modèle atelier");
    expect(templateDescription(t, "Modèle atelier", "Notre modèle")).toBe("Notre modèle");
  });

  it("says in the storekeeper and buyer models how receiving and approving are kept apart", () => {
    expect(templateDescription(t, "Storekeeper Template", null)).toContain("droit de réception");
    expect(templateDescription(t, "Buyer Template", null)).toContain("droit d'approbation");
  });
});
