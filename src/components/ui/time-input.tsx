import * as React from "react";
import { useTranslation } from "react-i18next";

import { Input, type InputProps } from "@/components/ui/input";
import { parseTimeInput } from "@/lib/date-input";
import { cn } from "@/lib/utils";

export type TimeInputProps = Omit<InputProps, "type" | "step" | "value" | "onChange" | "size"> & {
  /** Controlled value in `HH:mm` (24h). Empty string when unset. */
  value: string;
  onChange: (value: string) => void;
  /** `sm` matches compact dialogs (h-8). */
  size?: "sm" | "md";
};

/**
 * Time of day on a 24-hour clock, typed as `HH:mm` (also `8h05`, `0805`). Replaces the native
 * time input, which shows AM/PM according to the Windows region. The value stays `HH:mm`.
 */
export const TimeInput = React.forwardRef<HTMLInputElement, TimeInputProps>(
  ({ className, value, onChange, disabled, size = "md", placeholder, onBlur, ...props }, ref) => {
    const { t } = useTranslation("common");
    const [draft, setDraft] = React.useState(value);
    const draftRef = React.useRef(draft);
    draftRef.current = draft;

    // A value changed from outside replaces the text; text that already reads as the current
    // value is left as typed.
    React.useEffect(() => {
      if ((parseTimeInput(draftRef.current) ?? "") !== value) setDraft(value);
    }, [value]);

    const typed = parseTimeInput(draft);
    const invalid = draft.trim() !== "" && typed == null;

    function onText(next: string) {
      // Digits and one separator only; a colon is added after the hour when two digits are typed.
      let text = next.replace(/[^0-9:hH]/g, "").slice(0, 5);
      if (/^\d{2}$/.test(text) && draft.length < text.length) text = `${text}:`;
      setDraft(text);
      const parsed = /^\d{1,2}[:hH]\d{2}$|^\d{3,4}$/.test(text) ? parseTimeInput(text) : null;
      if (parsed) {
        if (parsed !== value) onChange(parsed);
      } else if (value !== "") {
        onChange("");
      }
    }

    return (
      <Input
        ref={ref}
        type="text"
        inputMode="numeric"
        autoComplete="off"
        value={draft}
        disabled={disabled}
        placeholder={placeholder ?? t("dateField.timePlaceholder")}
        aria-label={t("dateField.timeLabel")}
        {...props}
        aria-invalid={invalid || undefined}
        title={invalid ? t("dateField.invalidTime") : undefined}
        maxLength={5}
        className={cn(
          "font-mono tabular-nums",
          size === "sm" ? "h-8 text-sm" : "h-10",
          invalid && "border-destructive focus-visible:ring-destructive",
          className,
        )}
        onChange={(e) => onText(e.target.value)}
        onBlur={(e) => {
          if (typed) setDraft(typed);
          onBlur?.(e);
        }}
      />
    );
  },
);
TimeInput.displayName = "TimeInput";
