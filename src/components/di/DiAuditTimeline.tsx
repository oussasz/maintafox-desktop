/**
 * DiAuditTimeline.tsx
 *
 * Read-only vertical timeline showing DI audit change events.
 * Phase 2 – Sub-phase 04 – File 04 – Sprint S3.
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
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "@/components/ui";
import { cn } from "@/lib/utils";
import { listDiChangeEvents, type DiChangeEvent } from "@/services/di-audit-service";
import { toErrorMessage } from "@/utils/errors";

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiAuditTimelineProps {
  diId: number;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "submit":
      return <ArrowUp className={cn(cls, "text-blue-500")} />;
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

function applyResultBadge(result: string) {
  switch (result) {
    case "applied":
      return <Badge className="bg-green-100 text-green-800 hover:bg-green-100">Applied</Badge>;
    case "blocked":
      return <Badge className="bg-red-100 text-red-800 hover:bg-red-100">Blocked</Badge>;
    case "partial":
      return <Badge className="bg-yellow-100 text-yellow-800 hover:bg-yellow-100">Partial</Badge>;
    default:
      return <Badge variant="outline">{result}</Badge>;
  }
}

function formatActedAt(iso: string): string {
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiAuditTimeline({ diId }: DiAuditTimelineProps) {
  const [events, setEvents] = useState<DiChangeEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

  // ── Loading skeleton ──────────────────────────────────────────────────

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

  // ── Error state ───────────────────────────────────────────────────────

  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
        <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
        <span>Could not load audit trail.</span>
      </div>
    );
  }

  // ── Empty state ───────────────────────────────────────────────────────

  if (events.length === 0) {
    return (
      <p className="py-6 text-center text-sm text-muted-foreground">
        No audit events recorded for this request.
      </p>
    );
  }

  // ── Timeline ──────────────────────────────────────────────────────────

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
              <span className="text-sm font-medium text-text-primary">{evt.action}</span>
              {applyResultBadge(evt.apply_result)}
              {evt.requires_step_up === 1 && (
                <Badge className="bg-blue-100 text-blue-800 hover:bg-blue-100">Step-up</Badge>
              )}
            </div>

            {evt.summary && <p className="mt-1 text-sm text-muted-foreground">{evt.summary}</p>}

            <p className="mt-1 text-xs text-muted-foreground">
              {evt.actor_id != null ? `User #${evt.actor_id}` : "System"} ·{" "}
              {formatActedAt(evt.acted_at)}
            </p>
          </div>
        </div>
      ))}
    </div>
  );
}
