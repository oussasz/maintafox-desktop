/**
 * Activity feed (Settings › Activity & audit): what happened in the modules, newest first.
 *
 * FR/EN labels for modules, classes and severities; a summary as readable lines (never raw
 * JSON); filters with plain names; load errors with retry; one page at a time with the
 * pagination stating its position.
 */
import { Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ListAccessNote } from "@/components/ui/list-access-note";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { useCanReadList } from "@/lib/list-access";
import {
  type ActivityEventSummary,
  type ActivityFilter,
  type SavedActivityFilter,
  getEventChain,
  listActivityEvents,
  listSavedActivityFilters,
  saveActivityFilter,
} from "@/services/activity-service";
import { listOrgTree } from "@/services/org-node-service";
import { pushAppToast } from "@/store/app-toast-store";
import { P } from "@shared/rbac/permissions.generated";

import {
  ACTIVITY_CLASS_OPTIONS,
  ACTIVITY_MODULE_OPTIONS,
  ACTIVITY_SEVERITY_OPTIONS,
  eventClassLabel,
  eventLabel,
  humanizeCode,
  moduleLabel,
  payloadLines,
  severityLabel,
  severityTone,
} from "./activity-labels";

const PAGE_SIZE = 40;
const ANY = "__any__";

const TONE_TO_TIMELINE: Record<string, TimelineTone> = {
  danger: "danger",
  warning: "warning",
  attention: "warning",
  info: "info",
  success: "success",
};

interface ActivityFeedPanelProps {
  className?: string;
}

export function ActivityFeedPanel({ className }: ActivityFeedPanelProps) {
  const { t } = useTranslation("activity");
  const { t: tShell } = useTranslation("shell");
  const { can, isLoading: permissionsLoading } = usePermissions();
  const { formatDateTime } = useFormatters();

  const [items, setItems] = useState<ActivityEventSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [expanded, setExpanded] = useState<Record<number, ActivityChainLine[]>>({});
  const [chainLoading, setChainLoading] = useState<Record<number, boolean>>({});
  const [savedViews, setSavedViews] = useState<SavedActivityFilter[]>([]);
  const [saveViewName, setSaveViewName] = useState("");
  const [selectedSavedViewId, setSelectedSavedViewId] = useState("");

  const [eventClass, setEventClass] = useState<string>("");
  const [sourceModule, setSourceModule] = useState<string>("");
  const [severity, setSeverity] = useState<string>("");
  const [entityScopeId, setEntityScopeId] = useState<string>("");
  // Organisation units to filter on, picked from the organisation tree (the filter matches one unit).
  // Without the organisation view right the tree is not requested and the field says why.
  const canReadOrg = useCanReadList("orgNodes");
  const [orgNodes, setOrgNodes] = useState<{ id: string; label: string }[]>([]);
  const [orgNodesError, setOrgNodesError] = useState<unknown>(null);
  const [orgNodesReload, setOrgNodesReload] = useState(0);

  useEffect(() => {
    if (!canReadOrg) return;
    let cancelled = false;
    setOrgNodesError(null);
    listOrgTree()
      .then((rows) => {
        if (cancelled) return;
        setOrgNodes(
          rows.map((r) => ({
            id: String(r.node.id),
            label: `${"  ".repeat(Math.max(0, r.node.depth))}${r.node.code} — ${r.node.name}`,
          })),
        );
      })
      .catch((err: unknown) => {
        if (!cancelled) setOrgNodesError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [canReadOrg, orgNodesReload]);
  const [correlationId, setCorrelationId] = useState<string>("");
  const [dateFrom, setDateFrom] = useState<string>("");
  const [dateTo, setDateTo] = useState<string>("");
  const [appliedFilter, setAppliedFilter] = useState<ActivityFilter>({
    limit: PAGE_SIZE,
    offset: 0,
  });
  const offset = appliedFilter.offset ?? 0;

  const currentFilter = useMemo<ActivityFilter>(() => {
    const out: ActivityFilter = { limit: PAGE_SIZE, offset: 0 };
    if (eventClass) out.event_class = eventClass;
    if (sourceModule) out.source_module = sourceModule;
    if (severity) out.severity = severity;
    if (correlationId.trim()) out.correlation_id = correlationId.trim();
    if (dateFrom) out.date_from = `${dateFrom}T00:00:00Z`;
    if (dateTo) out.date_to = `${dateTo}T23:59:59Z`;
    if (entityScopeId.trim()) {
      const parsed = Number(entityScopeId);
      if (!Number.isNaN(parsed)) out.entity_scope_id = parsed;
    }
    return out;
  }, [correlationId, dateFrom, dateTo, entityScopeId, eventClass, severity, sourceModule]);

  const loadSavedViews = useCallback(async () => {
    try {
      setSavedViews(await listSavedActivityFilters());
    } catch {
      // Saved views are a convenience; the feed itself still loads (the IPC toast reports it).
    }
  }, []);

  const loadData = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setItems(await listActivityEvents(appliedFilter));
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, [appliedFilter]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  useEffect(() => {
    void loadSavedViews();
  }, [loadSavedViews]);

  useEffect(() => {
    if (!autoRefresh) return;
    const handle = window.setInterval(() => {
      void loadData();
    }, 30_000);
    return () => window.clearInterval(handle);
  }, [autoRefresh, loadData]);

  const handleExpand = useCallback(
    async (row: ActivityEventSummary) => {
      if (expanded[row.id]) {
        setExpanded((prev) => {
          const next = { ...prev };
          delete next[row.id];
          return next;
        });
        return;
      }
      setChainLoading((prev) => ({ ...prev, [row.id]: true }));
      try {
        const chain = await getEventChain(row.id, "activity_events");
        setExpanded((prev) => ({
          ...prev,
          [row.id]: chain.events.map((e) => ({
            key: `${e.table}-${e.event_id}`,
            at: e.happened_at,
            module: e.source_module,
            code: e.event_code ?? e.action_code ?? null,
            link: e.link_type,
          })),
        }));
      } catch (err) {
        pushAppToast({
          title: t("feed.chainFailed"),
          description: describeAppError(err).title,
          variant: "destructive",
        });
      } finally {
        setChainLoading((prev) => ({ ...prev, [row.id]: false }));
      }
    },
    [expanded, t],
  );

  const timelineEntries = useMemo((): TimelineEntry[] => {
    return items.map((row) => {
      const lines = payloadLines(row.summary_json);
      return {
        id: String(row.id),
        title: (
          <span>
            {moduleLabel(t, row.source_module)}
            <span className="text-text-muted"> — {eventLabel(t, row.event_code)}</span>
          </span>
        ),
        timestamp: row.happened_at,
        actor: row.actor_username ?? t("feed.system"),
        badges: [
          {
            label: severityLabel(t, row.severity),
            tone: TONE_TO_TIMELINE[severityTone(row.severity)] ?? "muted",
          },
          { label: eventClassLabel(t, row.event_class), tone: "muted" },
        ],
        description: (
          <>
            {lines.length > 0 ? (
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">
                {lines.map((l, i) => (
                  <div key={`${l.key}-${i}`} className="contents">
                    <dt className="text-text-muted">{l.key}</dt>
                    <dd className="break-words text-text-secondary">{l.value}</dd>
                  </div>
                ))}
              </dl>
            ) : (
              <span className="text-xs text-text-muted">{t("feed.noSummary")}</span>
            )}
            {row.from_other_device ? (
              <p className="mt-1 text-xs text-text-muted">
                {tShell("sync.history.fromOtherDevice")}
              </p>
            ) : null}
          </>
        ),
        details: expanded[row.id] ? (
          <ul className="space-y-1 text-xs">
            {expanded[row.id]?.map((line) => (
              <li key={line.key} className="flex flex-wrap gap-x-2">
                <span className="text-text-muted">
                  {line.link
                    ? t(`feed.link.${line.link}`, { defaultValue: humanizeCode(line.link) })
                    : t("feed.link.related")}
                </span>
                <span>{moduleLabel(t, line.module)}</span>
                {line.code ? (
                  <span className="text-text-secondary">{eventLabel(t, line.code)}</span>
                ) : null}
                <time className="text-text-muted" dateTime={line.at}>
                  {formatDateTime(line.at)}
                </time>
              </li>
            ))}
          </ul>
        ) : undefined,
        detailsExpanded: Boolean(expanded[row.id]),
        onDetailsToggle: () => void handleExpand(row),
        detailsLabel: chainLoading[row.id] ? t("feed.chainLoading") : t("feed.chain"),
      };
    });
  }, [items, expanded, chainLoading, handleExpand, t, tShell, formatDateTime]);

  const applyFilters = useCallback(() => {
    setAppliedFilter({ ...currentFilter, offset: 0, limit: PAGE_SIZE });
  }, [currentFilter]);

  const resetFilters = () => {
    setEventClass("");
    setSourceModule("");
    setSeverity("");
    setEntityScopeId("");
    setCorrelationId("");
    setDateFrom("");
    setDateTo("");
    setSelectedSavedViewId("");
    setAppliedFilter({ limit: PAGE_SIZE, offset: 0 });
  };

  const applySavedView = (id: string) => {
    setSelectedSavedViewId(id);
    const selected = savedViews.find((s) => String(s.id) === id);
    if (!selected || typeof selected.filter_json !== "object" || selected.filter_json === null) {
      return;
    }
    const raw = selected.filter_json as Record<string, unknown>;
    const text = (k: string) => (typeof raw[k] === "string" ? (raw[k] as string) : "");
    setEventClass(text("event_class"));
    setSourceModule(text("source_module"));
    setSeverity(text("severity"));
    setCorrelationId(text("correlation_id"));
    setEntityScopeId(
      typeof raw["entity_scope_id"] === "number" ? String(raw["entity_scope_id"]) : "",
    );
    setDateFrom(text("date_from").slice(0, 10));
    setDateTo(text("date_to").slice(0, 10));
  };

  const saveView = async () => {
    try {
      await saveActivityFilter({
        view_name: saveViewName.trim(),
        filter_json: currentFilter,
        is_default: false,
      });
      setSaveViewName("");
      await loadSavedViews();
      pushAppToast({ title: t("feed.viewSaved"), variant: "success" });
    } catch {
      // The IPC toast reports the failure.
    }
  };

  const pageStart = offset + 1;
  const pageEnd = offset + items.length;

  if (permissionsLoading) {
    return (
      <Card className={className}>
        <CardContent className="pt-6">
          <LoadingState />
        </CardContent>
      </Card>
    );
  }

  if (!can(P.LOG_VIEW)) {
    return (
      <Card className={className}>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">{t("tabs.activity")}</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-text-muted">{t("feed.noPermission")}</CardContent>
      </Card>
    );
  }

  const selectFilter = (
    id: string,
    label: string,
    value: string,
    onChange: (v: string) => void,
    options: readonly string[],
    optionLabel: (code: string) => string,
  ) => (
    <div className="space-y-1">
      <Label htmlFor={id} className="text-xs text-text-muted">
        {label}
      </Label>
      <Select value={value || ANY} onValueChange={(v) => onChange(v === ANY ? "" : v)}>
        <SelectTrigger id={id} aria-label={label} className="h-10">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ANY}>{t("filters.any")}</SelectItem>
          {options.map((o) => (
            <SelectItem key={o} value={o}>
              {optionLabel(o)}
            </SelectItem>
          ))}
          {value && !options.includes(value) ? (
            <SelectItem value={value}>{optionLabel(value)}</SelectItem>
          ) : null}
        </SelectContent>
      </Select>
    </div>
  );

  return (
    <Card className={className}>
      <CardHeader className="flex flex-row items-center justify-between gap-2 pb-3">
        <CardTitle className="text-base">{t("tabs.activity")}</CardTitle>
        <RefreshButton onRefresh={() => void loadData()} loading={loading} />
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          {selectFilter(
            "activity-filter-module",
            t("filters.module"),
            sourceModule,
            setSourceModule,
            ACTIVITY_MODULE_OPTIONS,
            (c) => moduleLabel(t, c),
          )}
          {selectFilter(
            "activity-filter-severity",
            t("filters.severity"),
            severity,
            setSeverity,
            ACTIVITY_SEVERITY_OPTIONS,
            (c) => severityLabel(t, c),
          )}
          {selectFilter(
            "activity-filter-class",
            t("filters.eventClass"),
            eventClass,
            setEventClass,
            ACTIVITY_CLASS_OPTIONS,
            (c) => eventClassLabel(t, c),
          )}
          <div className="space-y-1">
            <Label htmlFor="activity-filter-date-from" className="text-xs text-text-muted">
              {t("filters.dateFrom")}
            </Label>
            <Input
              id="activity-filter-date-from"
              type="date"
              className="h-10"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor="activity-filter-date-to" className="text-xs text-text-muted">
              {t("filters.dateTo")}
            </Label>
            <Input
              id="activity-filter-date-to"
              type="date"
              className="h-10"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
            />
          </div>
          <details className="md:col-span-1">
            <summary className="cursor-pointer select-none py-2 text-xs text-text-muted">
              {t("filters.advanced")}
            </summary>
            <div className="mt-1 space-y-2">
              {canReadOrg || entityScopeId
                ? selectFilter(
                    "activity-filter-entity-scope-id",
                    t("filters.orgUnit"),
                    entityScopeId,
                    setEntityScopeId,
                    orgNodes.map((n) => n.id),
                    (id) =>
                      orgNodes.find((n) => n.id === id)?.label ??
                      t("filters.orgUnitUnlisted", { id }),
                  )
                : null}
              {!canReadOrg ? <ListAccessNote list="orgNodes" /> : null}
              {orgNodesError != null ? (
                <ErrorState
                  className="py-3"
                  error={orgNodesError}
                  onRetry={() => setOrgNodesReload((n) => n + 1)}
                />
              ) : null}
              <div className="space-y-1">
                <Label htmlFor="activity-filter-correlation-id" className="text-xs text-text-muted">
                  {t("filters.correlation")}
                </Label>
                <Input
                  id="activity-filter-correlation-id"
                  className="h-10"
                  value={correlationId}
                  onChange={(e) => setCorrelationId(e.target.value)}
                />
              </div>
            </div>
          </details>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button className="h-10" onClick={applyFilters}>
            {t("filters.apply")}
          </Button>
          <Button className="h-10" variant="outline" onClick={resetFilters}>
            {t("filters.reset")}
          </Button>
          <div className="flex min-h-10 items-center gap-2 rounded-md border border-surface-border px-3 text-xs">
            <Checkbox
              id="activity-auto-refresh"
              checked={autoRefresh}
              onCheckedChange={(v) => setAutoRefresh(Boolean(v))}
            />
            <label htmlFor="activity-auto-refresh" className="cursor-pointer">
              {t("feed.autoRefresh")}
            </label>
          </div>
        </div>

        <div className="flex flex-wrap items-end gap-2">
          <div className="space-y-1">
            <Label htmlFor="activity-saved-views" className="text-xs text-text-muted">
              {t("feed.savedViews")}
            </Label>
            <Select
              value={selectedSavedViewId || ANY}
              onValueChange={(v) => applySavedView(v === ANY ? "" : v)}
            >
              <SelectTrigger id="activity-saved-views" className="h-10 w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>{t("feed.noSavedView")}</SelectItem>
                {savedViews.map((view) => (
                  <SelectItem key={view.id} value={String(view.id)}>
                    {view.is_default
                      ? t("feed.defaultView", { name: view.view_name })
                      : view.view_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-1">
            <Label htmlFor="activity-save-view-name" className="text-xs text-text-muted">
              {t("feed.saveViewName")}
            </Label>
            <Input
              id="activity-save-view-name"
              className="h-10 max-w-[220px]"
              value={saveViewName}
              onChange={(e) => setSaveViewName(e.target.value)}
            />
          </div>
          <Button
            variant="outline"
            className="h-10 gap-1.5"
            disabled={!saveViewName.trim()}
            onClick={() => void saveView()}
          >
            <Save className="h-4 w-4" aria-hidden />
            {t("feed.saveView")}
          </Button>
        </div>

        <Timeline
          items={timelineEntries}
          loading={loading}
          error={loadError}
          onRetry={() => void loadData()}
          groupBy="day"
          aria-label={t("tabs.activity")}
          empty={
            <div className="rounded-md border border-surface-border p-4 text-center text-sm text-text-muted">
              {t("feed.empty")}
            </div>
          }
        />

        <div className="flex flex-wrap items-center justify-end gap-2">
          {items.length > 0 ? (
            <span className="text-xs text-text-muted" role="status">
              {t("pager.range", { from: pageStart, to: pageEnd })}
            </span>
          ) : null}
          <Button
            variant="outline"
            className="h-10"
            disabled={offset === 0 || loading}
            onClick={() =>
              setAppliedFilter((current) => ({
                ...current,
                offset: Math.max(0, (current.offset ?? 0) - PAGE_SIZE),
              }))
            }
          >
            {t("pager.previous")}
          </Button>
          <Button
            variant="outline"
            className="h-10"
            disabled={items.length < PAGE_SIZE || loading}
            onClick={() =>
              setAppliedFilter((current) => ({
                ...current,
                offset: (current.offset ?? 0) + PAGE_SIZE,
              }))
            }
          >
            {t("pager.next")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

interface ActivityChainLine {
  key: string;
  at: string;
  module: string | null;
  code: string | null;
  link: string | null;
}
