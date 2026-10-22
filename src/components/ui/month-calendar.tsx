/**
 * Month grid used by the date fields: Monday-first, month navigation, day cells.
 *
 * Day cells select on pointer-down: inside Radix dialogs and transformed containers a click can
 * be swallowed by the overlay, which is why the date fields never rely on the native date picker.
 */

import { ChevronLeft, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { isDateWithin, isIsoDate } from "@/lib/date-input";
import { cn } from "@/lib/utils";

function startOfMonth(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), 1);
}

function addMonths(d: Date, delta: number): Date {
  return new Date(d.getFullYear(), d.getMonth() + delta, 1);
}

export function toIsoDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function parseIso(iso: string | undefined): Date | null {
  if (!iso || !isIsoDate(iso)) return null;
  const [y = 0, m = 1, d = 1] = iso.split("-").map(Number);
  return new Date(y, m - 1, d);
}

function clampMonth(monthStart: Date, min?: string, max?: string): Date {
  let next = monthStart;
  const lo = parseIso(min);
  const hi = parseIso(max);
  if (lo && next.getTime() < startOfMonth(lo).getTime()) next = startOfMonth(lo);
  if (hi && next.getTime() > startOfMonth(hi).getTime()) next = startOfMonth(hi);
  return next;
}

export interface MonthCalendarProps {
  /** Selected day, `YYYY-MM-DD` or empty. */
  selected: string;
  onSelect: (iso: string) => void;
  /** Inclusive bounds, `YYYY-MM-DD`. */
  min?: string | undefined;
  max?: string | undefined;
  /** Extra per-day lock (for example a day with no team capacity left). */
  isDateDisabled?: ((iso: string) => boolean) | undefined;
  /** Title of a locked day (shown as a tooltip). */
  disabledDayTitle?: string | undefined;
}

export function MonthCalendar({
  selected,
  onSelect,
  min,
  max,
  isDateDisabled,
  disabledDayTitle,
}: MonthCalendarProps) {
  const { t, i18n } = useTranslation("common");
  const language = i18n.language || undefined;
  const [cursor, setCursor] = useState(() =>
    clampMonth(startOfMonth(parseIso(selected) ?? parseIso(min) ?? new Date()), min, max),
  );

  const minDate = parseIso(min);
  const maxDate = parseIso(max);
  const minMonth = minDate ? startOfMonth(minDate) : null;
  const maxMonth = maxDate ? startOfMonth(maxDate) : null;
  const canGoPrev = !minMonth || cursor.getTime() > minMonth.getTime();
  const canGoNext = !maxMonth || cursor.getTime() < maxMonth.getTime();

  const cells = useMemo(() => {
    const first = startOfMonth(cursor);
    const startPad = (first.getDay() + 6) % 7; // Monday first
    const daysInMonth = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
    const out: Array<{ iso: string; inMonth: boolean; day: number }> = [];
    for (let i = 0; i < startPad; i++) {
      const d = new Date(first.getFullYear(), first.getMonth(), 1 - (startPad - i));
      out.push({ iso: toIsoDate(d), inMonth: false, day: d.getDate() });
    }
    for (let day = 1; day <= daysInMonth; day++) {
      const d = new Date(first.getFullYear(), first.getMonth(), day);
      out.push({ iso: toIsoDate(d), inMonth: true, day });
    }
    let next = 1;
    while (out.length % 7 !== 0) {
      const d = new Date(first.getFullYear(), first.getMonth() + 1, next++);
      out.push({ iso: toIsoDate(d), inMonth: false, day: d.getDate() });
    }
    return out;
  }, [cursor]);

  const monthTitle = cursor.toLocaleDateString(language, { month: "long", year: "numeric" });
  const weekdayLabels = useMemo(() => {
    const base = new Date(2024, 0, 1); // a Monday
    return Array.from({ length: 7 }, (_, i) => {
      const d = new Date(base);
      d.setDate(base.getDate() + i);
      return d.toLocaleDateString(language, { weekday: "short" });
    });
  }, [language]);

  return (
    <div>
      <div className="mb-2 flex items-center justify-between gap-1">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 w-7 p-0"
          aria-label={t("dateField.prevMonth")}
          disabled={!canGoPrev}
          onClick={() => setCursor((c) => clampMonth(addMonths(c, -1), min, max))}
        >
          <ChevronLeft className="h-4 w-4" />
        </Button>
        <span className="text-sm font-medium capitalize">{monthTitle}</span>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 w-7 p-0"
          aria-label={t("dateField.nextMonth")}
          disabled={!canGoNext}
          onClick={() => setCursor((c) => clampMonth(addMonths(c, 1), min, max))}
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
        {cells.map((cell) => {
          const isSelected = cell.iso === selected;
          const inRange = isDateWithin(cell.iso, min, max);
          const blocked = Boolean(isDateDisabled?.(cell.iso));
          const selectable = inRange && !blocked;
          return (
            <button
              key={cell.iso}
              type="button"
              disabled={!selectable}
              title={blocked && inRange ? disabledDayTitle : undefined}
              aria-pressed={isSelected}
              className={cn(
                "h-8 rounded text-xs transition-colors",
                !selectable && "cursor-not-allowed text-text-muted/30",
                selectable && (cell.inMonth ? "text-text-primary" : "text-text-muted/50"),
                isSelected && selectable
                  ? "bg-primary text-primary-foreground"
                  : selectable && "hover:bg-surface-2",
              )}
              aria-label={new Date(`${cell.iso}T12:00:00`).toLocaleDateString(language, {
                weekday: "long",
                day: "numeric",
                month: "long",
                year: "numeric",
              })}
              onPointerDown={(e) => {
                if (!selectable || e.button !== 0) return;
                e.preventDefault();
                e.stopPropagation();
                onSelect(cell.iso);
              }}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                // Keyboard activation (Enter / Space) arrives as a click without a pointer-down.
                if (selectable && e.detail === 0) onSelect(cell.iso);
              }}
            >
              {cell.day}
            </button>
          );
        })}
      </div>
    </div>
  );
}
