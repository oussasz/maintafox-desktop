/**
 * Site scope of the work-order screens (step 6, site-scoped roles).
 *
 * - `useWoAccess`: the user's rights at a work order's site (its org node `entity_id`). A work
 *   order without a site is checked against plant-wide rights only, as the backend does.
 * - `WoHiddenNotice`: the list says how many work orders it leaves out, and why (outside the
 *   user's sites / without a site) — never silently. The counts ignore filters and search.
 * - `WoNoSiteBadge`: a work order recorded without a site is flagged "to review"; only
 *   plant-wide users see it.
 */

import { MapPinOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { type AccessAt, useAccessAt } from "@/hooks/use-access-at";
import { cn } from "@/lib/utils";
import type { WorkOrder } from "@shared/ipc-types";

/** A work order without an org node (NULL, or the `0` of old rows). */
export function woWithoutSite(wo: Pick<WorkOrder, "entity_id">): boolean {
  return !(wo.entity_id != null && wo.entity_id > 0);
}

/**
 * The org node a work order's rights are read at: its site, or `0` when it has none (the rights
 * at no node are the plant-wide ones only). `null` while no work order is loaded.
 */
export function woAccessNode(wo: Pick<WorkOrder, "entity_id"> | null | undefined): number | null {
  if (wo == null) return null;
  return woWithoutSite(wo) ? 0 : (wo.entity_id as number);
}

/** The user's rights at the work order's site; the backend checks the same and stays the authority. */
export function useWoAccess(wo: Pick<WorkOrder, "entity_id"> | null | undefined): AccessAt {
  return useAccessAt(woAccessNode(wo));
}

/** The user's plant-wide rights only (the rights at no node), for actions kept plant-wide. */
export function usePlantWideAccess(): AccessAt {
  return useAccessAt(0);
}

interface WoHiddenNoticeProps {
  outsideScope: number;
  withoutSite: number;
  className?: string;
}

export function WoHiddenNotice({ outsideScope, withoutSite, className }: WoHiddenNoticeProps) {
  const { t } = useTranslation("ot");
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
      <span>{t("siteScope.hidden.allWorkOrders")}</span>
      <span>{t("siteScope.hidden.askPlantWide")}</span>
    </div>
  );
}

interface WoNoSiteBadgeProps {
  wo: Pick<WorkOrder, "entity_id">;
  size?: "sm" | "md";
}

export function WoNoSiteBadge({ wo, size = "sm" }: WoNoSiteBadgeProps) {
  const { t } = useTranslation("ot");
  if (!woWithoutSite(wo)) return null;
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
