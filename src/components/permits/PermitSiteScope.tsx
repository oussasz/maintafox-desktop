/**
 * Site scope of the permit screens (step 6, site-scoped roles).
 *
 * - `usePermitAccess`: the user's rights at a permit's site (its org node `entity_id`). A permit
 *   without a site is checked against plant-wide rights only, as the backend does.
 * - `PermitHiddenNotice`: the list says how many permits it leaves out, and why (outside the
 *   user's sites / without a site) — never silently.
 * - `PermitNoSiteBadge`: a permit recorded without a site is flagged "to review"; only
 *   plant-wide users see it.
 */

import { MapPinOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { type AccessAt, useAccessAt } from "@/hooks/use-access-at";
import { cn } from "@/lib/utils";
import type { WorkPermitRecord } from "@/services/permit-service";

type PermitSite = Pick<WorkPermitRecord, "entity_id">;

/** A permit without an org node (the `0` or NULL of rows recorded without a site). */
export function permitWithoutSite(permit: PermitSite): boolean {
  return !(permit.entity_id != null && permit.entity_id > 0);
}

/** The org node a permit's rights are read at: its site, or `0` (plant-wide rights only). */
export function permitAccessNode(permit: PermitSite | null | undefined): number | null {
  if (permit == null) return null;
  return permitWithoutSite(permit) ? 0 : permit.entity_id;
}

/** The user's rights at the permit's site; the backend checks the same and stays the authority. */
export function usePermitAccess(permit: PermitSite | null | undefined): AccessAt {
  return useAccessAt(permitAccessNode(permit));
}

/** The user's plant-wide rights only (the rights at no node), for plant-wide objects. */
export function usePermitPlantWideAccess(): AccessAt {
  return useAccessAt(0);
}

interface PermitHiddenNoticeProps {
  outsideScope: number;
  withoutSite: number;
  className?: string;
}

export function PermitHiddenNotice({
  outsideScope,
  withoutSite,
  className,
}: PermitHiddenNoticeProps) {
  const { t } = useTranslation("permits");
  if (outsideScope <= 0 && withoutSite <= 0) return null;
  return (
    <div
      role="status"
      className={cn(
        "flex flex-wrap items-center gap-x-2 gap-y-1 rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-xs text-tone-warning-fg",
        className,
      )}
    >
      {outsideScope > 0 ? (
        <span>{t("siteScope.hidden.outsideScope", { count: outsideScope })}</span>
      ) : null}
      {withoutSite > 0 ? (
        <span>{t("siteScope.hidden.withoutSite", { count: withoutSite })}</span>
      ) : null}
      <span>{t("siteScope.hidden.allPermits")}</span>
      <span>{t("siteScope.hidden.askPlantWide")}</span>
    </div>
  );
}

interface PermitNoSiteBadgeProps {
  permit: PermitSite;
  size?: "sm" | "md";
}

export function PermitNoSiteBadge({ permit, size = "sm" }: PermitNoSiteBadgeProps) {
  const { t } = useTranslation("permits");
  if (!permitWithoutSite(permit)) return null;
  return (
    <StatusChip
      label={t("siteScope.noSite.badge")}
      title={t("siteScope.noSite.hint")}
      tone="warning"
      icon={MapPinOff}
      size={size}
    />
  );
}
