/**
 * Date and time of day in one control (no label): a date field plus a 24-hour time field.
 * The value is `YYYY-MM-DDTHH:mm`, like the native datetime-local input it replaces, and is empty
 * until both parts are set.
 */

import { useEffect, useRef, useState } from "react";

import { DateField } from "@/components/ui/date-field";
import { TimeInput } from "@/components/ui/time-input";
import { joinDateTime, splitDateTime } from "@/lib/date-input";
import { cn } from "@/lib/utils";

export interface DateTimeFieldProps {
  id?: string;
  /** `YYYY-MM-DDTHH:mm`, or empty. */
  value: string;
  onChange: (next: string) => void;
  /** Inclusive bounds on the calendar day, `YYYY-MM-DD`. */
  min?: string | undefined;
  max?: string | undefined;
  isDateDisabled?: ((iso: string) => boolean) | undefined;
  disabledDayTitle?: string | undefined;
  disabled?: boolean;
  required?: boolean;
  size?: "sm" | "md";
  className?: string;
  /** Time used when a date is picked before any time is set. */
  defaultTime?: string;
  "aria-label"?: string;
  "aria-invalid"?: boolean;
}

export function DateTimeField({
  id,
  value,
  onChange,
  min,
  max,
  isDateDisabled,
  disabledDayTitle,
  disabled,
  required,
  size = "md",
  className,
  defaultTime = "08:00",
  "aria-label": ariaLabel,
  "aria-invalid": ariaInvalid,
}: DateTimeFieldProps) {
  // Bounds may be given as date-times; the calendar restricts the day.
  const minDay = min?.slice(0, 10);
  const maxDay = max?.slice(0, 10);
  const parts = splitDateTime(value);
  // The two parts are kept apart while one of them is still empty, so a date picked first is not
  // lost before the time is typed.
  const [date, setDate] = useState(parts.date);
  const [time, setTime] = useState(parts.time);
  const localRef = useRef({ date, time });
  localRef.current = { date, time };
  useEffect(() => {
    const local = joinDateTime(localRef.current.date, localRef.current.time);
    // The value already reads as the local parts, or it is empty only because a part is still
    // missing: keep what the user has typed.
    if (value === local || (value === "" && local === "")) return;
    setDate(parts.date);
    setTime(parts.time);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  function changeDate(next: string) {
    setDate(next);
    const nextTime = time || (next ? defaultTime : "");
    if (next && !time) setTime(nextTime);
    onChange(joinDateTime(next, nextTime));
  }

  function changeTime(next: string) {
    setTime(next);
    onChange(joinDateTime(date, next));
  }

  return (
    <div className={cn("flex items-start gap-2", className)}>
      <DateField
        {...(id ? { id } : {})}
        value={date}
        onChange={changeDate}
        min={minDay}
        max={maxDay}
        isDateDisabled={isDateDisabled}
        disabledDayTitle={disabledDayTitle}
        size={size}
        {...(disabled != null ? { disabled } : {})}
        {...(required != null ? { required } : {})}
        {...(ariaLabel ? { "aria-label": ariaLabel } : {})}
        {...(ariaInvalid != null ? { "aria-invalid": ariaInvalid } : {})}
        className="min-w-0 flex-1"
      />
      <TimeInput
        {...(id ? { id: `${id}-time` } : {})}
        value={time}
        onChange={changeTime}
        size={size}
        disabled={disabled || !date}
        className="w-[5.5rem] shrink-0"
      />
    </div>
  );
}
