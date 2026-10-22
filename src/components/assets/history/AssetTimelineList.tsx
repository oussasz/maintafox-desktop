import { Clock3, User } from "lucide-react";
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { ASSET_STATUS_LABEL_KEY } from "@/components/assets/asset-tones";
import { diStatusToI18nKey } from "@/components/di/status-meta";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry } from "@/components/timeline";
import { statusToI18nKey } from "@/utils/wo-status";
import type { AssetHistoryEvent } from "@shared/ipc-types";

type TranslateFn = (key: string, opts?: Record<string, unknown>) => string;

/** Event types whose backend title is a fixed English phrase: shown from `history.titles.*`. */
const TRANSLATED_TITLES = new Set([
  "asset_created",
  "asset_updated",
  "decommissioned",
  "assignment_changed",
  "location_changed",
  "org_changed",
  "parent_changed",
  "child_changed",
  "wo_created",
  "wo_started",
  "wo_closed",
  "di_created",
  "di_closed",
  "pm_scheduled",
  "pm_executed",
  "failure_recorded",
  "meter_reading",
  "document_linked",
  "photo_added",
]);

function eventTitle(event: AssetHistoryEvent, t: TranslateFn): string {
  if (!TRANSLATED_TITLES.has(event.event_type)) return event.title;
  return t(`history.titles.${event.event_type}`, {
    code: event.ref?.entity_code ?? "",
    defaultValue: event.title,
  });
}

/** The status carried by an event, in the vocabulary of the module that owns it. */
function eventStatus(event: AssetHistoryEvent, t: TranslateFn): string | undefined {
  const code = event.status_label;
  if (!code) return undefined;
  const type = event.event_type;
  if (type.startsWith("wo_")) return t(`ot:status.${statusToI18nKey(code)}`);
  if (type.startsWith("di_")) return t(`di:status.${diStatusToI18nKey(code)}`);
  if (type === "pm_scheduled") return t(`pm:occurrenceStatus.${code}`, { defaultValue: code });
  if (type === "pm_executed") return t(`pm:executionResult.${code}`, { defaultValue: code });
  if (type === "meter_reading") return t(`history.quality.${code}`, { defaultValue: code });
  if (type === "inspection_recorded") {
    return t(`history.inspectionResult.${code}`, { defaultValue: code });
  }
  if (type === "failure_recorded") {
    return t(`history.failureStatus.${code}`, { defaultValue: code });
  }
  const assetStatusKey = ASSET_STATUS_LABEL_KEY[code.trim().toUpperCase()];
  return assetStatusKey ? t(assetStatusKey) : code;
}

export function AssetTimelineList({
  events,
  onOpenRef,
  canOpenRef = () => true,
}: {
  events: AssetHistoryEvent[];
  onOpenRef: (ev: AssetHistoryEvent) => void;
  /** False when the user lacks the right of the screen the event opens: shown, not clickable. */
  canOpenRef?: (ev: AssetHistoryEvent) => boolean;
}) {
  const { t, i18n } = useTranslation(["equipment", "ot", "di", "pm"]);
  const tr = t as unknown as TranslateFn;

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((event) => {
        const typeLabel = t(`history.eventTypes.${event.event_type}`, {
          defaultValue: event.event_type,
        });
        const hasRef = Boolean(event.ref?.entity_type);
        const clickable = hasRef && canOpenRef(event);
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

        if (hasRef && !clickable && event.ref?.entity_code) {
          metaBits.push(
            <span key="ref" title={t("history.noRightToOpen")}>
              {event.ref.entity_code}
            </span>,
          );
        }

        return {
          id: event.id,
          title: eventTitle(event, tr),
          description:
            event.description ??
            (typeof event.metadata?.["source_type"] === "string"
              ? tr(`history.failureSource.${event.metadata["source_type"]}`, {
                  defaultValue: event.metadata["source_type"],
                })
              : undefined),
          timestamp: event.occurred_at,
          subtitle: eventStatus(event, tr),
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
    [events, onOpenRef, canOpenRef, t, tr],
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
