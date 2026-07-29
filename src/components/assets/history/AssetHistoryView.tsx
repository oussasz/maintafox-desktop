import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";

import { AssetLifeSummaryStrip } from "@/components/assets/history/AssetLifeSummaryStrip";
import { AssetTimelineFilters } from "@/components/assets/history/AssetTimelineFilters";
import { AssetTimelineList } from "@/components/assets/history/AssetTimelineList";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { getAssetHistorySummary, listAssetHistoryEvents } from "@/services/asset-history-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  AssetHistoryEvent,
  AssetHistoryEventType,
  AssetHistoryPeriod,
  AssetHistorySummary,
} from "@shared/ipc-types";

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
  const [error, setError] = useState<string | null>(null);
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
        limit: 500,
        offset: 0,
      }),
    ])
      .then(([summaryResp, eventsResp]) => {
        if (cancelled) return;
        setSummary(summaryResp);
        setEvents(eventsResp);
      })
      .catch((err) => {
        if (!cancelled) setError(toErrorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [assetId, eventTypes, period, debouncedSearch]);

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

  if (loading) {
    return <p className="text-sm text-text-muted">{t("history.loading")}</p>;
  }
  if (error) {
    return <p className="text-sm text-status-danger">{error}</p>;
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
    </div>
  );
}
