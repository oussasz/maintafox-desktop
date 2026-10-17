/**
 * AdminAuditTimeline.tsx
 *
 * Read-only timeline of the admin change ledger (`admin_change_events`): who changed which
 * account, role or grant, when, with which outcome. Filters (action, actor, target user, target
 * role, date range, outcome) run on the server with the caller's visibility; the same filter
 * exports to CSV. `fixedFilter` pins the timeline to one user or role (user detail history).
 */

import {
  AlertCircle,
  ArrowLeftRight,
  Dot,
  Download,
  Key,
  Lock,
  LockOpen,
  PencilLine,
  RotateCcw,
  Shield,
  User,
  Zap,
  ZapOff,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  exportAdminEventsCsv,
  listAdminEventFacets,
  listAdminEvents,
} from "@/services/rbac-service";
import type { AdminChangeEventDetail, AdminEventFacets, AdminEventFilter } from "@shared/ipc-types";

const ALL = "__all__";
const PAGE_SIZE = 200;

function actionIcon(action: string) {
  const cls = "h-4 w-4";
  switch (action) {
    case "user_created":
    case "user_updated":
    case "user_deactivated":
    case "user_reactivated":
      return <User className={cn(cls, "text-primary")} />;
    case "account_locked":
      return <Lock className={cn(cls, "text-accent")} />;
    case "account_unlocked":
      return <LockOpen className={cn(cls, "text-primary")} />;
    case "password_reset_by_admin":
      return <RotateCcw className={cn(cls, "text-primary")} />;
    case "role_assigned":
    case "role_revoked":
      return <Shield className={cn(cls, "text-primary")} />;
    case "role_created":
    case "role_updated":
    case "role_deleted":
    case "role_retired":
    case "permission_created":
      return <PencilLine className={cn(cls, "text-primary")} />;
    case "emergency_grant_created":
      return <Zap className={cn(cls, "text-accent")} />;
    case "emergency_grant_expired":
    case "emergency_grant_revoked":
      return <ZapOff className={cn(cls, "text-text-secondary")} />;
    case "delegation_policy_created":
    case "delegation_policy_updated":
    case "delegation_policy_deleted":
      return <Key className={cn(cls, "text-primary")} />;
    case "role_imported":
    case "role_exported":
      return <ArrowLeftRight className={cn(cls, "text-primary")} />;
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

function downloadCsv(fileName: string, data: string) {
  // BOM so spreadsheet tools open accented names correctly.
  const blob = new Blob([`﻿${data}`], { type: "text/csv;charset=utf-8;" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

interface AdminAuditTimelineProps {
  /** Filter that the viewer cannot change (e.g. one user's history). */
  fixedFilter?: Partial<AdminEventFilter>;
  /** Hide the filter bar (embedded history). */
  compact?: boolean;
}

interface FilterState {
  action: string;
  actor: string;
  targetUser: string;
  targetRole: string;
  result: string;
  dateFrom: string;
  dateTo: string;
}

const EMPTY_FILTER: FilterState = {
  action: ALL,
  actor: ALL,
  targetUser: ALL,
  targetRole: ALL,
  result: ALL,
  dateFrom: "",
  dateTo: "",
};

export function AdminAuditTimeline({ fixedFilter, compact = false }: AdminAuditTimelineProps) {
  const { t, i18n } = useTranslation("admin");
  const { t: tShell } = useTranslation("shell");
  const [events, setEvents] = useState<AdminChangeEventDetail[]>([]);
  const [facets, setFacets] = useState<AdminEventFacets | null>(null);
  const [facetsError, setFacetsError] = useState<unknown>(null);
  const [filter, setFilter] = useState<FilterState>(EMPTY_FILTER);
  const [loading, setLoading] = useState(true);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expandedDiffs, setExpandedDiffs] = useState<Set<number>>(new Set());

  const actionLabel = useCallback(
    (action: string) => t(`console.actions.${action}`, action.replace(/_/g, " ")),
    [t],
  );
  const resultLabel = useCallback((result: string) => t(`console.results.${result}`, result), [t]);

  const query: AdminEventFilter = useMemo(() => {
    const q: AdminEventFilter = {};
    if (filter.action !== ALL) q.action = filter.action;
    if (filter.actor !== ALL) q.actor_id = Number(filter.actor);
    if (filter.targetUser !== ALL) q.target_user_id = Number(filter.targetUser);
    if (filter.targetRole !== ALL) q.target_role_id = Number(filter.targetRole);
    if (filter.result !== ALL) q.apply_result = filter.result;
    if (filter.dateFrom) q.date_from = filter.dateFrom;
    if (filter.dateTo) q.date_to = filter.dateTo;
    return { ...q, ...fixedFilter };
  }, [filter, fixedFilter]);

  const rangeInvalid =
    filter.dateFrom !== "" && filter.dateTo !== "" && filter.dateFrom > filter.dateTo;

  const loadEvents = useCallback(async () => {
    if (rangeInvalid) return;
    try {
      setLoading(true);
      setError(null);
      setEvents(await listAdminEvents({ ...query, limit: PAGE_SIZE }));
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setLoading(false);
    }
  }, [query, rangeInvalid]);

  useEffect(() => {
    void loadEvents();
  }, [loadEvents]);

  const loadFacets = useCallback(async () => {
    setFacetsError(null);
    try {
      setFacets(await listAdminEventFacets());
    } catch (err) {
      setFacets(null);
      setFacetsError(err);
    }
  }, []);

  useEffect(() => {
    if (compact) return;
    void loadFacets();
  }, [compact, loadFacets]);

  const handleExport = async () => {
    setExporting(true);
    try {
      const csv = await exportAdminEventsCsv(query);
      const stamp = new Date().toISOString().slice(0, 10);
      downloadCsv(`admin-audit-${stamp}.csv`, csv);
    } catch {
      // The IPC layer reports the error.
    } finally {
      setExporting(false);
    }
  };

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
          { label: resultLabel(evt.apply_result), tone: resultTone(evt.apply_result) },
        ];
        if (evt.step_up_used) {
          badges.push({ label: t("audit.stepUp"), tone: "success" as TimelineTone });
        }

        const subtitleParts: string[] = [];
        if (target) subtitleParts.push(`→ ${target}`);
        if (evt.scope_type) {
          subtitleParts.push(
            t("audit.scopeValue", {
              scope: `${evt.scope_type}${
                evt.scope_reference && !evt.from_other_device ? ` / ${evt.scope_reference}` : ""
              }`,
            }),
          );
        }
        if (evt.from_other_device) {
          subtitleParts.push(tShell("sync.history.fromOtherDevice"));
        }

        return {
          id: String(evt.id),
          title: actionLabel(evt.action),
          subtitle: subtitleParts.length ? subtitleParts.join(" · ") : undefined,
          description: evt.summary ?? undefined,
          timestamp: evt.acted_at,
          actor: evt.actor_username
            ? evt.actor_username
            : evt.actor_id != null
              ? t("audit.user", { id: evt.actor_id })
              : t("audit.system"),
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
          detailsLabel: t("audit.viewDiff"),
        };
      }),
    [events, expandedDiffs, t, tShell, actionLabel, resultLabel],
  );

  const set = (patch: Partial<FilterState>) => setFilter((f) => ({ ...f, ...patch }));
  const filtered = JSON.stringify(filter) !== JSON.stringify(EMPTY_FILTER);

  const facetSelect = (
    id: string,
    label: string,
    value: string,
    onChange: (v: string) => void,
    options: { value: string; label: string }[],
  ) => (
    <div className="min-w-[10rem] flex-1 space-y-1">
      <Label htmlFor={id} className="text-xs text-text-secondary">
        {label}
      </Label>
      {/* The filter choices come from the ledger: disabled until they are loaded. */}
      <Select value={value} onValueChange={onChange} disabled={facets == null}>
        <SelectTrigger id={id} className="h-9">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ALL}>{t("console.audit.all")}</SelectItem>
          {options.map((o) => (
            <SelectItem key={o.value} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );

  return (
    <div className="space-y-4">
      {!compact && facetsError != null ? (
        <ErrorState
          error={facetsError}
          title={t("console.audit.facetsLoadError")}
          onRetry={() => void loadFacets()}
          className="py-4"
        />
      ) : null}
      {!compact ? (
        <div className="space-y-3 rounded-lg border border-surface-border bg-surface-0 p-3">
          <div className="flex flex-wrap items-end gap-3">
            {facetSelect(
              "audit-action",
              t("console.audit.action"),
              filter.action,
              (v) => set({ action: v }),
              (facets?.actions ?? []).map((a) => ({ value: a, label: actionLabel(a) })),
            )}
            {facetSelect(
              "audit-actor",
              t("console.audit.actor"),
              filter.actor,
              (v) => set({ actor: v }),
              (facets?.actors ?? []).map((a) => ({
                value: String(a.id),
                label: `${a.label} (${a.count})`,
              })),
            )}
            {facetSelect(
              "audit-target-user",
              t("console.audit.targetUser"),
              filter.targetUser,
              (v) => set({ targetUser: v }),
              (facets?.target_users ?? []).map((a) => ({
                value: String(a.id),
                label: `${a.label} (${a.count})`,
              })),
            )}
            {facetSelect(
              "audit-target-role",
              t("console.audit.targetRole"),
              filter.targetRole,
              (v) => set({ targetRole: v }),
              (facets?.target_roles ?? []).map((a) => ({
                value: String(a.id),
                label: `${a.label} (${a.count})`,
              })),
            )}
          </div>
          <div className="flex flex-wrap items-end gap-3">
            {facetSelect(
              "audit-result",
              t("console.audit.result"),
              filter.result,
              (v) => set({ result: v }),
              (facets?.apply_results ?? []).map((r) => ({ value: r, label: resultLabel(r) })),
            )}
            <div className="space-y-1">
              <Label htmlFor="audit-from" className="text-xs text-text-secondary">
                {t("console.audit.from")}
              </Label>
              <Input
                id="audit-from"
                type="date"
                className="h-9"
                value={filter.dateFrom}
                onChange={(e) => set({ dateFrom: e.target.value })}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="audit-to" className="text-xs text-text-secondary">
                {t("console.audit.to")}
              </Label>
              <Input
                id="audit-to"
                type="date"
                className="h-9"
                value={filter.dateTo}
                onChange={(e) => set({ dateTo: e.target.value })}
              />
            </div>
            <div className="ml-auto flex gap-2">
              <Button
                variant="ghost"
                size="sm"
                disabled={!filtered}
                onClick={() => setFilter(EMPTY_FILTER)}
              >
                {t("console.audit.reset")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={exporting || rangeInvalid}
                onClick={() => void handleExport()}
              >
                <Download className="mr-1.5 h-3.5 w-3.5" aria-hidden />
                {t("console.audit.exportCsv")}
              </Button>
            </div>
          </div>
          {rangeInvalid ? (
            <p className="text-xs text-status-danger" role="alert">
              {t("console.audit.rangeInvalid")}
            </p>
          ) : null}
          <p className="text-xs text-text-secondary">
            {events.length >= PAGE_SIZE
              ? t("console.audit.firstN", { count: PAGE_SIZE })
              : t("console.audit.count", { count: events.length })}
          </p>
        </div>
      ) : null}

      <Timeline
        items={entries}
        loading={loading}
        locale={i18n.language}
        aria-label={t("audit.title")}
        empty={t("audit.empty")}
        error={
          error ? (
            <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{t("audit.loadError", { error })}</span>
            </div>
          ) : null
        }
      />
    </div>
  );
}
