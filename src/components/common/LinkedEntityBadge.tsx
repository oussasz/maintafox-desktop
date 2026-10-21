/**
 * LinkedEntityBadge — clickable primary badge for first-class business links
 * (DI ↔ WO ↔ permit, PM plan, asset, inspection round).
 *
 * Renders nothing when code is absent. Never displays database IDs.
 */

import { CalendarClock, ClipboardCheck, ClipboardList, Cog, HardHat, Wrench } from "lucide-react";
import type { KeyboardEvent, MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { PERMIT_QUERY_SELECT } from "@/components/permits/permit-rules";
import { Badge } from "@/components/ui/badge";
import { formatEntityCode } from "@/lib/display";
import { cn } from "@/lib/utils";
import { formatDate } from "@/utils/format-date";

export type LinkedEntityKind =
  | "work_order"
  | "di"
  | "permit"
  | "pm_plan"
  | "asset"
  | "inspection_round";

/** Tooltip of an inspection-round link: the round's template name and its scheduled date. */
export function roundTitle(
  templateName: string | null | undefined,
  scheduledAt: string | null | undefined,
  language: string,
): string | null {
  const name = (templateName ?? "").trim();
  if (!name) return null;
  return scheduledAt ? `${name} · ${formatDate(scheduledAt, language)}` : name;
}

export interface LinkedEntityBadgeProps {
  entity: LinkedEntityKind;
  code: string | null | undefined;
  /** Numeric FK for deep-link routing only — never rendered. */
  entityId?: number | null | undefined;
  /** Tooltip body (e.g. title); falls back to open-entity label. */
  title?: string | null | undefined;
  className?: string | undefined;
  /** Override default navigate deep-link. */
  onNavigate?: (() => void) | undefined;
}

const ENTITY_ICON = {
  work_order: Wrench,
  di: ClipboardList,
  permit: HardHat,
  pm_plan: CalendarClock,
  asset: Cog,
  inspection_round: ClipboardCheck,
} as const;

function deepLinkPath(entity: LinkedEntityKind, entityId: number): string {
  if (entity === "work_order") return `/work-orders?openWo=${entityId}`;
  if (entity === "permit") return `/permits?${PERMIT_QUERY_SELECT}=${entityId}`;
  if (entity === "pm_plan") return `/pm?planId=${entityId}`;
  if (entity === "asset") return `/equipment?assetId=${entityId}`;
  if (entity === "inspection_round") return `/inspections?openRound=${entityId}`;
  return `/requests?openDi=${entityId}`;
}

export function LinkedEntityBadge({
  entity,
  code,
  entityId,
  title,
  className,
  onNavigate,
}: LinkedEntityBadgeProps) {
  const { t } = useTranslation("common");
  const navigate = useNavigate();

  const trimmed = (code ?? "").trim();
  if (!trimmed) return null;

  const displayCode = formatEntityCode(trimmed);
  if (displayCode === "—") return null;

  const Icon = ENTITY_ICON[entity];
  const label = t(`linkedEntity.${entity}`, { code: displayCode });
  const tooltip = (title ?? "").trim() || t(`linkedEntity.open.${entity}`, { code: displayCode });
  const canNavigate = entityId != null && entityId > 0;

  const go = () => {
    if (!canNavigate || entityId == null) return;
    if (onNavigate) {
      onNavigate();
      return;
    }
    void navigate(deepLinkPath(entity, entityId));
  };

  const handleClick = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    go();
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.stopPropagation();
    e.preventDefault();
    go();
  };

  return (
    <Badge
      variant="default"
      role={canNavigate ? "link" : undefined}
      tabIndex={canNavigate ? 0 : undefined}
      title={tooltip}
      aria-label={tooltip}
      onClick={canNavigate ? handleClick : undefined}
      onKeyDown={canNavigate ? handleKeyDown : undefined}
      className={cn(
        "max-w-full gap-1 font-mono font-semibold",
        canNavigate && "cursor-pointer hover:bg-primary/80",
        !canNavigate && "cursor-default",
        className,
      )}
    >
      <Icon className="h-3 w-3 shrink-0" aria-hidden />
      <span className="truncate">{label}</span>
    </Badge>
  );
}
