/**
 * Month / week / day calendar for PM occurrences (by due_at).
 * Mirrors DiCalendarView — not ResourceTimeline / Gantt.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { calendarOccurrenceTone, PM_CALENDAR_TONE_BORDER } from "@/components/pm/pm-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { PmOccurrence } from "@shared/ipc-types";

type CalendarMode = "month" | "week" | "day";

interface PmCalendarViewProps {
  occurrences: PmOccurrence[];
  loading?: boolean;
  onChipClick: (occ: PmOccurrence) => void;
}

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

/** Local calendar day key matching how the grid cells are built. */
function dateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** Map RFC3339 due_at to the same local day key as the grid. */
function dueAtLocalKey(dueAt: string): string | null {
  const d = new Date(dueAt);
  if (Number.isNaN(d.getTime())) return null;
  return dateKey(d);
}

const MAX_CHIPS = 3;
const DAY_NAMES_FR = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
const DAY_NAMES_EN = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function chipBorderClass(occ: PmOccurrence): string {
  return PM_CALENDAR_TONE_BORDER[calendarOccurrenceTone(occ)] ?? "border-l-gray-300";
}

function chipLabel(occ: PmOccurrence): string {
  return occ.plan_code?.trim() || occ.plan_title?.trim() || "PM";
}

export function PmCalendarView({ occurrences, loading, onChipClick }: PmCalendarViewProps) {
  const { t, i18n } = useTranslation("pm");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [mode, setMode] = useState<CalendarMode>("month");
  const [cursor, setCursor] = useState(() => new Date());

  const dayNames = i18n.language === "fr" ? DAY_NAMES_FR : DAY_NAMES_EN;

  const byDate = useMemo(() => {
    const map = new Map<string, PmOccurrence[]>();
    for (const occ of occurrences) {
      if (!occ.due_at) continue;
      const key = dueAtLocalKey(occ.due_at);
      if (!key) continue;
      const arr = map.get(key);
      if (arr) arr.push(occ);
      else map.set(key, [occ]);
    }
    return map;
  }, [occurrences]);

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

  const weekDays = useMemo(() => {
    const start = startOfWeek(cursor);
    return Array.from(
      { length: 7 },
      (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i),
    );
  }, [cursor]);

  const today = new Date();

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-4 py-2">
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
          <span className="text-sm font-semibold capitalize text-text-primary">
            {mode === "day"
              ? cursor.toLocaleDateString(dateLocale, {
                  weekday: "long",
                  day: "numeric",
                  month: "long",
                  year: "numeric",
                })
              : formatMonthYear(cursor, dateLocale)}
          </span>
          {loading ? (
            <span className="text-xs text-muted-foreground">{t("calendar.loading")}</span>
          ) : null}
        </div>

        <div className="flex items-center gap-0.5 rounded-md border p-0.5">
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

      <div className="flex flex-wrap gap-3 border-b border-surface-border px-4 py-1.5 text-[10px] text-muted-foreground">
        {(
          [
            ["overdue", "border-l-red-500"],
            ["today", "border-l-orange-500"],
            ["pipeline", "border-l-blue-500"],
            ["completed", "border-l-green-500"],
          ] as const
        ).map(([key, border]) => (
          <span key={key} className={`border-l-2 pl-1.5 ${border}`}>
            {t(`calendar.legend.${key}`)}
          </span>
        ))}
      </div>

      {mode === "month" && (
        <div className="flex-1 overflow-auto p-2">
          <div className="mb-1 grid grid-cols-7 text-center text-[10px] font-medium text-text-muted">
            {dayNames.map((d) => (
              <div key={d} className="py-1">
                {d}
              </div>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-px bg-surface-border">
            {monthDays.map((day) => {
              const key = dateKey(day);
              const items = byDate.get(key) ?? [];
              const isCurrentMonth = day.getMonth() === cursor.getMonth();
              const isToday = isSameDay(day, today);
              return (
                <div
                  key={key}
                  className={`min-h-[80px] bg-surface-0 p-1 ${!isCurrentMonth ? "opacity-40" : ""}`}
                >
                  <span
                    className={`text-[10px] font-medium ${isToday ? "rounded-full bg-brand-primary px-1.5 py-0.5 text-white" : "text-text-muted"}`}
                  >
                    {day.getDate()}
                  </span>
                  <div className="mt-1 space-y-0.5">
                    {items.slice(0, MAX_CHIPS).map((occ) => (
                      <button
                        key={occ.id}
                        type="button"
                        className={`w-full cursor-pointer truncate rounded border-l-2 bg-surface-1 px-1 py-0.5 text-left text-[9px] hover:bg-surface-2 ${chipBorderClass(occ)}`}
                        onClick={() => onChipClick(occ)}
                        title={occ.plan_title ?? chipLabel(occ)}
                      >
                        {chipLabel(occ)}
                      </button>
                    ))}
                    {items.length > MAX_CHIPS ? (
                      <span className="text-[9px] text-text-muted">
                        {t("calendar.more", { count: items.length - MAX_CHIPS })}
                      </span>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {mode === "week" && (
        <div className="flex-1 overflow-auto p-2">
          <div className="grid grid-cols-7 gap-2">
            {weekDays.map((day) => {
              const key = dateKey(day);
              const items = byDate.get(key) ?? [];
              const isToday = isSameDay(day, today);
              return (
                <div key={key} className="min-h-[200px] rounded border border-surface-border p-2">
                  <div
                    className={`mb-2 text-xs font-medium ${isToday ? "text-brand-primary" : "text-text-muted"}`}
                  >
                    {day.toLocaleDateString(dateLocale, { weekday: "short", day: "numeric" })}
                  </div>
                  <div className="space-y-1">
                    {items.map((occ) => (
                      <button
                        key={occ.id}
                        type="button"
                        className={`w-full cursor-pointer rounded border-l-2 bg-surface-1 px-1.5 py-1 text-left text-[10px] hover:bg-surface-2 ${chipBorderClass(occ)}`}
                        onClick={() => onChipClick(occ)}
                      >
                        <span className="font-mono">{chipLabel(occ)}</span>
                        {occ.plan_title ? (
                          <span className="ml-1 truncate">{occ.plan_title}</span>
                        ) : null}
                      </button>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {mode === "day" && (
        <div className="flex-1 overflow-auto p-4">
          {(() => {
            const key = dateKey(cursor);
            const items = byDate.get(key) ?? [];
            return items.length === 0 ? (
              <p className="py-8 text-center text-sm text-text-muted">{t("calendar.empty")}</p>
            ) : (
              <div className="space-y-2">
                {items.map((occ) => (
                  <button
                    key={occ.id}
                    type="button"
                    className={`w-full cursor-pointer rounded-lg border border-surface-border border-l-4 p-3 text-left transition-colors hover:bg-surface-1 ${chipBorderClass(occ)}`}
                    onClick={() => onChipClick(occ)}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-xs text-text-muted">{chipLabel(occ)}</span>
                      <Badge variant="outline" className="border-0 bg-surface-1 text-[10px]">
                        {t(`occurrenceStatus.${occ.status}`, { defaultValue: occ.status })}
                      </Badge>
                    </div>
                    {occ.plan_title ? <p className="mt-1 text-sm">{occ.plan_title}</p> : null}
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
