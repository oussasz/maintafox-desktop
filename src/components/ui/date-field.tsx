/**
 * Date field: text in the app language (dd/mm/yyyy in French) plus a calendar popover.
 *
 * Replaces the native date input, whose format follows the Windows region (mm/dd/yyyy in French
 * screens) and whose built-in calendar icon is clipped in narrow panels. The value stays an ISO
 * `YYYY-MM-DD` string (empty when unset), so stored data and sync are unchanged.
 */

import { Calendar as CalendarIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { MonthCalendar } from "@/components/ui/month-calendar";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  dateInputPlaceholder,
  formatIsoDateForInput,
  isDateWithin,
  isIsoDate,
  parseDateInput,
  splitFieldClasses,
} from "@/lib/date-input";
import { cn } from "@/lib/utils";

export interface DateFieldProps {
  id?: string;
  name?: string;
  /** `YYYY-MM-DD`, or empty when no date is set. */
  value: string;
  onChange: (next: string) => void;
  /** Inclusive bounds, `YYYY-MM-DD`. */
  min?: string | undefined;
  max?: string | undefined;
  /** Extra per-day lock in the calendar (for example no capacity left that day). */
  isDateDisabled?: ((iso: string) => boolean) | undefined;
  /** Tooltip of a day locked by `isDateDisabled`. */
  disabledDayTitle?: string | undefined;
  disabled?: boolean;
  required?: boolean;
  /** `sm` matches compact dialogs (h-8). */
  size?: "sm" | "md";
  className?: string;
  placeholder?: string;
  "aria-label"?: string | undefined;
  "aria-describedby"?: string | undefined;
  "aria-invalid"?: boolean | undefined;
  onBlur?: () => void;
}

export function DateField({
  id,
  name,
  value,
  onChange,
  min,
  max,
  isDateDisabled,
  disabledDayTitle,
  disabled = false,
  required,
  size = "md",
  className,
  placeholder,
  "aria-label": ariaLabel,
  "aria-describedby": ariaDescribedBy,
  "aria-invalid": ariaInvalid,
  onBlur,
}: DateFieldProps) {
  const { t, i18n } = useTranslation("common");
  const language = i18n.language;
  const compact = size === "sm";
  const [draft, setDraft] = useState(() => formatIsoDateForInput(value, language));
  const [open, setOpen] = useState(false);
  const draftRef = useRef(draft);
  draftRef.current = draft;

  // The last value this field passed to its parent: the change coming back is its own and must
  // not rewrite the text being typed (a short year such as 03/11/20 is not yet a value).
  const emitted = useRef<string | null>(null);
  function emit(next: string) {
    emitted.current = next;
    onChange(next);
  }

  // A value changed from outside (reset, load) replaces the text.
  useEffect(() => {
    if (emitted.current === value) {
      emitted.current = null;
      return;
    }
    const typed = parseDateInput(draftRef.current, language) ?? "";
    if (typed !== value) setDraft(formatIsoDateForInput(value, language));
  }, [value, language]);

  const typedIso = parseDateInput(draft, language);
  const outOfRange = typedIso != null && !isDateWithin(typedIso, min, max);
  const invalid = draft.trim() !== "" && (typedIso == null || outOfRange);

  function onText(next: string) {
    setDraft(next);
    // A date outside the bounds is passed on too and flagged, so the form can say why it refuses it.
    // Only a finished date is passed on while typing (a four-digit year or a full ISO date), so the
    // form does not see 2020 on the way to 2026; a short year is read when the field is left or
    // Enter is pressed.
    const finished = /\d{4}$/.test(next.trim()) || isIsoDate(next.trim());
    const iso = finished ? parseDateInput(next, language) : null;
    if (iso) {
      if (iso !== value) emit(iso);
    } else if (value !== "") {
      // An unfinished or impossible date is no date, as with a native field.
      emit("");
    }
  }

  function commitText() {
    const iso = parseDateInput(draft, language);
    if (iso) {
      setDraft(formatIsoDateForInput(iso, language));
      if (iso !== value) emit(iso);
    }
  }

  const label = ariaLabel ?? t("dateField.pickDate");
  const classes = splitFieldClasses(className);

  return (
    <div className={cn("relative", classes.wrapper)}>
      <Input
        id={id}
        name={name}
        type="text"
        inputMode="numeric"
        autoComplete="off"
        value={draft}
        disabled={disabled}
        required={required}
        placeholder={placeholder ?? dateInputPlaceholder(language)}
        aria-label={ariaLabel}
        aria-describedby={ariaDescribedBy}
        aria-invalid={ariaInvalid ?? (invalid || undefined)}
        title={invalid ? t("dateField.invalid") : undefined}
        className={cn(
          "min-w-0 tabular-nums",
          compact && "h-8 text-sm",
          invalid && "border-destructive focus-visible:ring-destructive",
          classes.input,
          "pr-9",
        )}
        onChange={(e) => onText(e.target.value)}
        onBlur={() => {
          commitText();
          onBlur?.();
        }}
        onKeyDown={(e) => {
          // Enter submits the surrounding form without leaving the field: read a short year first.
          if (e.key === "Enter") commitText();
        }}
      />
      <Popover open={open} onOpenChange={(next) => !disabled && setOpen(next)}>
        <PopoverTrigger asChild>
          <button
            type="button"
            disabled={disabled}
            aria-label={label}
            title={label}
            className={cn(
              "absolute right-1 top-1/2 inline-flex -translate-y-1/2 items-center justify-center rounded text-text-muted hover:bg-surface-2 hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-50",
              compact ? "h-6 w-6" : "h-7 w-7",
            )}
          >
            <CalendarIcon className="h-4 w-4" aria-hidden />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="end"
          side="bottom"
          className="z-[200] w-[280px] p-2"
          onPointerDown={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <MonthCalendar
            selected={typedIso ?? value}
            min={min}
            max={max}
            isDateDisabled={isDateDisabled}
            disabledDayTitle={disabledDayTitle}
            onSelect={(iso) => {
              setDraft(formatIsoDateForInput(iso, language));
              if (iso !== value) emit(iso);
              setOpen(false);
            }}
          />
        </PopoverContent>
      </Popover>
    </div>
  );
}
