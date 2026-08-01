/**
 * DiReviewTimeline.tsx
 *
 * Renders di_review_events including SLA snapshot fields.
 * Presentation via shared Timeline.
 */

import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry } from "@/components/timeline";
import { formatOrDash } from "@/lib/display";
import { getDiReviewEvents } from "@/services/di-review-service";
import { formatDate as formatDiDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { DiReviewEvent } from "@shared/ipc-types";

interface DiReviewTimelineProps {
  diId: number;
}

function isPastDeadline(deadline: string | null, actedAt: string): boolean {
  if (!deadline) return false;
  return actedAt > deadline;
}

export function DiReviewTimeline({ diId }: DiReviewTimelineProps) {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [events, setEvents] = useState<DiReviewEvent[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    void getDiReviewEvents(diId)
      .then((rows) => {
        if (!cancelled) setEvents(rows);
      })
      .catch(() => {
        if (!cancelled) setEvents([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [diId]);

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((ev) => {
        const breached = ev.sla_deadline != null && isPastDeadline(ev.sla_deadline, ev.acted_at);
        const eventLabel = t(`reviewTimeline.eventType.${ev.event_type}`, {
          defaultValue: ev.event_type,
        });
        const hasSla =
          ev.sla_target_hours != null ||
          ev.sla_deadline != null ||
          ev.sla_resolution_target_hours != null ||
          ev.sla_resolution_deadline != null;

        return {
          id: String(ev.id),
          title: eventLabel,
          subtitle: `${ev.from_status} → ${ev.to_status}`,
          description: ev.notes ?? undefined,
          timestamp: ev.acted_at,
          actor: `${t("reviewTimeline.actor")}: ${t("reviewTimeline.systemActor")}`,
          tone: breached ? ("danger" as const) : undefined,
          details: hasSla ? (
            <div className="grid grid-cols-2 gap-2 text-xs">
              <div>
                <span className="text-text-muted">{t("reviewTimeline.slaTarget")}: </span>
                {ev.sla_target_hours != null
                  ? t("reviewTimeline.hours", { hours: ev.sla_target_hours })
                  : formatOrDash(null)}
              </div>
              <div>
                <span className="text-text-muted">{t("reviewTimeline.slaDeadline")}: </span>
                {ev.sla_deadline ? formatDiDate(ev.sla_deadline, dateLocale) : formatOrDash(null)}
              </div>
              <div>
                <span className="text-text-muted">{t("reviewTimeline.slaResolutionTarget")}: </span>
                {ev.sla_resolution_target_hours != null
                  ? t("reviewTimeline.hours", { hours: ev.sla_resolution_target_hours })
                  : formatOrDash(null)}
              </div>
              <div>
                <span className="text-text-muted">
                  {t("reviewTimeline.slaResolutionDeadline")}:{" "}
                </span>
                {ev.sla_resolution_deadline
                  ? formatDiDate(ev.sla_resolution_deadline, dateLocale)
                  : formatOrDash(null)}
              </div>
              {breached ? (
                <div className="col-span-2 font-medium text-red-700">
                  {t("reviewTimeline.breachHint")}
                </div>
              ) : null}
            </div>
          ) : undefined,
          detailsLabel: t("reviewTimeline.slaDetails", { defaultValue: "SLA details" }),
        };
      }),
    [events, t, dateLocale],
  );

  return (
    <Timeline
      items={entries}
      loading={loading}
      locale={dateLocale}
      density="compact"
      empty={t("reviewTimeline.empty")}
      aria-label={t("reviewTimeline.title", { defaultValue: "Review timeline" })}
    />
  );
}
