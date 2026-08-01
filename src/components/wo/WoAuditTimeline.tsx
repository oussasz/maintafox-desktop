/**
 * WoAuditTimeline.tsx
 *
 * Read-only vertical timeline showing WO audit change events.
 * Presentation via shared Timeline; fetch/mapping stay here.
 */

import {
  AlertCircle,
  Calendar,
  Check,
  Dot,
  Lock,
  Pause,
  Play,
  PlayCircle,
  ShieldX,
  User,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { listWoChangeEvents, type WoChangeEvent } from "@/services/wo-audit-service";
import { toErrorMessage } from "@/utils/errors";

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "planned":
      return <Calendar className={cn(cls, "text-blue-500")} />;
    case "assigned":
      return <User className={cn(cls, "text-indigo-500")} />;
    case "started":
      return <Play className={cn(cls, "text-green-600")} />;
    case "paused":
      return <Pause className={cn(cls, "text-orange-500")} />;
    case "resumed":
      return <PlayCircle className={cn(cls, "text-green-500")} />;
    case "mech_completed":
      return <Wrench className={cn(cls, "text-teal-500")} />;
    case "verified":
      return <Check className={cn(cls, "text-emerald-600")} />;
    case "closed":
      return <Lock className={cn(cls, "text-neutral-500")} />;
    case "reopened":
      return <PlayCircle className={cn(cls, "text-amber-500")} />;
    case "blocked":
      return <ShieldX className={cn(cls, "text-red-600")} />;
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

interface WoAuditTimelineProps {
  woId: number;
}

export function WoAuditTimeline({ woId }: WoAuditTimelineProps) {
  const { t, i18n } = useTranslation("ot");
  const [events, setEvents] = useState<WoChangeEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadEvents = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listWoChangeEvents(woId, 100);
      setEvents(items);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [woId]);

  useEffect(() => {
    void loadEvents();
  }, [loadEvents]);

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((evt) => {
        const resultKey =
          evt.apply_result === "applied"
            ? "audit.applied"
            : evt.apply_result === "blocked"
              ? "audit.blocked"
              : evt.apply_result === "partial"
                ? "audit.partial"
                : null;
        const badges = [
          {
            label: resultKey ? t(resultKey) : evt.apply_result,
            tone: resultTone(evt.apply_result),
          },
        ];
        if (evt.requires_step_up === 1) {
          badges.push({ label: t("audit.stepUp"), tone: "info" as TimelineTone });
        }
        return {
          id: String(evt.id),
          title: evt.action.replace(/_/g, " "),
          description: evt.summary ?? undefined,
          timestamp: evt.acted_at,
          actor:
            evt.actor_id != null ? formatPersonLabel(evt.actor_display_name) : t("audit.system"),
          icon: actionIcon(evt.action),
          badges,
        };
      }),
    [events, t],
  );

  return (
    <Timeline
      items={entries}
      loading={loading}
      locale={i18n.language}
      aria-label={t("audit.title", { defaultValue: "Work order audit" })}
      empty={t("audit.empty")}
      error={
        error ? (
          <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>
              {t("audit.loadError")}
              {error}
            </span>
          </div>
        ) : null
      }
    />
  );
}
