/**
 * Plain-language text for permission codes on the administration screens.
 *
 * A permission is shown by its translated label (`admin:permissions.items.<code>.label`, the
 * code with `.` replaced by `_`) and description; when a key is missing, the catalogue
 * description stands in, and only then the code itself.
 */
import type { TFunction } from "i18next";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { P, PERMISSION_CATALOG } from "@shared/rbac/permissions.generated";

/** `t` of the admin namespace (`useTranslation("admin")`). */
type AdminT = TFunction<"admin">;

/** Permissions shown under another domain than their code prefix. */
const DOMAIN_OVERRIDES: Readonly<Record<string, string>> = {
  // Goods-receipt validation is a stores task (the ERP connector itself is not in this version).
  [P.ERP_RECONCILE]: "inv",
};

/** Domain a permission is grouped under (`ot.close` → `ot`). */
export function permissionDomain(name: string): string {
  return DOMAIN_OVERRIDES[name] ?? name.split(".")[0] ?? name;
}

function itemKey(name: string, field: "label" | "description"): string {
  return `permissions.items.${name.replace(/\./g, "_")}.${field}`;
}

/** Catalogue description of a system permission (the text stored with it). */
function catalogDescription(name: string): string | null {
  return PERMISSION_CATALOG.find((p) => p.name === name)?.description ?? null;
}

/** Translated label; falls back to the stored description, then the code. */
export function permissionLabel(t: AdminT, name: string, description?: string | null): string {
  const fallback = description?.trim() || catalogDescription(name) || name;
  return String(t(itemKey(name, "label"), { ns: "admin", defaultValue: fallback }));
}

/** Translated description; falls back to the stored description (empty when there is none). */
export function permissionDescription(
  t: AdminT,
  name: string,
  description?: string | null,
): string {
  const fallback = description?.trim() || catalogDescription(name) || "";
  return String(t(itemKey(name, "description"), { ns: "admin", defaultValue: fallback }));
}

/** Translated domain name (`admin:permissions.domainLabels.<domain>`), the code otherwise. */
export function permissionDomainLabel(t: AdminT, domain: string): string {
  return String(t(`permissions.domainLabels.${domain}`, { ns: "admin", defaultValue: domain }));
}

/** The three helpers bound to the admin namespace of the current language. */
export function usePermissionText() {
  const { t } = useTranslation("admin");
  return useMemo(
    () => ({
      label: (name: string, description?: string | null) => permissionLabel(t, name, description),
      description: (name: string, description?: string | null) =>
        permissionDescription(t, name, description),
      domainLabel: (domain: string) => permissionDomainLabel(t, domain),
    }),
    [t],
  );
}
