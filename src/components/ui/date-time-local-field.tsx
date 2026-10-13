/**
 * Date + time field that works inside Radix Dialogs / transformed containers.
 * Native `datetime-local` pickers are clipped or inert under dialog transform + overflow
 * in WebView2; date is chosen via a portaled popover calendar, time via `type="time"`.
 */

import { Calendar as CalendarIcon, ChevronLeft, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

function splitValue(value: string): { date: string; time: string } {
  if (!value.trim()) return { date: "", time: "" };
  const [date = "", timePart = ""] = value.split("T");
  return { date, time: timePart.slice(0, 5) };
}

function joinValue(date: string, time: string): string {
  if (!date && !time) return "";
  if (!date || !time) return "";
  return `${date}T${time}`;
}

function startOfMonth(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

function addMonths(d: Date, delta: number): Date {
  return new Date(d.getFullYear(), d.getMonth() + delta, 1);
}

function toIsoDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function parseIsoDate(iso: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(iso)) return null;
  const [y, m, day] = iso.split("-").map(Number);
  if (y == null || m == null || day == null) return null;
  const d = new Date(y, m - 1, day);
  if (Number.isNaN(d.getTime())) return null;
  return d;
}

function isDateInRange(iso: string, minDate?: string, maxDate?: string): boolean {
  if (minDate && iso < minDate) return false;
  if (maxDate && iso > maxDate) return false;
  return true;
}

function clampMonthCursor(monthStart: Date, minDate?: string, maxDate?: string): Date {
  let next = monthStart;
  const min = minDate ? parseIsoDate(minDate) : null;
  const max = maxDate ? parseIsoDate(maxDate) : null;
  if (min && next.getTime() < startOfMonth(min).getTime()) {
    next = startOfMonth(min);
  }
  if (max && next.getTime() > startOfMonth(max).getTime()) {
    next = startOfMonth(max);
  }
  return next;
}

type DateTimeLocalFieldProps = {
  id: string;
  label: string;
  value: string;
  onChange: (next: string) => void;
  disabled?: boolean;
  className?: string;
  /** Inclusive lower bound `YYYY-MM-DD` (calendar day). */
  minDate?: string;
  /** Inclusive upper bound `YYYY-MM-DD` (calendar day). */
  maxDate?: string;
  /** Extra calendar-day disable (e.g. no remaining team capacity). */
  isDateDisabled?: (isoDate: string) => boolean;
};

export function DateTimeLocalField({
  id,
  label,
  value,
  onChange,
  disabled = false,
  className,
  minDate,
  maxDate,
  isDateDisabled,
}: DateTimeLocalFieldProps) {
  const { t, i18n } = useTranslation("planning");
  const { date, time } = splitValue(value);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(() =>
    clampMonthCursor(
      startOfMonth(parseIsoDate(date) ?? parseIsoDate(minDate ?? "") ?? new Date()),
      minDate,
      maxDate,
    ),
  );

  const minMonth = minDate ? startOfMonth(parseIsoDate(minDate) ?? new Date(0)) : null;
  const maxMonth = maxDate ? startOfMonth(parseIsoDate(maxDate) ?? new Date(0)) : null;
  const canGoPrev = !minMonth || cursor.getTime() > minMonth.getTime();
  const canGoNext = !maxMonth || cursor.getTime() < maxMonth.getTime();

  const weeks = useMemo(() => {
    const first = startOfMonth(cursor);
    const startPad = (first.getDay() + 6) % 7; // Monday-first
    const daysInMonth = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
    const cells: Array<{ iso: string; inMonth: boolean; day: number }[]> = [];
    let week: Array<{ iso: string; inMonth: boolean; day: number }> = [];

    for (let i = 0; i < startPad; i++) {
      const d = new Date(first.getFullYear(), first.getMonth(), 1 - (startPad - i));
      week.push({ iso: toIsoDate(d), inMonth: false, day: d.getDate() });
    }
    for (let day = 1; day <= daysInMonth; day++) {
      const d = new Date(first.getFullYear(), first.getMonth(), day);
      week.push({ iso: toIsoDate(d), inMonth: true, day });
      if (week.length === 7) {
        cells.push(week);
        week = [];
      }
    }
    if (week.length > 0) {
      let next = 1;
      while (week.length < 7) {
        const d = new Date(first.getFullYear(), first.getMonth() + 1, next++);
        week.push({ iso: toIsoDate(d), inMonth: false, day: d.getDate() });
      }
      cells.push(week);
    }
    return cells;
  }, [cursor]);

  const monthTitle = cursor.toLocaleDateString(i18n.language || undefined, {
    month: "long",
    year: "numeric",
  });

  const weekdayLabels = useMemo(() => {
    const base = new Date(2024, 0, 1); // Monday
    return Array.from({ length: 7 }, (_, i) => {
      const d = new Date(base);
      d.setDate(base.getDate() + i);
      return d.toLocaleDateString(i18n.language || undefined, { weekday: "short" });
    });
  }, [i18n.language]);

  function setDatePart(nextDate: string) {
    if (!isDateInRange(nextDate, minDate, maxDate)) return;
    if (isDateDisabled?.(nextDate)) return;
    const nextTime = time || "08:00";
    onChange(joinValue(nextDate, nextTime));
    setOpen(false);
  }

  function onTimeChange(nextTime: string) {
    if (!date) return;
    onChange(joinValue(date, nextTime));
  }

  return (
    <div className={cn("space-y-1.5", className)}>
      <Label htmlFor={`${id}-date`}>{label}</Label>
      <div className="grid grid-cols-[1fr_auto] gap-2">
        <Popover
          open={open}
          onOpenChange={(next) => {
            if (disabled) return;
            if (next) {
              setCursor(
                clampMonthCursor(
                  startOfMonth(parseIsoDate(date) ?? parseIsoDate(minDate ?? "") ?? new Date()),
                  minDate,
                  maxDate,
                ),
              );
            }
            setOpen(next);
          }}
        >
          <PopoverTrigger asChild>
            <Button
              id={`${id}-date`}
              type="button"
              variant="outline"
              disabled={disabled}
              className="h-8 justify-start px-2 text-left text-sm font-normal"
              aria-label={t("commitment.pickDate")}
            >
              <CalendarIcon className="mr-2 h-3.5 w-3.5 shrink-0 text-text-muted" aria-hidden />
              <span className={cn(!date && "text-text-muted")}>
                {date || t("commitment.pickDate")}
              </span>
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align="start"
            side="bottom"
            className="z-[200] w-[280px] p-2"
            onPointerDown={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <div className="mb-2 flex items-center justify-between gap-1">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 w-7 p-0"
                aria-label={t("commitment.prevMonth")}
                disabled={!canGoPrev}
                onClick={() =>
                  setCursor((c) => clampMonthCursor(addMonths(c, -1), minDate, maxDate))
                }
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              <span className="text-sm font-medium capitalize">{monthTitle}</span>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-7 w-7 p-0"
                aria-label={t("commitment.nextMonth")}
                disabled={!canGoNext}
                onClick={() =>
                  setCursor((c) => clampMonthCursor(addMonths(c, 1), minDate, maxDate))
                }
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
            <div className="mb-1 grid grid-cols-7 gap-0.5 text-center text-[10px] text-text-muted">
              {weekdayLabels.map((w) => (
                <div key={w} className="py-1">
                  {w}
                </div>
              ))}
            </div>
            <div className="grid grid-cols-7 gap-0.5">
              {weeks.flat().map((cell) => {
                const selected = cell.iso === date;
                const inRange = isDateInRange(cell.iso, minDate, maxDate);
                const blocked = Boolean(isDateDisabled?.(cell.iso));
                const selectable = inRange && !blocked;
                return (
                  <button
                    key={cell.iso}
                    type="button"
                    disabled={!selectable}
                    title={blocked && inRange ? t("commitment.capacityDayFull") : undefined}
                    className={cn(
                      "h-8 rounded text-xs transition-colors",
                      !selectable && "cursor-not-allowed text-text-muted/30",
                      selectable && (cell.inMonth ? "text-text-primary" : "text-text-muted/50"),
                      selected && selectable
                        ? "bg-primary text-primary-foreground"
                        : selectable && "hover:bg-surface-2",
                    )}
                    onPointerDown={(e) => {
                      if (!selectable) return;
                      e.preventDefault();
                      e.stopPropagation();
                      setDatePart(cell.iso);
                    }}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                    }}
                  >
                    {cell.day}
                  </button>
                );
              })}
            </div>
          </PopoverContent>
        </Popover>

        <Input
          id={`${id}-time`}
          type="time"
          step={60}
          disabled={disabled || !date}
          value={time}
          aria-label={t("commitment.time")}
          className="h-8 w-[7.5rem] text-sm"
          onChange={(e) => onTimeChange(e.target.value)}
          onClick={(e) => {
            const el = e.currentTarget;
            try {
              el.showPicker?.();
            } catch {
              /* WebView may reject; typing still works */
            }
          }}
        />
      </div>
    </div>
  );
}
