/**
 * WoCalendarView.tsx
 *
 * Month / week calendar view for work orders.
 * WOs indexed by planned_start and rendered as urgency-coloured chips.
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import type { WorkOrder } from "@shared/ipc-types";

// ── Types ─────────────────────────────────────────────────────────────────────

type CalendarMode = "month" | "week";

interface WoCalendarViewProps {
  items: WorkOrder[];
  onSelect: (wo: WorkOrder) => void;
}

// ── Urgency colours ───────────────────────────────────────────────────────────

function urgencyColor(urgencyId: number | null): string {
  switch (urgencyId) {
    case 1:
      return "bg-emerald-100 text-emerald-800";
    case 2:
      return "bg-blue-100 text-blue-800";
    case 3:
      return "bg-amber-100 text-amber-800";
    case 4:
    case 5:
      return "bg-red-100 text-red-800";
    default:
      return "bg-gray-100 text-gray-600";
  }
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
const DAY_NAMES_FR = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
const DAY_NAMES_EN = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCalendarView({ items, onSelect }: WoCalendarViewProps) {
  const { t, i18n } = useTranslation("ot");
  const [mode, setMode] = useState<CalendarMode>("month");
  const [cursor, setCursor] = useState(() => new Date());

  const dayNames = i18n.language === "fr" ? DAY_NAMES_FR : DAY_NAMES_EN;

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
      else d.setDate(d.getDate() - 7);
      return d;
    });
  }, [mode]);

  const goNext = useCallback(() => {
    setCursor((c) => {
      const d = new Date(c);
      if (mode === "month") d.setMonth(d.getMonth() + 1);
      else d.setDate(d.getDate() + 7);
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
            {formatMonthYear(cursor, i18n.language)}
          </span>
        </div>

        <div className="flex items-center rounded-md border p-0.5 gap-0.5">
          {(["month", "week"] as CalendarMode[]).map((m) => (
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
                        className={`w-full text-left text-[9px] rounded px-1 py-0.5 truncate cursor-pointer ${urgencyColor(wo.urgency_id)}`}
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
                        className={`w-full text-left text-[10px] rounded px-1.5 py-1 cursor-pointer ${urgencyColor(wo.urgency_id)}`}
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
    </div>
  );
}
