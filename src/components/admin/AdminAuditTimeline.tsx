/**
 * AdminAuditTimeline.tsx
 *
 * Read-only vertical timeline showing admin governance audit events.
 * Presentation via shared Timeline; fetch + diff expand stay here.
 */

import {
  AlertCircle,
  ArrowLeftRight,
  Dot,
  Key,
  PencilLine,
  Shield,
  User,
  Zap,
  ZapOff,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { cn } from "@/lib/utils";
import { listAdminEvents } from "@/services/rbac-service";
import { toErrorMessage } from "@/utils/errors";
import type { AdminChangeEventDetail, AdminEventFilter } from "@shared/ipc-types";

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "user_created":
    case "user_deactivated":
      return <User className={cn(cls, "text-blue-500")} />;
    case "role_assigned":
    case "role_revoked":
      return <Shield className={cn(cls, "text-indigo-500")} />;
    case "role_created":
    case "role_updated":
    case "role_deleted":
    case "role_retired":
      return <PencilLine className={cn(cls, "text-amber-600")} />;
    case "emergency_grant_created":
      return <Zap className={cn(cls, "text-orange-500")} />;
    case "emergency_grant_expired":
    case "emergency_grant_revoked":
      return <ZapOff className={cn(cls, "text-red-500")} />;
    case "delegation_policy_created":
    case "delegation_policy_updated":
    case "delegation_policy_deleted":
      return <Key className={cn(cls, "text-purple-500")} />;
    case "role_imported":
    case "role_exported":
      return <ArrowLeftRight className={cn(cls, "text-teal-500")} />;
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

function formatDiffJson(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function targetLabel(evt: AdminChangeEventDetail): string | null {
  const parts: string[] = [];
  if (evt.target_username) parts.push(evt.target_username);
  if (evt.target_role_name) parts.push(evt.target_role_name);
  return parts.length > 0 ? parts.join(" · ") : null;
}

interface AdminAuditTimelineProps {
  filter?: Partial<AdminEventFilter>;
}

export function AdminAuditTimeline({ filter }: AdminAuditTimelineProps) {
  const { t, i18n } = useTranslation("admin");
  const [events, setEvents] = useState<AdminChangeEventDetail[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());

  const loadEvents = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const items = await listAdminEvents({
        limit: 200,
        ...filter,
      });
      setEvents(items);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [filter]);

  useEffect(() => {
    void loadEvents();
  }, [loadEvents]);

  const toggleDiff = (id: number) => {
    setExpandedDiffs((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const entries: TimelineEntry[] = useMemo(
    () =>
      events.map((evt) => {
        const isBlocked = evt.apply_result === "blocked";
        const target = targetLabel(evt);
        const badges = [
          {
            label:
              evt.apply_result === "applied"
                ? t("audit.applied")
                : evt.apply_result === "blocked"
                  ? t("audit.blocked")
                  : evt.apply_result === "partial"
                    ? t("audit.partial")
                    : evt.apply_result,
            tone: resultTone(evt.apply_result),
          },
        ];
        if (evt.step_up_used) {
          badges.push({
            label: t("audit.stepUp", "Step-up"),
            tone: "success" as TimelineTone,
          });
        }

        const subtitleParts: string[] = [];
        if (target) subtitleParts.push(`→ ${target}`);
        if (evt.scope_type) {
          subtitleParts.push(
            `${t("audit.scope", "Scope")}: ${evt.scope_type}${
              evt.scope_reference ? ` / ${evt.scope_reference}` : ""
            }`,
          );
        }

        return {
          id: String(evt.id),
          title: evt.action.replace(/_/g, " "),
          subtitle: subtitleParts.length ? subtitleParts.join(" · ") : undefined,
          description: evt.summary ?? undefined,
          timestamp: evt.acted_at,
          actor: evt.actor_username
            ? evt.actor_username
            : evt.actor_id != null
              ? t("audit.user", { id: evt.actor_id })
              : t("audit.system", "Système"),
          icon: actionIcon(evt.action),
          badges,
          tone: isBlocked ? ("danger" as TimelineTone) : undefined,
          details: evt.diff_json ? (
            <pre className="mt-1 max-h-64 overflow-auto rounded-md border border-surface-border bg-surface-2 p-3 text-xs text-text-secondary">
              {formatDiffJson(evt.diff_json)}
            </pre>
          ) : undefined,
          detailsExpanded: expandedDiffs.has(evt.id),
          onDetailsToggle: () => toggleDiff(evt.id),
          detailsLabel: t("audit.viewDiff", "Voir le diff"),
        };
      }),
    [events, expandedDiffs, t],
  );

  return (
    <Timeline
      items={entries}
      loading={loading}
      locale={i18n.language}
      aria-label={t("audit.title", { defaultValue: "Admin audit timeline" })}
      empty={t("audit.empty", "Aucun événement d'audit enregistré.")}
      error={
        error ? (
          <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>
              {t("audit.loadError", "Impossible de charger l'historique d'audit.")} {error}
            </span>
          </div>
        ) : null
      }
    />
  );
}
