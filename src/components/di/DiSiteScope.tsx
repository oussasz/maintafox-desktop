/**
 * Site scope of the requests screen (step 6, site-scoped roles).
 *
 * - `DiHiddenRequestsNotice`: the list says how many requests matching the filters it leaves
 *   out, and why (outside the user's sites / without a site) — never silently.
 * - `DiNoSiteBadge`: a request recorded without a site (before a site was required) is flagged
 *   "to review"; only plant-wide users see it.
 */

import { MapPinOff } from "lucide-react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { cn } from "@/lib/utils";
import type { InterventionRequest } from "@shared/ipc-types";

/** A request without an org node (the `0` of rows from before a site was required). */
export function diWithoutSite(di: Pick<InterventionRequest, "org_node_id">): boolean {
  return !(di.org_node_id > 0);
}

interface DiHiddenRequestsNoticeProps {
  outsideScope: number;
  withoutSite: number;
  className?: string;
}

export function DiHiddenRequestsNotice({
  outsideScope,
  withoutSite,
  className,
}: DiHiddenRequestsNoticeProps) {
  const { t } = useTranslation("di");
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
        <span>{t("list.hidden.outsideScope", { count: outsideScope })}</span>
      ) : null}
      {withoutSite > 0 ? <span>{t("list.hidden.withoutSite", { count: withoutSite })}</span> : null}
      <span>{t("list.hidden.allRequests")}</span>
      <span>{t("list.hidden.askPlantWide")}</span>
    </div>
  );
}

interface DiNoSiteBadgeProps {
  di: Pick<InterventionRequest, "org_node_id">;
  size?: "sm" | "md";
}

export function DiNoSiteBadge({ di, size = "sm" }: DiNoSiteBadgeProps) {
  const { t } = useTranslation("di");
  if (!diWithoutSite(di)) return null;
  return (
    <StatusChip
      label={t("noSite.badge")}
      title={t("noSite.hint")}
      tone="warning"
      icon={MapPinOff}
      size={size}
    />
  );
}
