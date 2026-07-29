import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import type { EntityActivityItem } from "@/components/detail/entity-activity-types";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

function dayKey(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toISOString().slice(0, 10);
}

function dayLabel(iso: string, locale: string, todayLabel: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return todayLabel;
  return d.toLocaleDateString(locale, {
    day: "2-digit",
    month: "short",
    year: "numeric",
  });
}

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
    const periodMs =
      period === "30d" ? 30 * 86400000 : period === "90d" ? 90 * 86400000 : null;

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

  const grouped = useMemo(() => {
    const map = new Map<string, { label: string; items: EntityActivityItem[] }>();
    for (const ev of filtered) {
      const key = dayKey(ev.at);
      const existing = map.get(key);
      if (existing) {
        existing.items.push(ev);
      } else {
        map.set(key, {
          label: dayLabel(ev.at, i18n.language, todayLabel),
          items: [ev],
        });
      }
    }
    return Array.from(map.entries());
  }, [filtered, i18n.language, todayLabel]);

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

      {filtered.length === 0 ? (
        <p className="text-sm text-text-muted">{emptyLabel}</p>
      ) : (
        <div className="space-y-4">
          {grouped.map(([key, group]) => (
            <div key={key} className="space-y-2">
              <p className="text-xs font-semibold uppercase tracking-wide text-text-muted">
                {group.label}
              </p>
              <div className="space-y-2 border-l border-surface-border pl-3">
                {group.items.map((ev) => (
                  <div
                    key={ev.id}
                    className="relative rounded border border-surface-border bg-surface-1 p-2.5"
                  >
                    <div className="absolute -left-[14px] top-3.5 h-2.5 w-2.5 rounded-full border-2 border-primary bg-surface-0" />
                    <div className="min-w-0 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <Badge variant="secondary" className="h-5 px-1.5 text-[10px] font-normal">
                          {typeLabel(ev.typeKey)}
                        </Badge>
                        {ev.statusLabel ? (
                          <span className="text-[11px] text-text-muted">{ev.statusLabel}</span>
                        ) : null}
                      </div>
                      <p className="text-sm font-medium leading-snug">{ev.title}</p>
                      {ev.description ? (
                        <p className="text-xs text-text-muted line-clamp-2">{ev.description}</p>
                      ) : null}
                      <p className="text-[11px] text-text-muted">
                        {new Date(ev.at).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </p>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
