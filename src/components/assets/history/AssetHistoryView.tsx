import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { AssetLifeSummaryStrip } from "@/components/assets/history/AssetLifeSummaryStrip";
import { AssetTimelineFilters } from "@/components/assets/history/AssetTimelineFilters";
import { AssetTimelineList } from "@/components/assets/history/AssetTimelineList";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { getAssetHistorySummary, listAssetHistoryEvents } from "@/services/asset-history-service";
import type {
  AssetHistoryEvent,
  AssetHistoryEventType,
  AssetHistoryPeriod,
  AssetHistorySummary,
} from "@shared/ipc-types";

/** Events read per page (the backend's page size for this list). */
const HISTORY_PAGE = 500;

function mapTypeFilterToEventTypes(typeFilter: string): AssetHistoryEventType[] | null {
  switch (typeFilter) {
    case "ot":
      return ["wo_created", "wo_started", "wo_closed"];
    case "di":
      return ["di_created", "di_closed"];
    case "pm":
      return ["pm_scheduled", "pm_executed"];
    case "inspection":
      return ["inspection_recorded"];
    case "documents":
      return ["document_linked", "photo_added"];
    case "failures":
      return ["failure_recorded"];
    default:
      return null;
  }
}

export function AssetHistoryView({ assetId }: { assetId: number }) {
  const { t } = useTranslation("equipment");
  const navigate = useNavigate();
  const [summary, setSummary] = useState<AssetHistorySummary | null>(null);
  const [events, setEvents] = useState<AssetHistoryEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  /** The last page came back full: older events may exist (no total is returned). */
  const [mayHaveMore, setMayHaveMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [moreError, setMoreError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  const [typeFilter, setTypeFilter] = useState("all");
  const [period, setPeriod] = useState<AssetHistoryPeriod>("all");
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search, 300);

  const eventTypes = useMemo(() => mapTypeFilterToEventTypes(typeFilter), [typeFilter]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    Promise.all([
      getAssetHistorySummary(assetId),
      listAssetHistoryEvents({
        asset_id: assetId,
        event_types: eventTypes,
        period,
        search: debouncedSearch.trim() || null,
        limit: HISTORY_PAGE,
        offset: 0,
      }),
    ])
      .then(([summaryResp, eventsResp]) => {
        if (cancelled) return;
        setSummary(summaryResp);
        setEvents(eventsResp);
        setMayHaveMore(eventsResp.length >= HISTORY_PAGE);
        setMoreError(null);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [assetId, eventTypes, period, debouncedSearch, attempt]);

  const loadOlder = async () => {
    setLoadingMore(true);
    setMoreError(null);
    try {
      const page = await listAssetHistoryEvents({
        asset_id: assetId,
        event_types: eventTypes,
        period,
        search: debouncedSearch.trim() || null,
        limit: HISTORY_PAGE,
        offset: events.length,
      });
      setEvents((prev) => [...prev, ...page]);
      setMayHaveMore(page.length >= HISTORY_PAGE);
    } catch (err) {
      setMoreError(err);
    } finally {
      setLoadingMore(false);
    }
  };

  const openRef = (ev: AssetHistoryEvent) => {
    const hint = ev.ref?.route_hint;
    if (hint) {
      navigate(hint);
      return;
    }
    if (ev.ref?.entity_type === "wo" && ev.ref.entity_id != null) {
      navigate(`/work-orders?openWo=${ev.ref.entity_id}`);
    } else if (ev.ref?.entity_type === "di" && ev.ref.entity_id != null) {
      navigate(`/requests?openDi=${ev.ref.entity_id}`);
    } else if (ev.ref?.entity_type === "pm" && ev.ref.entity_id != null) {
      navigate(`/pm?planId=${ev.ref.entity_id}`);
    }
  };

  if (loading && events.length === 0) {
    return <LoadingState label={t("history.loading")} />;
  }
  if (error != null) {
    // A failed load is never an empty history.
    return <ErrorState error={error} title={t("history.loadError")} onRetry={retry} />;
  }

  return (
    <div className="space-y-3">
      <AssetLifeSummaryStrip summary={summary} />
      <AssetTimelineFilters
        typeFilter={typeFilter}
        period={period}
        search={search}
        onTypeFilterChange={setTypeFilter}
        onPeriodChange={setPeriod}
        onSearchChange={setSearch}
      />
      <AssetTimelineList events={events} onOpenRef={openRef} />
      {mayHaveMore ? (
        <div
          role="status"
          className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-xs text-tone-info-fg"
        >
          <span>{t("history.partial", { count: events.length })}</span>
          <Button
            variant="outline"
            className="h-10"
            disabled={loadingMore}
            onClick={() => void loadOlder()}
          >
            {loadingMore ? t("history.loading") : t("history.loadOlder")}
          </Button>
        </div>
      ) : null}
      {moreError != null ? (
        <ErrorState
          error={moreError}
          title={t("history.loadOlderError")}
          onRetry={() => void loadOlder()}
        />
      ) : null}
    </div>
  );
}
