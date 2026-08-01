import { Clock3, User } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry } from "@/components/timeline";
import type { AssetHistoryEvent } from "@shared/ipc-types";

export function AssetTimelineList({
  events,
  onOpenRef,
}: {
  events: AssetHistoryEvent[];
  onOpenRef: (ev: AssetHistoryEvent) => void;
}) {
  const { t, i18n } = useTranslation("equipment");

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((event) => {
        const typeLabel = t(`history.eventTypes.${event.event_type}`, {
          defaultValue: event.event_type,
        });
        const clickable = Boolean(event.ref?.entity_type);
        const metaBits: ReactNode[] = [];
        if (event.actor_label) {
          metaBits.push(
            <span key="actor" className="inline-flex items-center gap-1">
              <User className="h-3 w-3" aria-hidden />
              {event.actor_label}
            </span>,
          );
        }
        if (event.duration_minutes != null) {
          metaBits.push(
            <span key="dur" className="inline-flex items-center gap-1">
              <Clock3 className="h-3 w-3" aria-hidden />
              {Math.floor(event.duration_minutes / 60)}h
              {String(event.duration_minutes % 60).padStart(2, "0")}
            </span>,
          );
        }

        return {
          id: event.id,
          title: event.title,
          description: event.description ?? undefined,
          timestamp: event.occurred_at,
          subtitle: event.status_label ?? undefined,
          badges: [{ label: typeLabel, tone: "muted" as const }],
          actor: metaBits.length ? <>{metaBits}</> : undefined,
          link: clickable
            ? {
                label: event.ref?.entity_code ?? t("history.open"),
                onClick: () => onOpenRef(event),
              }
            : undefined,
        };
      }),
    [events, onOpenRef, t],
  );

  return (
    <Timeline
      items={entries}
      groupBy="day"
      todayLabel={t("history.today")}
      locale={i18n.language}
      density="compact"
      empty={t("history.empty")}
      aria-label={t("history.title", { defaultValue: "Asset history" })}
    />
  );
}
