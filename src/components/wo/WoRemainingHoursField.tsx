/**
 * WoRemainingHoursField.tsx
 *
 * Optional "estimated remaining time (hours)" of a resume or a reopening to in progress. The
 * permit gate then judges the end of the work as now + this time instead of now + the work
 * order's full expected duration (`permit::wo_gate`). It never changes the planned dates or the
 * expected duration; the backend records it on the resume's or reopening's history row (who,
 * when, the value). Left empty: the full expected duration.
 */

import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

/** The typed remaining time: `null` when empty, a number of hours > 0, or `"invalid"`. */
export function parseRemainingHours(text: string): number | null | "invalid" {
  const raw = text.trim();
  if (raw === "") return null;
  // Decimal comma (French) or point.
  if (!/^\d+(?:[.,]\d+)?$/.test(raw)) return "invalid";
  const hours = Number(raw.replace(",", "."));
  return Number.isFinite(hours) && hours > 0 ? hours : "invalid";
}

interface WoRemainingHoursFieldProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export function WoRemainingHoursField({
  id,
  value,
  onChange,
  disabled,
}: WoRemainingHoursFieldProps) {
  const { t } = useTranslation("ot");
  const invalid = parseRemainingHours(value) === "invalid";
  const hintId = `${id}-hint`;
  return (
    <div className="space-y-1">
      <Label htmlFor={id}>{t("remainingTime.label")}</Label>
      <Input
        id={id}
        inputMode="decimal"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
        aria-invalid={invalid}
        aria-describedby={hintId}
        placeholder={t("remainingTime.placeholder")}
      />
      <p
        id={hintId}
        className={invalid ? "text-xs text-destructive" : "text-xs text-muted-foreground"}
      >
        {invalid ? t("remainingTime.invalid") : t("remainingTime.hint")}
      </p>
    </div>
  );
}
