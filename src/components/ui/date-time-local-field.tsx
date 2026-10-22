/**
 * Labelled date + time field that works inside Radix dialogs and transformed containers.
 * The date is typed or chosen from a portaled calendar (dd/mm/yyyy in French), the time is a
 * 24-hour `HH:mm` field. The value is `YYYY-MM-DDTHH:mm`, empty until both parts are set.
 */

import { useTranslation } from "react-i18next";

import { DateTimeField } from "@/components/ui/date-time-field";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

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
  const { t } = useTranslation("planning");
  return (
    <div className={cn("space-y-1.5", className)}>
      <Label htmlFor={`${id}-date`}>{label}</Label>
      <DateTimeField
        id={`${id}-date`}
        value={value}
        onChange={onChange}
        min={minDate}
        max={maxDate}
        isDateDisabled={isDateDisabled}
        disabledDayTitle={t("commitment.capacityDayFull")}
        disabled={disabled}
        size="sm"
      />
    </div>
  );
}
