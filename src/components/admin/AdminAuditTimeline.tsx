/**
 * AdminAuditTimeline.tsx
 *
 * Read-only vertical timeline showing admin governance audit events.
 * Fetches from the `list_admin_events` IPC command via rbac-service.
 * Mirrors the WoAuditTimeline / DiAuditTimeline pattern.
 *
 * Phase 2 – Sub-phase 06 – File 04.
 */

import {
  AlertCircle,
  ArrowLeftRight,
  ChevronDown,
  ChevronRight,
  Dot,
  Key,
  PencilLine,
  Shield,
  User,
  Zap,
  ZapOff,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { listAdminEvents } from "@/services/rbac-service";
import { toErrorMessage } from "@/utils/errors";
import type { AdminChangeEventDetail, AdminEventFilter } from "@shared/ipc-types";

// ── Helpers ───────────────────────────────────────────────────────────────────

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

function formatActedAt(iso: string, locale: string): string {
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

// ── Props ─────────────────────────────────────────────────────────────────────

interface AdminAuditTimelineProps {
  filter?: Partial<AdminEventFilter>;
}

// ── Component ─────────────────────────────────────────────────────────────────

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
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  // ── Loading skeleton ────────────────────────────────────────────────────

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

  // ── Error state ─────────────────────────────────────────────────────────

  if (error) {
    return (
      <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
        <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
        <span>
          {t("audit.loadError", "Impossible de charger l'historique d'audit.")} {error}
        </span>
      </div>
    );
  }

  // ── Empty state ─────────────────────────────────────────────────────────

  if (events.length === 0) {
    return (
      <p className="py-6 text-center text-sm text-muted-foreground">
        {t("audit.empty", "Aucun événement d'audit enregistré.")}
      </p>
    );
  }

  // ── Timeline ────────────────────────────────────────────────────────────

  return (
    <div className="space-y-0">
      {events.map((evt, idx) => {
        const isBlocked = evt.apply_result === "blocked";
        const isDiffExpanded = expandedDiffs.has(evt.id);
        const target = targetLabel(evt);

        return (
          <div
            key={evt.id}
            className={cn(
              "relative flex gap-3 pb-6 last:pb-0",
              isBlocked && "rounded-md bg-red-50 dark:bg-red-950/20 px-2 py-2",
            )}
          >
            {/* Vertical connector line */}
            {idx < events.length - 1 && (
              <div className="absolute left-4 top-8 h-full w-px bg-surface-border" />
            )}

            {/* Icon circle */}
            <div className="z-10 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-surface-border bg-background">
              {actionIcon(evt.action)}
            </div>

            {/* Content */}
            <div className="min-w-0 flex-1 pt-0.5">
              {/* First line: action + badges */}
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium capitalize text-text-primary">
                  {evt.action.replace(/_/g, " ")}
                </span>
                {applyResultBadge(evt.apply_result, t)}
                {evt.step_up_used && (
                  <Badge className="bg-green-100 text-green-800 hover:bg-green-100 text-[10px] px-1.5 py-0">
                    {t("audit.stepUp", "Step-up")}
                  </Badge>
                )}
              </div>

              {/* Target user/role */}
              {target && (
                <p className="mt-0.5 text-sm text-text-secondary">
                  → {target}
                </p>
              )}

              {/* Scope */}
              {evt.scope_type && (
                <p className="text-xs text-muted-foreground">
                  {t("audit.scope", "Scope")}: {evt.scope_type}
                  {evt.scope_reference ? ` / ${evt.scope_reference}` : ""}
                </p>
              )}

              {/* Summary */}
              {evt.summary && (
                <p className="mt-1 text-sm text-muted-foreground">{evt.summary}</p>
              )}

              {/* Actor + timestamp */}
              <p className="mt-1 text-xs text-muted-foreground">
                {evt.actor_username
                  ? evt.actor_username
                  : evt.actor_id != null
                    ? t("audit.user", { id: evt.actor_id })
                    : t("audit.system", "Système")}{" "}
                · {formatActedAt(evt.acted_at, i18n.language)}
              </p>

              {/* Diff JSON collapsible */}
              {evt.diff_json && (
                <div className="mt-2">
                  <button
                    type="button"
                    onClick={() => toggleDiff(evt.id)}
                    className="flex items-center gap-1 text-xs font-medium text-primary hover:underline"
                  >
                    {isDiffExpanded ? (
                      <ChevronDown className="h-3 w-3" />
                    ) : (
                      <ChevronRight className="h-3 w-3" />
                    )}
                    {t("audit.viewDiff", "Voir le diff")}
                  </button>
                  {isDiffExpanded && (
                    <pre className="mt-1 max-h-64 overflow-auto rounded-md border border-surface-border bg-surface-2 p-3 text-xs text-text-secondary">
                      {formatDiffJson(evt.diff_json)}
                    </pre>
                  )}
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
