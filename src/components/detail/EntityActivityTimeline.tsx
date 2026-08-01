/**
 * EntityActivityTimeline — filter/group adapter over the shared Timeline.
 * Presentation-only; callers supply EntityActivityItem[].
 */

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { EntityActivityItem } from "@/components/detail/entity-activity-types";
import { Timeline } from "@/components/timeline";
import type { TimelineEntry } from "@/components/timeline";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

export interface EntityActivityTimelineProps {
  items: EntityActivityItem[];
  emptyLabel: string;
  todayLabel: string;
  searchPlaceholder: string;
  kindAllLabel: string;
  periodAllLabel: string;
  period30dLabel: string;
  period90dLabel: string;
  /** Map typeKey → display badge label */
  typeLabel: (typeKey: string) => string;
  kindFilterOptions?: { value: string; label: string }[];
}

export function EntityActivityTimeline({
  items,
  emptyLabel,
  todayLabel,
  searchPlaceholder,
  kindAllLabel,
  periodAllLabel,
  period30dLabel,
  period90dLabel,
  typeLabel,
  kindFilterOptions,
}: EntityActivityTimelineProps) {
  const { i18n } = useTranslation();
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<string>("all");
  const [period, setPeriod] = useState<string>("all");

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    const now = Date.now();
    const periodMs = period === "30d" ? 30 * 86400000 : period === "90d" ? 90 * 86400000 : null;

    return items.filter((item) => {
      if (kind !== "all" && item.kind !== kind) return false;
      if (periodMs != null) {
        const t = new Date(item.at).getTime();
        if (Number.isNaN(t) || now - t > periodMs) return false;
      }
      if (!q) return true;
      const hay = `${item.title} ${item.description ?? ""} ${item.typeKey}`.toLowerCase();
      return hay.includes(q);
    });
  }, [items, kind, period, search]);

  const entries: TimelineEntry[] = useMemo(
    () =>
      filtered.map((ev) => ({
        id: ev.id,
        title: ev.title,
        description: ev.description ?? undefined,
        timestamp: ev.at,
        badges: [{ label: typeLabel(ev.typeKey), tone: "muted" as const }],
        subtitle: ev.statusLabel ?? undefined,
      })),
    [filtered, typeLabel],
  );

  const kindOptions = kindFilterOptions ?? [
    { value: "all", label: kindAllLabel },
    { value: "created", label: "Created" },
    { value: "edited", label: "Edited" },
    { value: "stock_movement", label: "Stock movement" },
    { value: "reservation_created", label: "Reservation created" },
    { value: "reservation_released", label: "Reservation released" },
  ];

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap gap-2">
        <Input
          className="h-8 max-w-xs text-xs"
          placeholder={searchPlaceholder}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <Select value={kind} onValueChange={setKind}>
          <SelectTrigger className="h-8 w-[160px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {kindOptions.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={period} onValueChange={setPeriod}>
          <SelectTrigger className="h-8 w-[140px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{periodAllLabel}</SelectItem>
            <SelectItem value="30d">{period30dLabel}</SelectItem>
            <SelectItem value="90d">{period90dLabel}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <Timeline
        items={entries}
        groupBy="day"
        todayLabel={todayLabel}
        locale={i18n.language}
        density="compact"
        empty={emptyLabel}
        aria-label="Entity activity"
      />
    </div>
  );
}
