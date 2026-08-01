/**
 * @deprecated Prefer AssetTimelineList + shared Timeline.
 * Kept as a thin single-event mapper for any residual imports.
 */

import { Timeline } from "@/components/timeline";
import type { AssetHistoryEvent } from "@shared/ipc-types";

type Props = {
  event: AssetHistoryEvent;
  onOpenRef: (ev: AssetHistoryEvent) => void;
};

export function AssetTimelineItem({ event, onOpenRef }: Props) {
  return (
    <Timeline
      items={[event]}
      getEntry={(ev) => {
        const clickable = Boolean(ev.ref?.entity_type);
        return {
          id: ev.id,
          title: ev.title,
          description: ev.description ?? undefined,
          timestamp: ev.occurred_at,
          subtitle: ev.status_label ?? undefined,
          badges: [{ label: ev.event_type, tone: "muted" }],
          actor: ev.actor_label ?? undefined,
          link: clickable
            ? { label: ev.ref?.entity_code ?? "Open", onClick: () => onOpenRef(ev) }
            : undefined,
        };
      }}
      density="compact"
    />
  );
}
