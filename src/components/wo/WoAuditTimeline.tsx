/**
 * WoAuditTimeline.tsx
 *
 * Read-only vertical timeline showing WO audit change events.
 * Fetches from the `list_wo_change_events` IPC command via wo-audit-service.
 * Mirrors the DiAuditTimeline pattern from SP04.
 *
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S3.
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
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { listWoChangeEvents, type WoChangeEvent } from "@/services/wo-audit-service";
import { toErrorMessage } from "@/utils/errors";

// ── Helpers ───────────────────────────────────────────────────────────────────

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

function applyResultBadge(result: string, t: (key: string) => string) {
  switch (result) {
    case "applied":
      return (
        <Badge className="bg-green-100 text-green-800 hover:bg-green-100 text-[10px] px-1.5 py-0">
          {t("audit.applied")}
        </Badge>
      );
    case "blocked":
      return (
        <Badge className="bg-red-100 text-red-800 hover:bg-red-100 text-[10px] px-1.5 py-0">
          {t("audit.blocked")}
        </Badge>
      );
    case "partial":
      return (
        <Badge className="bg-yellow-100 text-yellow-800 hover:bg-yellow-100 text-[10px] px-1.5 py-0">
          {t("audit.partial")}
        </Badge>
      );
    default:
      return (
        <Badge variant="outline" className="text-[10px] px-1.5 py-0">
          {result}
        </Badge>
      );
  }
}

function formatActedAt(iso: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, {
      day: "2-digit",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoAuditTimelineProps {
  woId: number;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoAuditTimeline({ woId }: WoAuditTimelineProps) {
  const { t } = useTranslation("ot");
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

  // ── Loading skeleton ──────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="space-y-4 py-4">
        {Array.from({ length: 4 }).map((_, i) => (
          <div key={i} className="flex items-start gap-3">
            <div className="h-8 w-8 animate-pulse rounded-full bg-muted" />
            <div className="flex-1 space-y-2">
              <div className="h-4 w-3/4 animate-pulse rounded bg-muted" />
              <div className="h-3 w-1/2 animate-pulse rounded bg-muted" />
            </div>
          </div>
        ))}
      </div>
    );
  }

  // ── Error state ───────────────────────────────────────────────────────────

  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
        <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
        <span>
          {t("audit.loadError")}
          {error}
        </span>
      </div>
    );
  }

  // ── Empty state ───────────────────────────────────────────────────────────

  if (events.length === 0) {
    return <p className="py-6 text-center text-sm text-muted-foreground">{t("audit.empty")}</p>;
  }

  // ── Timeline ──────────────────────────────────────────────────────────────

  return (
    <div className="space-y-0">
      {events.map((evt, idx) => (
        <div key={evt.id} className="relative flex gap-3 pb-6 last:pb-0">
          {/* Vertical line */}
          {idx < events.length - 1 && (
            <div className="absolute left-4 top-8 h-full w-px bg-surface-border" />
          )}

          {/* Icon circle */}
          <div className="z-10 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-surface-border bg-background">
            {actionIcon(evt.action)}
          </div>

          {/* Content */}
          <div className="min-w-0 flex-1 pt-0.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-sm font-medium capitalize text-text-primary">
                {evt.action.replace(/_/g, " ")}
              </span>
              {applyResultBadge(evt.apply_result, t)}
              {evt.requires_step_up === 1 && (
                <Badge className="bg-blue-100 text-blue-800 hover:bg-blue-100 text-[10px] px-1.5 py-0">
                  {t("audit.stepUp")}
                </Badge>
              )}
            </div>

            {evt.summary && <p className="mt-1 text-sm text-muted-foreground">{evt.summary}</p>}

            <p className="mt-1 text-xs text-muted-foreground">
              {evt.actor_id != null ? t("audit.user", { id: evt.actor_id }) : t("audit.system")} ·{" "}
              {formatActedAt(evt.acted_at)}
            </p>
          </div>
        </div>
      ))}
    </div>
  );
}
