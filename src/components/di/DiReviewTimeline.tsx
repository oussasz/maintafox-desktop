/**
 * DiReviewTimeline.tsx
 *
 * Renders di_review_events including SLA snapshot fields.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getDiReviewEvents } from "@/services/di-review-service";
import { formatOrDash } from "@/lib/display";
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

  if (loading) {
    return <p className="text-sm text-text-muted p-2">{t("sla.loading")}</p>;
  }

  if (events.length === 0) {
    return <p className="text-sm text-text-muted p-2">{t("reviewTimeline.empty")}</p>;
  }

  return (
    <ul className="space-y-3">
      {events.map((ev) => {
        const breached =
          ev.sla_deadline != null && isPastDeadline(ev.sla_deadline, ev.acted_at);
        const eventLabel = t(`reviewTimeline.eventType.${ev.event_type}`, {
          defaultValue: ev.event_type,
        });
        return (
          <li
            key={ev.id}
            className="rounded-md border border-surface-border p-3 text-sm space-y-1.5"
          >
            <div className="flex items-center justify-between gap-2">
              <span className="font-medium text-text-primary">{eventLabel}</span>
              <span className="text-xs text-text-muted">
                {formatDiDate(ev.acted_at, dateLocale)}
              </span>
            </div>
            <div className="text-xs text-text-muted">
              {ev.from_status} → {ev.to_status}
            </div>
            {ev.notes ? (
              <p className="text-xs text-text-secondary">{ev.notes}</p>
            ) : null}
            {(ev.sla_target_hours != null ||
              ev.sla_deadline != null ||
              ev.sla_resolution_target_hours != null ||
              ev.sla_resolution_deadline != null) && (
              <div className="mt-1 grid grid-cols-2 gap-2 text-xs">
                <div>
                  <span className="text-text-muted">{t("reviewTimeline.slaTarget")}: </span>
                  {ev.sla_target_hours != null
                    ? t("reviewTimeline.hours", { hours: ev.sla_target_hours })
                    : formatOrDash(null)}
                </div>
                <div>
                  <span className="text-text-muted">{t("reviewTimeline.slaDeadline")}: </span>
                  {ev.sla_deadline
                    ? formatDiDate(ev.sla_deadline, dateLocale)
                    : formatOrDash(null)}
                </div>
                <div>
                  <span className="text-text-muted">{t("reviewTimeline.slaResolutionTarget")}: </span>
                  {ev.sla_resolution_target_hours != null
                    ? t("reviewTimeline.hours", { hours: ev.sla_resolution_target_hours })
                    : formatOrDash(null)}
                </div>
                <div>
                  <span className="text-text-muted">{t("reviewTimeline.slaResolutionDeadline")}: </span>
                  {ev.sla_resolution_deadline
                    ? formatDiDate(ev.sla_resolution_deadline, dateLocale)
                    : formatOrDash(null)}
                </div>
                {breached ? (
                  <div className="col-span-2 text-red-700 font-medium">
                    {t("reviewTimeline.breachHint")}
                  </div>
                ) : null}
              </div>
            )}
            <div className="text-xs text-text-muted">
              {t("reviewTimeline.actor")}: {t("reviewTimeline.systemActor")}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
