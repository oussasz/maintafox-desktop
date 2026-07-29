import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { AssetTimelineItem } from "@/components/assets/history/AssetTimelineItem";
import type { AssetHistoryEvent } from "@shared/ipc-types";

function dayKey(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toISOString().slice(0, 10);
}

function dayLabel(iso: string, locale: string, todayLabel: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return todayLabel;
  return d.toLocaleDateString(locale, {
    day: "2-digit",
    month: "short",
    year: "numeric",
  });
}

export function AssetTimelineList({
  events,
  onOpenRef,
}: {
  events: AssetHistoryEvent[];
  onOpenRef: (ev: AssetHistoryEvent) => void;
}) {
  const { t, i18n } = useTranslation("equipment");
  const grouped = useMemo(() => {
    const map = new Map<string, { label: string; items: AssetHistoryEvent[] }>();
    for (const ev of events) {
      const key = dayKey(ev.occurred_at);
      const existing = map.get(key);
      if (existing) {
        existing.items.push(ev);
      } else {
        map.set(key, {
          label: dayLabel(ev.occurred_at, i18n.language, t("history.today")),
          items: [ev],
        });
      }
    }
    return Array.from(map.entries());
  }, [events, i18n.language, t]);

  if (events.length === 0) {
    return <p className="text-sm text-text-muted">{t("history.empty")}</p>;
  }

  return (
    <div className="space-y-4">
      {grouped.map(([key, group]) => (
        <div key={key} className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-text-muted">
            {group.label}
          </p>
          <div className="space-y-2 border-l border-surface-border pl-3">
            {group.items.map((ev) => (
              <AssetTimelineItem key={ev.id} event={ev} onOpenRef={onOpenRef} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
