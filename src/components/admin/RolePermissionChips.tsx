import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { PermissionRecord } from "@shared/ipc-types";

// ── Domain colour mapping ─────────────────────────────────────────────────

const DOMAIN_COLORS: Record<string, string> = {
  di: "bg-blue-100 text-blue-800",
  ot: "bg-emerald-100 text-emerald-800",
  eq: "bg-orange-100 text-orange-800",
  pm: "bg-violet-100 text-violet-800",
  inv: "bg-amber-100 text-amber-800",
  per: "bg-cyan-100 text-cyan-800",
  org: "bg-pink-100 text-pink-800",
  ref: "bg-slate-100 text-slate-800",
  adm: "bg-red-100 text-red-800",
};

const DEFAULT_COLOR = "bg-gray-100 text-gray-800";
const MAX_VISIBLE_CHIPS = 5;

interface RolePermissionChipsProps {
  permissions: PermissionRecord[];
  /** Total permissions in the catalog — if all are present, show "Full access" */
  totalPermissionCount?: number;
}

/**
 * Domain-coloured permission chips for role list views.
 * Groups permissions by domain prefix and renders compact chips: `DI (5)` `OT (7)`.
 * Shows a gold "Full access" badge if the role has ALL permissions.
 */
export function RolePermissionChips({
  permissions,
  totalPermissionCount,
}: RolePermissionChipsProps) {
  const { t } = useTranslation("admin");
  const [showAll, setShowAll] = useState(false);

  // Group by domain prefix
  const domainCounts = useMemo(() => {
    const map = new Map<string, number>();
    for (const perm of permissions) {
      const dot = perm.name.indexOf(".");
      const domain = dot > 0 ? perm.name.slice(0, dot) : "other";
      map.set(domain, (map.get(domain) ?? 0) + 1);
    }
    // Sort by count descending
    return Array.from(map.entries()).sort((a, b) => b[1] - a[1]);
  }, [permissions]);

  // Full access check
  const isFullAccess =
    totalPermissionCount != null &&
    permissions.length >= totalPermissionCount &&
    totalPermissionCount > 0;

  if (isFullAccess) {
    return (
      <span className="inline-flex items-center rounded-full bg-amber-100 px-2.5 py-0.5 text-xs font-medium text-amber-800 ring-1 ring-amber-300/50">
        {t("roles.fullAccess", "Accès complet")}
      </span>
    );
  }

  if (domainCounts.length === 0) {
    return (
      <span className="text-xs text-text-muted">
        {t("roles.noPermissions", "Aucune permission")}
      </span>
    );
  }

  const visible = showAll ? domainCounts : domainCounts.slice(0, MAX_VISIBLE_CHIPS);
  const hiddenCount = domainCounts.length - MAX_VISIBLE_CHIPS;

  return (
    <div className="flex flex-wrap items-center gap-1">
      {visible.map(([domain, count]) => (
        <span
          key={domain}
          className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${DOMAIN_COLORS[domain] ?? DEFAULT_COLOR}`}
        >
          {domain.toUpperCase()} ({count})
        </span>
      ))}
      {!showAll && hiddenCount > 0 && (
        <button
          type="button"
          onClick={() => setShowAll(true)}
          className="inline-flex items-center rounded-full bg-surface-2 px-2 py-0.5 text-xs font-medium text-text-secondary hover:bg-surface-3 transition-colors"
        >
          +{hiddenCount}
        </button>
      )}
    </div>
  );
}
