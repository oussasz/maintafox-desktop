import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfChip, mfPermissionDomainChip } from "@/design-system/tokens";
import type { PermissionRecord } from "@shared/ipc-types";

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
      <span
        className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs ${mfChip.fullAccess}`}
      >
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
          className={`inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ${mfPermissionDomainChip[domain] ?? mfChip.neutral}`}
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
