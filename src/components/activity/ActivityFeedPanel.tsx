import { AlertCircle, Link2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type ActivityEventSummary,
  type ActivityFilter,
  type SavedActivityFilter,
  getEventChain,
  listActivityEvents,
  listSavedActivityFilters,
  saveActivityFilter,
} from "@/services/activity-service";
import { toErrorMessage } from "@/utils/errors";

const PAGE_SIZE = 40;

interface ActivityFeedPanelProps {
  className?: string;
}

export function ActivityFeedPanel({ className }: ActivityFeedPanelProps) {
  const { can, isLoading: permissionsLoading } = usePermissions();
  const [items, setItems] = useState<ActivityEventSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [autoRefresh, setAutoRefresh] = useState(false);
  const [expanded, setExpanded] = useState<Record<number, string>>({});
  const [chainLoading, setChainLoading] = useState<Record<number, boolean>>({});
  const [savedViews, setSavedViews] = useState<SavedActivityFilter[]>([]);
  const [saveViewName, setSaveViewName] = useState("");
  const [selectedSavedViewId, setSelectedSavedViewId] = useState("");

  const [eventClass, setEventClass] = useState<string>("");
  const [sourceModule, setSourceModule] = useState<string>("");
  const [severity, setSeverity] = useState<string>("");
  const [entityScopeId, setEntityScopeId] = useState<string>("");
  const [correlationId, setCorrelationId] = useState<string>("");
  const [dateFrom, setDateFrom] = useState<string>("");
  const [dateTo, setDateTo] = useState<string>("");
  const [offset, setOffset] = useState(0);
  const [appliedFilter, setAppliedFilter] = useState<ActivityFilter>({
    limit: PAGE_SIZE,
    offset: 0,
  });

  const currentFilter = useMemo<ActivityFilter>(() => {
    const out: ActivityFilter = { limit: PAGE_SIZE, offset };
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
  }, [correlationId, dateFrom, dateTo, entityScopeId, eventClass, offset, severity, sourceModule]);

  const loadSavedViews = useCallback(async () => {
    try {
      setSavedViews(await listSavedActivityFilters());
    } catch {
      // non-fatal for feed rendering
    }
  }, []);

  const loadData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setItems(await listActivityEvents(appliedFilter));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [appliedFilter]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  useEffect(() => {
    setAppliedFilter({
      limit: PAGE_SIZE,
      offset: 0,
    });
  }, []);

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

  const grouped = useMemo(() => {
    const groups = new Map<string, ActivityEventSummary[]>();
    for (const item of items) {
      const day = item.happened_at.slice(0, 10) || "unknown";
      const list = groups.get(day) ?? [];
      list.push(item);
      groups.set(day, list);
    }
    return Array.from(groups.entries()).sort(([a], [b]) => b.localeCompare(a));
  }, [items]);

  const handleExpand = async (row: ActivityEventSummary) => {
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
      const text = chain.events
        .map((e) => {
          const code = e.event_code ?? e.action_code ?? "unknown";
          return `- ${e.happened_at} [${e.link_type ?? "related"}] ${code}`;
        })
        .join("\n");
      setExpanded((prev) => ({ ...prev, [row.id]: text }));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setChainLoading((prev) => ({ ...prev, [row.id]: false }));
    }
  };

  const applyFilters = useCallback(() => {
    setOffset(0);
    setAppliedFilter({
      ...currentFilter,
      offset: 0,
      limit: PAGE_SIZE,
    });
  }, [currentFilter]);

  if (permissionsLoading) {
    return (
      <Card className={className}>
        <CardContent className="pt-6 text-sm text-muted-foreground">Loading permissions…</CardContent>
      </Card>
    );
  }

  if (!can("log.view")) {
    return (
      <Card className={className}>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Activity Feed</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          You do not have permission to view activity logs.
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className={className}>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Activity Feed</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-1 gap-2 md:grid-cols-3">
          <Input
            id="activity-filter-event-class"
            aria-label="Event class filter"
            placeholder="event_class"
            value={eventClass}
            onChange={(e) => setEventClass(e.target.value)}
          />
          <Input
            id="activity-filter-source-module"
            aria-label="Source module filter"
            placeholder="source_module"
            value={sourceModule}
            onChange={(e) => setSourceModule(e.target.value)}
          />
          <Input
            id="activity-filter-severity"
            aria-label="Severity filter"
            placeholder="severity"
            value={severity}
            onChange={(e) => setSeverity(e.target.value)}
          />
          <Input
            id="activity-filter-entity-scope-id"
            aria-label="Entity scope ID filter"
            placeholder="entity_scope_id"
            value={entityScopeId}
            onChange={(e) => setEntityScopeId(e.target.value)}
          />
          <Input
            id="activity-filter-correlation-id"
            aria-label="Correlation ID filter"
            placeholder="correlation_id"
            value={correlationId}
            onChange={(e) => setCorrelationId(e.target.value)}
          />
          <div className="grid grid-cols-2 gap-2">
            <Input
              id="activity-filter-date-from"
              aria-label="Date from filter"
              type="date"
              value={dateFrom}
              onChange={(e) => setDateFrom(e.target.value)}
            />
            <Input
              id="activity-filter-date-to"
              aria-label="Date to filter"
              type="date"
              value={dateTo}
              onChange={(e) => setDateTo(e.target.value)}
            />
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={applyFilters}>
            Apply filters
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setEventClass("");
              setSourceModule("");
              setSeverity("");
              setEntityScopeId("");
              setCorrelationId("");
              setDateFrom("");
              setDateTo("");
              setSelectedSavedViewId("");
              setOffset(0);
              setAppliedFilter({ limit: PAGE_SIZE, offset: 0 });
            }}
          >
            Reset
          </Button>
          <div className="flex items-center gap-2 rounded-md border px-2 py-1 text-xs">
            <Checkbox
              id="activity-auto-refresh"
              aria-label="Auto-refresh activity feed"
              checked={autoRefresh}
              onCheckedChange={(v) => setAutoRefresh(Boolean(v))}
            />
            <label htmlFor="activity-auto-refresh" className="cursor-pointer">
              Auto-refresh (30s)
            </label>
          </div>
          <Button size="sm" variant="ghost" onClick={() => void loadData()}>
            <RefreshCw className="mr-1 h-3 w-3" /> Refresh
          </Button>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <select
            aria-label="Saved activity views"
            className="h-9 rounded-md border px-2 text-sm"
            value={selectedSavedViewId}
            onChange={(e) => {
              setSelectedSavedViewId(e.target.value);
              const id = Number(e.target.value);
              const selected = savedViews.find((s) => s.id === id);
              if (!selected || typeof selected.filter_json !== "object" || selected.filter_json === null) return;
              const raw = selected.filter_json as Record<string, unknown>;
              setEventClass(typeof raw["event_class"] === "string" ? raw["event_class"] : "");
              setSourceModule(typeof raw["source_module"] === "string" ? raw["source_module"] : "");
              setSeverity(typeof raw["severity"] === "string" ? raw["severity"] : "");
              setCorrelationId(
                typeof raw["correlation_id"] === "string" ? raw["correlation_id"] : "",
              );
              setEntityScopeId(
                typeof raw["entity_scope_id"] === "number" ? String(raw["entity_scope_id"]) : "",
              );
              setDateFrom(
                typeof raw["date_from"] === "string" ? raw["date_from"].slice(0, 10) : "",
              );
              setDateTo(typeof raw["date_to"] === "string" ? raw["date_to"].slice(0, 10) : "");
              setOffset(0);
            }}
          >
            <option value="">Saved views</option>
            {savedViews.map((view) => (
              <option key={view.id} value={view.id}>
                {view.view_name}
                {view.is_default ? " (default)" : ""}
              </option>
            ))}
          </select>
          <Input
            className="max-w-[220px]"
            aria-label="Saved activity view name"
            placeholder="Save current view as..."
            value={saveViewName}
            onChange={(e) => setSaveViewName(e.target.value)}
          />
          <Button
            size="sm"
            variant="outline"
            disabled={!saveViewName.trim()}
            onClick={() => {
              void (async () => {
                await saveActivityFilter({
                  view_name: saveViewName.trim(),
                  filter_json: currentFilter,
                  is_default: false,
                });
                setSaveViewName("");
                await loadSavedViews();
              })().catch((err: unknown) => {
                setError(toErrorMessage(err));
              });
            }}
          >
            Save current view
          </Button>
        </div>

        {error && (
          <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-2 text-sm text-destructive">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        {loading && <div className="text-sm text-muted-foreground">Loading activity feed...</div>}

        <div className="space-y-3">
          {grouped.map(([day, rows]) => (
            <div key={day} className="space-y-2">
              <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{day}</div>
              {rows.map((row) => (
                <div key={row.id} className="rounded-md border p-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant="outline">{row.severity}</Badge>
                    <Badge>{row.event_code}</Badge>
                    <span className="text-xs text-muted-foreground">{row.source_module}</span>
                    <span className="text-xs text-muted-foreground">
                      {row.actor_username ?? `user:${row.actor_id ?? "system"}`}
                    </span>
                    <span className="text-xs text-muted-foreground">{new Date(row.happened_at).toLocaleString()}</span>
                  </div>

                  <div className="mt-1 text-sm text-muted-foreground">
                    {row.summary_json ? JSON.stringify(row.summary_json) : "No summary payload"}
                  </div>

                  <div className="mt-2 flex flex-wrap gap-2">
                    {row.source_record_id && (
                      <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" disabled>
                        Open source record
                      </Button>
                    )}
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 px-2 text-xs"
                      disabled={Boolean(chainLoading[row.id])}
                      onClick={() => void handleExpand(row)}
                    >
                      <Link2 className="mr-1 h-3 w-3" />
                      {chainLoading[row.id] ? "Loading chain..." : "Correlation chain"}
                    </Button>
                  </div>

                  {expanded[row.id] && (
                    <pre className="mt-2 whitespace-pre-wrap rounded bg-muted p-2 text-xs">{expanded[row.id]}</pre>
                  )}
                </div>
              ))}
            </div>
          ))}

          {!loading && items.length === 0 && (
            <div className="rounded-md border p-4 text-center text-sm text-muted-foreground">
              No activity events found for current filters.
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-2">
          <Button
            size="sm"
            variant="outline"
            disabled={offset === 0}
            onClick={() =>
              setOffset((prev) => {
                const nextOffset = Math.max(0, prev - PAGE_SIZE);
                setAppliedFilter((current) => ({ ...current, offset: nextOffset, limit: PAGE_SIZE }));
                return nextOffset;
              })
            }
          >
            Previous
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={items.length < PAGE_SIZE}
            onClick={() =>
              setOffset((prev) => {
                const nextOffset = prev + PAGE_SIZE;
                setAppliedFilter((current) => ({ ...current, offset: nextOffset, limit: PAGE_SIZE }));
                return nextOffset;
              })
            }
          >
            Next
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
