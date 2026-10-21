/**
 * Month / week / day calendar for planning commitments + locked windows.
 * Mirrors PmCalendarView / DiCalendarView — not ResourceTimeline / Gantt.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  calendarCommitmentTone,
  calendarVisibleRangeIso,
  calendarWindowTone,
  commitmentCalendarLabel,
  eachLocalDayKey,
  localDayKeyFromDate,
  parseLocalIsoDateToDate,
  PLANNING_CALENDAR_COMMITMENT_BORDER,
  PLANNING_CALENDAR_WINDOW_BORDER,
  startOfPlanningWeek,
  type PlanningCalendarMode,
} from "@/components/planning/planning-model";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { PlanningWindow, ScheduleCommitment } from "@shared/ipc-types";

const MAX_CHIPS = 3;
const DAY_NAMES_FR = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
const DAY_NAMES_EN = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

export type PlanningCalendarChip =
  | { kind: "commitment"; commitment: ScheduleCommitment }
  | { kind: "window"; window: PlanningWindow };

type PlanningCalendarViewProps = {
  commitments: ScheduleCommitment[];
  windows: PlanningWindow[];
  loading?: boolean;
  /** Seed cursor when the calendar surface mounts (page From date). */
  initialDateIso: string;
  onVisibleRangeChange: (dateFrom: string, dateTo: string) => void;
  onCommitmentClick: (commitment: ScheduleCommitment) => void;
  onWindowClick: (window: PlanningWindow) => void;
};

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

function chipKey(chip: PlanningCalendarChip): string {
  return chip.kind === "commitment" ? `cmt-${chip.commitment.id}` : `win-${chip.window.id}`;
}

function chipBorderClass(chip: PlanningCalendarChip): string {
  if (chip.kind === "commitment") {
    return (
      PLANNING_CALENDAR_COMMITMENT_BORDER[calendarCommitmentTone(chip.commitment)] ??
      PLANNING_CALENDAR_COMMITMENT_BORDER.normal
    );
  }
  return (
    PLANNING_CALENDAR_WINDOW_BORDER[calendarWindowTone(chip.window)] ??
    PLANNING_CALENDAR_WINDOW_BORDER.other
  );
}

function chipPrimaryLabel(
  chip: PlanningCalendarChip,
  windowTypeLabel: (windowType: string) => string,
): string {
  if (chip.kind === "commitment") {
    return commitmentCalendarLabel(chip.commitment);
  }
  return windowTypeLabel(chip.window.window_type);
}

export function PlanningCalendarView({
  commitments,
  windows,
  loading,
  initialDateIso,
  onVisibleRangeChange,
  onCommitmentClick,
  onWindowClick,
}: PlanningCalendarViewProps) {
  const { t, i18n } = useTranslation("planning");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const [mode, setMode] = useState<PlanningCalendarMode>("month");
  const [cursor, setCursor] = useState(() => parseLocalIsoDateToDate(initialDateIso));
  const lastNotifiedRangeRef = useRef<string>("");

  const windowTypeLabel = useCallback(
    (windowType: string) =>
      t(`windows.types.${windowType}`, {
        defaultValue: windowType,
      }),
    [t],
  );

  const dayNames = i18n.language === "fr" ? DAY_NAMES_FR : DAY_NAMES_EN;

  const byDate = useMemo(() => {
    const map = new Map<string, PlanningCalendarChip[]>();

    const push = (key: string, chip: PlanningCalendarChip) => {
      const arr = map.get(key);
      if (arr) arr.push(chip);
      else map.set(key, [chip]);
    };

    for (const commitment of commitments) {
      const chip: PlanningCalendarChip = { kind: "commitment", commitment };
      for (const key of eachLocalDayKey(commitment.committed_start, commitment.committed_end)) {
        push(key, chip);
      }
    }

    for (const window of windows) {
      const chip: PlanningCalendarChip = { kind: "window", window };
      for (const key of eachLocalDayKey(window.start_datetime, window.end_datetime)) {
        push(key, chip);
      }
    }

    return map;
  }, [commitments, windows]);

  useEffect(() => {
    const range = calendarVisibleRangeIso(cursor, mode);
    const token = `${range.dateFrom}|${range.dateTo}`;
    if (lastNotifiedRangeRef.current === token) return;
    lastNotifiedRangeRef.current = token;
    onVisibleRangeChange(range.dateFrom, range.dateTo);
  }, [cursor, mode, onVisibleRangeChange]);

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
    const start = startOfPlanningWeek(cursor);
    return Array.from(
      { length: 7 },
      (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i),
    );
  }, [cursor]);

  const today = new Date();

  function handleChipClick(chip: PlanningCalendarChip) {
    if (chip.kind === "commitment") onCommitmentClick(chip.commitment);
    else onWindowClick(chip.window);
  }

  function renderChipButton(
    chip: PlanningCalendarChip,
    className: string,
    showTitleExtra?: boolean,
  ) {
    const label = chipPrimaryLabel(chip, windowTypeLabel);
    const title =
      chip.kind === "commitment"
        ? (chip.commitment.source_title ?? label)
        : chip.window.lock_reason?.trim() || label;
    return (
      <button
        key={chipKey(chip)}
        type="button"
        className={`${className} ${chipBorderClass(chip)}`}
        onClick={() => handleChipClick(chip)}
        title={title}
      >
        {showTitleExtra && chip.kind === "commitment" && chip.commitment.source_title ? (
          <>
            <span className="font-mono">{label}</span>
            <span className="ml-1 truncate">{chip.commitment.source_title}</span>
          </>
        ) : (
          label
        )}
      </button>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col rounded-lg border border-surface-border bg-surface-0">
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
            <span className="text-xs text-text-muted">{t("calendar.loading")}</span>
          ) : null}
        </div>

        <div className="flex items-center gap-0.5 rounded-md border border-surface-border p-0.5">
          {(["month", "week", "day"] as PlanningCalendarMode[]).map((m) => (
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

      <div className="flex flex-wrap gap-3 border-b border-surface-border px-4 py-1.5 text-[10px] text-text-muted">
        {(
          [
            ["commitment", PLANNING_CALENDAR_COMMITMENT_BORDER.normal],
            ["frozen", PLANNING_CALENDAR_COMMITMENT_BORDER.frozen],
            ["conflict", PLANNING_CALENDAR_COMMITMENT_BORDER.conflict],
            ["freeze", PLANNING_CALENDAR_WINDOW_BORDER.freeze],
            ["shutdown", PLANNING_CALENDAR_WINDOW_BORDER.shutdown],
            ["window", PLANNING_CALENDAR_WINDOW_BORDER.production],
          ] as const
        ).map(([key, border]) => (
          <span key={key} className={`border-l-2 pl-1.5 ${border}`}>
            {t(`calendar.legend.${key}`)}
          </span>
        ))}
      </div>

      {mode === "month" ? (
        <div className="min-h-0 flex-1 overflow-auto p-2">
          <div className="mb-1 grid grid-cols-7 text-center text-[10px] font-medium text-text-muted">
            {dayNames.map((d) => (
              <div key={d} className="py-1">
                {d}
              </div>
            ))}
          </div>
          <div className="grid grid-cols-7 gap-px bg-surface-border">
            {monthDays.map((day) => {
              const key = localDayKeyFromDate(day);
              const items = byDate.get(key) ?? [];
              const isCurrentMonth = day.getMonth() === cursor.getMonth();
              const isToday = isSameDay(day, today);
              return (
                <div
                  key={key}
                  className={`min-h-[80px] bg-surface-0 p-1 ${!isCurrentMonth ? "opacity-40" : ""}`}
                >
                  <span
                    className={`text-[10px] font-medium ${isToday ? "rounded-full bg-primary px-1.5 py-0.5 text-primary-foreground" : "text-text-muted"}`}
                  >
                    {day.getDate()}
                  </span>
                  <div className="mt-1 space-y-0.5">
                    {items
                      .slice(0, MAX_CHIPS)
                      .map((chip) =>
                        renderChipButton(
                          chip,
                          "w-full cursor-pointer truncate rounded border-l-2 bg-surface-1 px-1 py-0.5 text-left text-[9px] hover:bg-surface-2",
                        ),
                      )}
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
      ) : null}

      {mode === "week" ? (
        <div className="min-h-0 flex-1 overflow-auto p-2">
          <div className="grid grid-cols-7 gap-2">
            {weekDays.map((day) => {
              const key = localDayKeyFromDate(day);
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
                    {items.map((chip) =>
                      renderChipButton(
                        chip,
                        "w-full cursor-pointer rounded border-l-2 bg-surface-1 px-1.5 py-1 text-left text-[10px] hover:bg-surface-2",
                        true,
                      ),
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      ) : null}

      {mode === "day" ? (
        <div className="min-h-0 flex-1 overflow-auto p-4">
          {(() => {
            const key = localDayKeyFromDate(cursor);
            const items = byDate.get(key) ?? [];
            return items.length === 0 ? (
              <p className="py-8 text-center text-sm text-text-muted">{t("calendar.empty")}</p>
            ) : (
              <div className="space-y-2">
                {items.map((chip) => (
                  <button
                    key={chipKey(chip)}
                    type="button"
                    className={`w-full cursor-pointer rounded-lg border border-surface-border border-l-4 p-3 text-left transition-colors hover:bg-surface-1 ${chipBorderClass(chip)}`}
                    onClick={() => handleChipClick(chip)}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-xs text-text-muted">
                        {chipPrimaryLabel(chip, windowTypeLabel)}
                      </span>
                      {chip.kind === "commitment" ? (
                        <>
                          {chip.commitment.frozen_at ? (
                            <Badge variant="secondary" className="text-[10px]">
                              {t("inspector.frozen")}
                            </Badge>
                          ) : null}
                          {chip.commitment.has_blocking_conflict ? (
                            <Badge variant="destructive" className="text-[10px]">
                              {t("inspector.blockingConflict")}
                            </Badge>
                          ) : null}
                        </>
                      ) : (
                        <Badge variant="outline" className="border-0 bg-surface-1 text-[10px]">
                          {t(`windows.types.${chip.window.window_type}`, {
                            defaultValue: chip.window.window_type,
                          })}
                        </Badge>
                      )}
                    </div>
                    {chip.kind === "commitment" && chip.commitment.source_title ? (
                      <p className="mt-1 text-sm">{chip.commitment.source_title}</p>
                    ) : null}
                    {chip.kind === "window" && chip.window.lock_reason ? (
                      <p className="mt-1 text-sm text-text-secondary">{chip.window.lock_reason}</p>
                    ) : null}
                  </button>
                ))}
              </div>
            );
          })()}
        </div>
      ) : null}
    </div>
  );
}
