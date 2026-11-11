/**
 * WoAuditTimeline.tsx
 *
 * Read-only vertical timeline showing the OT's history: its change events and the audit-trail
 * events whose object is the OT (close-out, imported history), with the fields each one changed
 * ("Date de clôture : — → 28/10/2026 18:26"). Presentation via shared Timeline; fetch/mapping
 * stay here.
 */

import {
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
import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { AuditChangeList } from "@/components/activity/AuditChangeList";
import {
  eventLabel,
  payloadChanges,
  payloadChangesUnavailable,
} from "@/components/activity/activity-labels";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { mfTone } from "@/design-system/tone";
import { useLatestHistory } from "@/hooks/use-latest-history";
import { formatPersonLabel } from "@/lib/display";
import { cn } from "@/lib/utils";
import { type WoChangeEvent, listWoChangeEvents } from "@/services/wo-audit-service";

/** Page of the history and backend maximum (`wo::audit::WO_CHANGE_EVENTS_MAX_LIMIT`). */
const HISTORY_PAGE = 100;
const HISTORY_MAX = 500;

/** Actions recorded by the backend (`wo_change_events.action`) that have a translated label. */
const KNOWN_ACTIONS = new Set([
  "created",
  "updated",
  "submitted",
  "planned",
  "planning_approved",
  "marked_ready",
  "returned_to_planning",
  "assigned",
  "emergency_reassigned",
  "started",
  "paused",
  "held",
  "resumed",
  "mechanically_completed",
  "failure_detail_saved",
  "verification_saved",
  "verified",
  "closed",
  "reopened",
  "cancelled",
  "rca_updated",
  "attachment_uploaded",
  "service_cost_updated",
  "permit_requirement_changed",
  "site_moved",
  "equipment_changed",
  "shutdown_required_set",
  "shutdown_required_cleared",
  "permit_rule_flag_cleared",
  "task_reopened",
  "labor_closed_by_cancellation",
  "labor_closed_by_closeout",
]);

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "planned":
    case "planning_approved":
      return <Calendar className={cn(cls, mfTone.info.text)} />;
    case "assigned":
    case "emergency_reassigned":
      return <User className={cn(cls, mfTone.info.text)} />;
    case "started":
    case "resumed":
      return <Play className={cn(cls, mfTone.progress.text)} />;
    case "paused":
    case "held":
      return <Pause className={cn(cls, mfTone.attention.text)} />;
    case "mechanically_completed":
      return <Wrench className={cn(cls, mfTone.success.text)} />;
    case "verified":
    case "verification_saved":
      return <Check className={cn(cls, mfTone.success.text)} />;
    case "closed":
      return <Lock className={cn(cls, mfTone.neutral.text)} />;
    case "reopened":
      return <PlayCircle className={cn(cls, mfTone.warning.text)} />;
    case "cancelled":
      return <ShieldX className={cn(cls, mfTone.danger.text)} />;
    default:
      return <Dot className={cn(cls, mfTone.neutral.text)} />;
  }
}

function resultTone(result: string): TimelineTone {
  if (result === "applied" || result === "success") return "success";
  if (result === "blocked") return "danger";
  if (result === "partial") return "warning";
  return "default";
}

/** `{"requires_permit": true}` of a permit-requirement change; null when absent. */
function requiresPermitOf(detailsJson: string | null): boolean | null {
  if (!detailsJson) return null;
  try {
    const parsed = JSON.parse(detailsJson) as { requires_permit?: unknown };
    return typeof parsed.requires_permit === "boolean" ? parsed.requires_permit : null;
  } catch {
    return null;
  }
}

/** `{"origin": "depannage"}` of an assignment made by a dépannage (`wo::depannage`); else false. */
function fromDepannage(detailsJson: string | null): boolean {
  if (!detailsJson) return false;
  try {
    const parsed = JSON.parse(detailsJson) as { origin?: unknown };
    return parsed.origin === "depannage";
  } catch {
    return false;
  }
}

interface WoAuditTimelineProps {
  woId: number;
}

export function WoAuditTimeline({ woId }: WoAuditTimelineProps) {
  const { t, i18n } = useTranslation("ot");
  const { t: tActivity } = useTranslation("activity");
  const fetchLatest = useCallback((limit: number) => listWoChangeEvents(woId, limit), [woId]);
  const history = useLatestHistory<WoChangeEvent>(fetchLatest, {
    pageSize: HISTORY_PAGE,
    maxLimit: HISTORY_MAX,
  });
  const events = history.items;

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((evt) => {
        const fromAudit = evt.source === "audit";
        const resultKey =
          evt.apply_result === "applied" || (fromAudit && evt.apply_result === "success")
            ? "audit.applied"
            : evt.apply_result === "blocked"
              ? "audit.blocked"
              : evt.apply_result === "partial"
                ? "audit.partial"
                : null;
        const badges = resultKey
          ? [{ label: t(resultKey), tone: resultTone(evt.apply_result) }]
          : [];
        if (evt.requires_step_up === 1) {
          badges.push({ label: t("audit.stepUp"), tone: "info" as TimelineTone });
        }
        // The stored summary is the backend's own record (English); the screen reads the action
        // in the UI language instead.
        const known = KNOWN_ACTIONS.has(evt.action);
        const permit =
          evt.action === "permit_requirement_changed" ? requiresPermitOf(evt.details_json) : null;
        const changes = payloadChanges(evt.details_json);
        const unavailable = payloadChangesUnavailable(evt.details_json);
        const permitText =
          permit !== null ? t(permit ? "audit.permitRequired" : "audit.permitNotRequired") : null;
        const originText =
          evt.action === "assigned" && fromDepannage(evt.details_json)
            ? t("audit.originDepannage")
            : null;
        // Audit-trail events are named like in the audit log (`activity` labels).
        const title = fromAudit
          ? eventLabel(tActivity, evt.action)
          : known
            ? t(`audit.actions.${evt.action}`)
            : t("audit.actions.other");
        return {
          id: `${evt.source}-${evt.id}`,
          title,
          ...(permitText !== null || originText !== null || changes.length > 0 || unavailable
            ? {
                description: (
                  <>
                    {originText !== null ? <span className="block">{originText}</span> : null}
                    {permitText !== null ? <span className="block">{permitText}</span> : null}
                    <AuditChangeList changes={changes} className="text-xs" />
                    {unavailable ? (
                      <span className="block text-xs text-text-muted">
                        {tActivity("audit.detail.changesUnavailable")}
                      </span>
                    ) : null}
                  </>
                ),
              }
            : {}),
          timestamp: evt.acted_at,
          actor:
            evt.actor_id != null ? formatPersonLabel(evt.actor_display_name) : t("audit.system"),
          icon: actionIcon(evt.action),
          badges,
        };
      }),
    [events, t, tActivity],
  );

  return (
    <Timeline
      items={entries}
      loading={history.loading}
      locale={i18n.language}
      aria-label={t("audit.title")}
      empty={t("audit.empty")}
      error={history.error ?? undefined}
      onRetry={history.reload}
      hasOlder={history.hasOlder}
      {...(history.canLoadOlder ? { onLoadOlder: history.loadOlder } : {})}
      loadingOlder={history.loadingOlder}
    />
  );
}
