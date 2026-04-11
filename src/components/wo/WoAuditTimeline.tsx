/**
 * WoAuditTimeline.tsx
 *
 * Vertical timeline showing all state transitions for a work order.
 * Reads transitions from the wo-store (activeWo.transitions).
 * Falls back to a minimal 3-date summary when no transitions exist.
 *
 * GA-010 — Sprint S5.
 */

import { Calendar } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { useWoStore } from "@/stores/wo-store";
import type { WorkOrder } from "@shared/ipc-types";

// ── Status → badge colour ───────────────────────────────────────────────────

const STATUS_STYLE: Record<string, string> = {
  draft: "bg-gray-100 text-gray-600",
  planned: "bg-blue-100 text-blue-800",
  released: "bg-sky-100 text-sky-800",
  ready_to_schedule: "bg-indigo-100 text-indigo-800",
  assigned: "bg-violet-100 text-violet-800",
  in_progress: "bg-amber-100 text-amber-800",
  on_hold: "bg-orange-100 text-orange-800",
  paused: "bg-orange-100 text-orange-800",
  mechanically_complete: "bg-teal-100 text-teal-800",
  technically_verified: "bg-emerald-100 text-emerald-800",
  completed: "bg-green-100 text-green-800",
  verified: "bg-teal-100 text-teal-800",
  closed: "bg-neutral-100 text-neutral-500",
  cancelled: "bg-red-100 text-red-700",
};

function statusBadge(status: string) {
  const cls = STATUS_STYLE[status] ?? "bg-gray-100 text-gray-600";
  return <Badge className={`text-[10px] px-1.5 py-0 ${cls}`}>{status.replace(/_/g, " ")}</Badge>;
}

function fmtDateTime(iso: string, locale: string): string {
  try {
    return new Date(iso).toLocaleString(locale, {
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

function fmtDate(iso: string, locale: string): string {
  try {
    return new Date(iso).toLocaleDateString(locale, {
      day: "2-digit",
      month: "short",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}

// ── Props ───────────────────────────────────────────────────────────────────

interface WoAuditTimelineProps {
  wo: WorkOrder;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoAuditTimeline({ wo }: WoAuditTimelineProps) {
  const { t, i18n } = useTranslation("ot");
  const transitions = useWoStore((s) => s.activeWo?.transitions ?? []);
  const locale = i18n.language;

  // ── Fallback: no transitions available ──
  if (transitions.length === 0) {
    return (
      <div className="space-y-2">
        <h4 className="text-sm font-semibold">{t("detail.sections.history")}</h4>
        <div className="text-xs text-muted-foreground space-y-1">
          <p>
            <Calendar className="inline h-3 w-3 mr-1" />
            {t("audit.created")}: {fmtDate(wo.created_at, locale)}
          </p>
          <p>
            <Calendar className="inline h-3 w-3 mr-1" />
            {t("audit.updated")}: {fmtDate(wo.updated_at, locale)}
          </p>
          {wo.closed_at && (
            <p>
              <Calendar className="inline h-3 w-3 mr-1" />
              {t("audit.closed")}: {fmtDate(wo.closed_at, locale)}
            </p>
          )}
        </div>
      </div>
    );
  }

  // ── Full timeline ──
  return (
    <div className="space-y-3">
      <h4 className="text-sm font-semibold">{t("detail.sections.history")}</h4>

      <ol className="relative border-l border-muted-foreground/20 ml-2 space-y-4">
        {transitions.map((tr) => (
          <li key={tr.id} className="ml-4">
            {/* Dot */}
            <span className="absolute -left-1.5 mt-1.5 h-3 w-3 rounded-full border-2 border-background bg-muted-foreground/40" />

            <div className="flex flex-wrap items-center gap-1.5 text-xs">
              {statusBadge(tr.from_status)} <span className="text-muted-foreground">→</span>{" "}
              {statusBadge(tr.to_status)}
              <span className="text-muted-foreground ml-auto">
                {fmtDateTime(tr.acted_at, locale)}
              </span>
            </div>

            {/* Action & actor */}
            <p className="text-xs text-muted-foreground mt-0.5">
              <span className="font-medium capitalize">{tr.action.replace(/_/g, " ")}</span>
              {tr.actor_id != null && (
                <span className="ml-1">
                  — {t("audit.actor")} #{tr.actor_id}
                </span>
              )}
            </p>

            {/* Notes / reason */}
            {(tr.notes || tr.reason_code) && (
              <p className="text-xs text-muted-foreground/80 italic mt-0.5">
                {tr.reason_code && <span>[{tr.reason_code}] </span>}
                {tr.notes}
              </p>
            )}
          </li>
        ))}
      </ol>
    </div>
  );
}
