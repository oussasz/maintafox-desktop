import { Clock3, User } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { AssetHistoryEvent } from "@shared/ipc-types";

type Props = {
  event: AssetHistoryEvent;
  onOpenRef: (ev: AssetHistoryEvent) => void;
};

export function AssetTimelineItem({ event, onOpenRef }: Props) {
  const { t } = useTranslation("equipment");
  const typeLabel = t(`history.eventTypes.${event.event_type}`, {
    defaultValue: event.event_type,
  });
  const clickable = Boolean(event.ref?.entity_type);

  return (
    <div className="relative rounded border border-surface-border bg-surface-1 p-2.5">
      <div className="absolute -left-[14px] top-3.5 h-2.5 w-2.5 rounded-full border-2 border-primary bg-surface-0" />
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge variant="secondary" className="h-5 px-1.5 text-[10px] font-normal">
              {typeLabel}
            </Badge>
            {event.status_label ? (
              <span className="text-[11px] text-text-muted">{event.status_label}</span>
            ) : null}
          </div>
          <p className="text-sm font-medium leading-snug">{event.title}</p>
          {event.description ? (
            <p className="text-xs text-text-muted line-clamp-2">{event.description}</p>
          ) : null}
          <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-text-muted">
            <span>
              {new Date(event.occurred_at).toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              })}
            </span>
            {event.actor_label ? (
              <span className="inline-flex items-center gap-1">
                <User className="h-3 w-3" />
                {event.actor_label}
              </span>
            ) : null}
            {event.duration_minutes != null ? (
              <span className="inline-flex items-center gap-1">
                <Clock3 className="h-3 w-3" />
                {Math.floor(event.duration_minutes / 60)}h
                {String(event.duration_minutes % 60).padStart(2, "0")}
              </span>
            ) : null}
          </div>
        </div>
        {clickable ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 px-2 text-xs"
            onClick={() => onOpenRef(event)}
          >
            {event.ref?.entity_code ?? t("history.open")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}
