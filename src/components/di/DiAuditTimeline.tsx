/**
 * DiAuditTimeline.tsx
 *
 * Vertical timeline of DI change/audit events (not the formal state log).
 * Presentation via shared Timeline; fetch/mapping stay here.
 */

import {
  ArrowUp,
  Eye,
  Check,
  XCircle,
  Zap,
  ShieldX,
  Clock,
  CornerUpLeft,
  Dot,
  ListFilter,
} from "lucide-react";
import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { mfTone } from "@/design-system/tone";
import { useLatestHistory } from "@/hooks/use-latest-history";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { listDiChangeEvents, type DiChangeEvent } from "@/services/di-audit-service";
import { intlLocaleForLanguage } from "@/utils/format-date";

/** Page of the history and backend maximum (`di::audit::DI_CHANGE_EVENTS_MAX_LIMIT`). */
const HISTORY_PAGE = 50;
const HISTORY_MAX = 500;

interface DiAuditTimelineProps {
  diId: number;
}

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "submit":
    case "intake_submitted":
      return <ArrowUp className={cn(cls, mfTone.info.text)} />;
    case "triage_submitted":
    case "triage_accept":
      return <ListFilter className={cn(cls, mfTone.info.text)} />;
    case "screened":
      return <Eye className={cn(cls, mfTone.progress.text)} />;
    case "approved":
      return <Check className={cn(cls, mfTone.success.text)} />;
    case "rejected":
      return <XCircle className={cn(cls, mfTone.danger.text)} />;
    case "converted":
      return <Zap className={cn(cls, mfTone.success.text)} />;
    case "blocked":
      return <ShieldX className={cn(cls, mfTone.danger.text)} />;
    case "deferred":
      return <Clock className={cn(cls, mfTone.neutral.text)} />;
    case "returned":
      return <CornerUpLeft className={cn(cls, mfTone.warning.text)} />;
    default:
      return <Dot className={cn(cls, mfTone.neutral.text)} />;
  }
}

function resultTone(result: string): TimelineTone {
  if (result === "applied") return "success";
  if (result === "blocked") return "danger";
  if (result === "partial") return "warning";
  return "default";
}

export function DiAuditTimeline({ diId }: DiAuditTimelineProps) {
  const { t, i18n } = useTranslation("di");
  const locale = intlLocaleForLanguage(i18n.language);
  const fetchLatest = useCallback((limit: number) => listDiChangeEvents(diId, limit), [diId]);
  const history = useLatestHistory<DiChangeEvent>(fetchLatest, {
    pageSize: HISTORY_PAGE,
    maxLimit: HISTORY_MAX,
  });
  const events = history.items;

  const actionLabel = (action: string) =>
    t(`auditTimeline.actionLabel.${action}` as "auditTimeline.actionLabel.screened", {
      defaultValue: action,
    });

  const resultLabel = (result: string) => {
    switch (result) {
      case "applied":
        return t("auditTimeline.resultApplied");
      case "blocked":
        return t("auditTimeline.resultBlocked");
      case "partial":
        return t("auditTimeline.resultPartial");
      default:
        return result;
    }
  };

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((evt) => {
        const badges = [
          { label: resultLabel(evt.apply_result), tone: resultTone(evt.apply_result) },
        ];
        if (evt.requires_step_up === 1) {
          badges.push({ label: t("auditTimeline.stepUp"), tone: "info" as TimelineTone });
        }
        return {
          id: String(evt.id),
          title: actionLabel(evt.action),
          // The stored summary is the backend's English record; known actions read in the UI language.
          description:
            t(`auditTimeline.summary.${evt.action}`, { defaultValue: evt.summary ?? "" }) ||
            undefined,
          timestamp: evt.acted_at,
          actor:
            evt.actor_id != null
              ? formatPersonLabel(evt.actor_display_name)
              : t("auditTimeline.actorSystem"),
          icon: actionIcon(evt.action),
          badges,
        };
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- i18n via t/locale
    [events, t],
  );

  return (
    <Timeline
      items={entries}
      loading={history.loading}
      locale={locale}
      aria-label={t("auditTimeline.title")}
      empty={t("auditTimeline.empty")}
      error={history.error ?? undefined}
      onRetry={history.reload}
      hasOlder={history.hasOlder}
      {...(history.canLoadOlder ? { onLoadOlder: history.loadOlder } : {})}
      loadingOlder={history.loadingOlder}
    />
  );
}
