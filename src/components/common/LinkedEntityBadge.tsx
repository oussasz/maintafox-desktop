/**
 * LinkedEntityBadge — clickable primary badge for first-class business links
 * (DI ↔ WO today; extensible to asset, PM, inspection, …).
 *
 * Renders nothing when code is absent. Never displays database IDs.
 */

import { ClipboardList, Wrench } from "lucide-react";
import type { KeyboardEvent, MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { formatEntityCode } from "@/lib/display";
import { cn } from "@/lib/utils";

export type LinkedEntityKind = "work_order" | "di";

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
} as const;

function deepLinkPath(entity: LinkedEntityKind, entityId: number): string {
  if (entity === "work_order") return `/work-orders?openWo=${entityId}`;
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
