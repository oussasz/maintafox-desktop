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
  AlertCircle,
  ListFilter,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { listDiChangeEvents, type DiChangeEvent } from "@/services/di-audit-service";
import { toErrorMessage } from "@/utils/errors";
import { intlLocaleForLanguage } from "@/utils/format-date";

interface DiAuditTimelineProps {
  diId: number;
}

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "submit":
    case "intake_submitted":
      return <ArrowUp className={cn(cls, "text-blue-500")} />;
    case "triage_submitted":
    case "triage_accept":
      return <ListFilter className={cn(cls, "text-sky-600")} />;
    case "screened":
      return <Eye className={cn(cls, "text-indigo-500")} />;
    case "approved":
      return <Check className={cn(cls, "text-green-600")} />;
    case "rejected":
      return <XCircle className={cn(cls, "text-red-500")} />;
    case "converted":
      return <Zap className={cn(cls, "text-amber-500")} />;
    case "blocked":
      return <ShieldX className={cn(cls, "text-red-600")} />;
    case "deferred":
      return <Clock className={cn(cls, "text-orange-500")} />;
    case "returned":
      return <CornerUpLeft className={cn(cls, "text-yellow-600")} />;
    default:
      return <Dot className={cn(cls, "text-muted-foreground")} />;
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
  const [events, setEvents] = useState<DiChangeEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

  const loadEvents = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listDiChangeEvents(diId, 50);
      setEvents(items);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [diId]);

  useEffect(() => {
    void loadEvents();
  }, [loadEvents]);

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
          description: evt.summary ?? undefined,
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
      loading={loading}
      locale={locale}
      aria-label={t("auditTimeline.title", { defaultValue: "DI audit timeline" })}
      empty={t("auditTimeline.empty")}
      error={
        error ? (
          <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{t("auditTimeline.loadError")}</span>
          </div>
        ) : null
      }
    />
  );
}
