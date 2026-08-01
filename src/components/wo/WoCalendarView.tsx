/**
 * WoCalendarView.tsx
 *
 * Month / week / day calendar view for work orders.
 * Same design as DiCalendarView — chips use border-l-2 urgency coloring.
 * WOs indexed by planned_start.
 * Phase 2 – Sub-phase 05.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { statusToI18nKey } from "@/utils/wo-status";
import type { WorkOrder } from "@shared/ipc-types";

// ── Types ─────────────────────────────────────────────────────────────────────

type CalendarMode = "month" | "week" | "day";

interface WoCalendarViewProps {
  items: WorkOrder[];
  onSelect: (wo: WorkOrder) => void;
}

// ── Urgency → border-left colour (matches DI pattern) ────────────────────────

const URGENCY_BORDER: Record<number, string> = {
  1: "border-l-green-400", // very low
  2: "border-l-green-400", // low
  3: "border-l-yellow-400", // medium
  4: "border-l-orange-400", // high
  5: "border-l-red-500", // critical
};

function urgencyBorderClass(level: number | null | undefined): string {
  if (level == null) return "border-l-gray-300";
  return URGENCY_BORDER[level] ?? "border-l-gray-300";
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function startOfWeek(d: Date): Date {
  const day = d.getDay();
  const diff = d.getDate() - day + (day === 0 ? -6 : 1);
  return new Date(d.getFullYear(), d.getMonth(), diff);
}

function formatMonthYear(d: Date, locale: string): string {
  return d.toLocaleDateString(locale, { month: "long", year: "numeric" });
}

function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function dateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

const MAX_CHIPS = 3;

function getDayNames(locale: string): string[] {
  const base = new Date(2024, 0, 1); // Monday 2024-01-01
  return Array.from({ length: 7 }, (_, i) => {
    const d = new Date(base);
    d.setDate(d.getDate() + i);
    return d.toLocaleDateString(locale, { weekday: "short" });
  });
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCalendarView({ items, onSelect }: WoCalendarViewProps) {
  const { t, i18n } = useTranslation("ot");
  const [mode, setMode] = useState<CalendarMode>("month");
  const [cursor, setCursor] = useState(() => new Date());

  const dayNames = useMemo(() => getDayNames(i18n.language), [i18n.language]);

  // Index WOs by planned_start date
  const wosByDate = useMemo(() => {
    const map = new Map<string, WorkOrder[]>();
    for (const wo of items) {
      if (!wo.planned_start) continue;
      const key = wo.planned_start.slice(0, 10);
      const arr = map.get(key);
      if (arr) arr.push(wo);
      else map.set(key, [wo]);
    }
    return map;
  }, [items]);

  // Navigation
  const goToday = useCallback(() => setCursor(new Date()), []);

  const goPrev = useCallback(() => {
    setCursor((c) => {
      const d = new Date(c);
      if (mode === "month") d.setMonth(d.getMonth() - 1);
      else if (mode === "week") d.setDate(d.getDate() - 7);
      else d.setDate(d.getDate() - 1);
      return d;
    });
  }, [mode]);

  const goNext = useCallback(() => {
    setCursor((c) => {
      const d = new Date(c);
      if (mode === "month") d.setMonth(d.getMonth() + 1);
      else if (mode === "week") d.setDate(d.getDate() + 7);
      else d.setDate(d.getDate() + 1);
      return d;
    });
  }, [mode]);

  // Build month grid (42 cells)
  const monthDays = useMemo(() => {
    const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
    let dayOfWeek = first.getDay();
    if (dayOfWeek === 0) dayOfWeek = 7;
    const start = new Date(first);
    start.setDate(start.getDate() - (dayOfWeek - 1));

    const days: Date[] = [];
    for (let i = 0; i < 42; i++) {
      days.push(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
    }
    return days;
  }, [cursor]);

  // Build week days
  const weekDays = useMemo(() => {
    const start = startOfWeek(cursor);
    return Array.from(
      { length: 7 },
      (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i),
    );
  }, [cursor]);

  const today = new Date();

  return (
    <div className="flex flex-col h-full">
      {/* ── Header ────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-surface-border">
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" className="h-7 px-2" onClick={goPrev}>
            <ChevronLeft className="h-3.5 w-3.5" />
          </Button>
          <Button variant="outline" size="sm" className="h-7 px-3 text-xs" onClick={goToday}>
            {t("calendar.today")}
          </Button>
          <Button variant="outline" size="sm" className="h-7 px-2" onClick={goNext}>
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
          <span className="text-sm font-semibold text-text-primary capitalize">
            {mode === "day"
              ? cursor.toLocaleDateString(i18n.language, {
                  weekday: "long",
                  day: "numeric",
                  month: "long",
                  year: "numeric",
                })
              : formatMonthYear(cursor, i18n.language)}
          </span>
        </div>

        <div className="flex items-center rounded-md border p-0.5 gap-0.5">
          {(["month", "week", "day"] as CalendarMode[]).map((m) => (
            <Button
              key={m}
              variant={mode === m ? "default" : "ghost"}
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => setMode(m)}
            >
              {t(`calendar.mode.${m}`)}
            </Button>
          ))}
        </div>
      </div>

      {/* ── Month View ────────────────────────────────────────────── */}
      {mode === "month" && (
        <div className="flex-1 overflow-auto p-2">
          <div className="grid grid-cols-7 text-center text-[10px] font-medium text-text-muted mb-1">
            {dayNames.map((d) => (
              <div key={d} className="py-1">
                {d}
              </div>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-px bg-surface-border">
            {monthDays.map((day) => {
              const key = dateKey(day);
              const wos = wosByDate.get(key) ?? [];
              const isCurrentMonth = day.getMonth() === cursor.getMonth();
              const isToday = isSameDay(day, today);
              return (
                <div
                  key={key}
                  className={`bg-surface-0 min-h-[80px] p-1 ${!isCurrentMonth ? "opacity-40" : ""}`}
                >
                  <span
                    className={`text-[10px] font-medium ${isToday ? "bg-brand-primary text-white rounded-full px-1.5 py-0.5" : "text-text-muted"}`}
                  >
                    {day.getDate()}
                  </span>
                  <div className="mt-1 space-y-0.5">
                    {wos.slice(0, MAX_CHIPS).map((wo) => (
                      <button
                        key={wo.id}
                        type="button"
                        aria-label={`${wo.code} – ${wo.title}`}
                        className={`w-full text-left text-[9px] rounded px-1 py-0.5 truncate border-l-2 bg-surface-1 hover:bg-surface-2 cursor-pointer ${urgencyBorderClass(wo.urgency_level)}`}
                        onClick={() => onSelect(wo)}
                      >
                        {wo.code}
                      </button>
                    ))}
                    {wos.length > MAX_CHIPS && (
                      <span className="text-[9px] text-text-muted">+{wos.length - MAX_CHIPS}</span>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* ── Week View ─────────────────────────────────────────────── */}
      {mode === "week" && (
        <div className="flex-1 overflow-auto p-2">
          <div className="grid grid-cols-7 gap-2">
            {weekDays.map((day) => {
              const key = dateKey(day);
              const wos = wosByDate.get(key) ?? [];
              const isToday = isSameDay(day, today);
              return (
                <div key={key} className="rounded border border-surface-border min-h-[200px] p-2">
                  <div
                    className={`text-xs font-medium mb-2 ${isToday ? "text-brand-primary" : "text-text-muted"}`}
                  >
                    {day.toLocaleDateString(i18n.language, { weekday: "short", day: "numeric" })}
                  </div>
                  <div className="space-y-1">
                    {wos.map((wo) => (
                      <button
                        key={wo.id}
                        type="button"
                        aria-label={`${wo.code} – ${wo.title}`}
                        className={`w-full text-left text-[10px] rounded px-1.5 py-1 border-l-2 bg-surface-1 hover:bg-surface-2 cursor-pointer ${urgencyBorderClass(wo.urgency_level)}`}
                        onClick={() => onSelect(wo)}
                      >
                        <span className="font-mono">{wo.code}</span>
                        <span className="ml-1 truncate">{wo.title}</span>
                      </button>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* ── Day View ──────────────────────────────────────────────── */}
      {mode === "day" && (
        <div className="flex-1 overflow-auto p-4">
          {(() => {
            const key = dateKey(cursor);
            const wos = wosByDate.get(key) ?? [];
            return wos.length === 0 ? (
              <p className="text-sm text-text-muted text-center py-8">{t("empty.list")}</p>
            ) : (
              <div className="space-y-2">
                {wos.map((wo) => (
                  <button
                    key={wo.id}
                    type="button"
                    className={`w-full text-left rounded-lg border border-surface-border p-3 border-l-4 hover:bg-surface-1 transition-colors cursor-pointer ${urgencyBorderClass(wo.urgency_level)}`}
                    onClick={() => onSelect(wo)}
                    aria-label={`${wo.code} – ${wo.title}`}
                  >
                    <div className="flex items-center gap-2">
                      <span className="font-mono text-xs text-text-muted">{wo.code}</span>
                      <Badge variant="outline" className="text-[10px] border-0 bg-surface-1">
                        {t(`status.${statusToI18nKey(wo.status_code ?? "draft")}`)}
                      </Badge>
                    </div>
                    <p className="text-sm mt-1">{wo.title}</p>
                  </button>
                ))}
              </div>
            );
          })()}
        </div>
      )}
    </div>
  );
}
