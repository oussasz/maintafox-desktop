/**
 * Month / week / day calendar for PM occurrences (by due_at).
 * Mirrors DiCalendarView — not ResourceTimeline / Gantt.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { calendarOccurrenceTone, PM_CALENDAR_TONE_BORDER } from "@/components/pm/pm-model";
import { Button } from "@/components/ui/button";
import type { PmOccurrenceRow as PmOccurrence } from "@/services/pm-service";
import { intlLocaleForLanguage } from "@/utils/format-date";

import { formatPmAsset, PmOccurrenceStatus } from "./pm-cells";

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

/** Chips shown in a month cell before the "+N more" link to the day list. */
const MAX_CHIPS = 3;

/** Monday-first short weekday names in the app language. */
function weekdayNames(locale: string): string[] {
  // 2024-01-01 is a Monday.
  return Array.from({ length: 7 }, (_, i) =>
    new Date(2024, 0, 1 + i).toLocaleDateString(locale, { weekday: "short" }),
  );
}

function chipBorderClass(occ: PmOccurrence): string {
  return PM_CALENDAR_TONE_BORDER[calendarOccurrenceTone(occ)];
}

function chipLabel(occ: PmOccurrence): string {
  return occ.plan_code?.trim() || occ.plan_title?.trim() || "";
}

function chipTitle(occ: PmOccurrence): string {
  return [occ.plan_title, formatPmAsset(occ.asset_code, occ.asset_name)]
    .filter(Boolean)
    .join(" · ");
}

export function PmCalendarView({ occurrences, loading, onChipClick }: PmCalendarViewProps) {
  const { t, i18n } = useTranslation("pm");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [mode, setMode] = useState<CalendarMode>("month");
  const [cursor, setCursor] = useState(() => new Date());

  const dayNames = useMemo(() => weekdayNames(dateLocale), [dateLocale]);

  const openDay = useCallback((day: Date) => {
    setCursor(day);
    setMode("day");
  }, []);

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
          <Button
            variant="outline"
            size="sm"
            className="h-9 w-9 p-0"
            onClick={goPrev}
            aria-label={t("calendar.previous")}
            title={t("calendar.previous")}
          >
            <ChevronLeft className="h-3.5 w-3.5" aria-hidden />
          </Button>
          <Button variant="outline" size="sm" className="h-9 px-3 text-xs" onClick={goToday}>
            {t("calendar.today")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-9 w-9 p-0"
            onClick={goNext}
            aria-label={t("calendar.next")}
            title={t("calendar.next")}
          >
            <ChevronRight className="h-3.5 w-3.5" aria-hidden />
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
            <span role="status" className="text-xs text-text-muted">
              {t("calendar.loading")}
            </span>
          ) : null}
        </div>

        <div className="flex items-center gap-0.5 rounded-md border p-0.5" role="group">
          {(["month", "week", "day"] as CalendarMode[]).map((m) => (
            <Button
              key={m}
              variant={mode === m ? "default" : "ghost"}
              size="sm"
              className="h-8 px-2 text-xs"
              aria-pressed={mode === m}
              onClick={() => setMode(m)}
            >
              {t(`calendar.mode.${m}`)}
            </Button>
          ))}
        </div>
      </div>

      <div className="flex flex-wrap gap-3 border-b border-surface-border px-4 py-1.5 text-xs text-text-muted">
        {(["overdue", "today", "pipeline", "completed"] as const).map((key) => (
          <span key={key} className={`border-l-2 pl-1.5 ${PM_CALENDAR_TONE_BORDER[key]}`}>
            {t(`calendar.legend.${key}`)}
          </span>
        ))}
      </div>

      {mode === "month" && (
        <div className="flex-1 overflow-auto p-2">
          <div className="mb-1 grid grid-cols-7 text-center text-xs font-medium text-text-muted">
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
                    className={`text-xs font-medium ${isToday ? "rounded-full bg-primary px-1.5 py-0.5 text-primary-foreground" : "text-text-muted"}`}
                  >
                    {day.getDate()}
                  </span>
                  <div className="mt-1 space-y-0.5">
                    {items.slice(0, MAX_CHIPS).map((occ) => (
                      <button
                        key={occ.id}
                        type="button"
                        className={`min-h-6 w-full cursor-pointer truncate rounded border-l-2 bg-surface-1 px-1.5 py-0.5 text-start text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${chipBorderClass(occ)}`}
                        onClick={() => onChipClick(occ)}
                        title={chipTitle(occ)}
                      >
                        {chipLabel(occ)}
                      </button>
                    ))}
                    {items.length > MAX_CHIPS ? (
                      <button
                        type="button"
                        className="text-xs text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        onClick={() => openDay(day)}
                        aria-label={t("calendar.moreAria", {
                          count: items.length,
                          date: day.toLocaleDateString(dateLocale, {
                            day: "numeric",
                            month: "long",
                          }),
                        })}
                      >
                        {t("calendar.more", { count: items.length - MAX_CHIPS })}
                      </button>
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
                    className={`mb-2 text-xs font-medium ${isToday ? "text-primary" : "text-text-muted"}`}
                  >
                    {day.toLocaleDateString(dateLocale, { weekday: "short", day: "numeric" })}
                  </div>
                  <div className="space-y-1">
                    {items.map((occ) => (
                      <button
                        key={occ.id}
                        type="button"
                        className={`w-full cursor-pointer rounded border-l-2 bg-surface-1 px-1.5 py-1 text-start text-xs hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${chipBorderClass(occ)}`}
                        onClick={() => onChipClick(occ)}
                        title={chipTitle(occ)}
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
                <h2 className="text-sm font-medium text-text-primary">
                  {t("calendar.dayListTitle", {
                    date: cursor.toLocaleDateString(dateLocale, {
                      day: "numeric",
                      month: "long",
                      year: "numeric",
                    }),
                  })}
                </h2>
                {items.map((occ) => (
                  <button
                    key={occ.id}
                    type="button"
                    className={`w-full cursor-pointer rounded-lg border border-surface-border border-l-4 p-3 text-left transition-colors hover:bg-surface-1 ${chipBorderClass(occ)}`}
                    onClick={() => onChipClick(occ)}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-xs text-text-muted">{chipLabel(occ)}</span>
                      <PmOccurrenceStatus occurrence={occ} />
                    </div>
                    {occ.plan_title ? <p className="mt-1 text-sm">{occ.plan_title}</p> : null}
                    {formatPmAsset(occ.asset_code, occ.asset_name) ? (
                      <p className="text-xs text-text-muted">
                        {formatPmAsset(occ.asset_code, occ.asset_name)}
                      </p>
                    ) : null}
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
