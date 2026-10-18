/**
 * Whether a part line is required for the job or optional. Planning blocks a commitment on a
 * short required part and only warns on a short optional one. There is no preselected answer:
 * a new line stays unanswered until the user picks one, and the line cannot be saved before.
 */
import { useId } from "react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { cn } from "@/lib/utils";

export function PartNecessityChoice({
  value,
  onChange,
  disabled = false,
  partLabel,
  className,
}: {
  /** null: not answered yet. */
  value: boolean | null;
  onChange: (required: boolean) => void;
  disabled?: boolean;
  /** The part this choice is for (accessible name of the group). */
  partLabel?: string;
  className?: string;
}) {
  const { t } = useTranslation("common");
  const name = useId();
  const hintId = `${name}-hint`;
  const options: { value: boolean; label: string }[] = [
    { value: true, label: t("partNecessity.required") },
    { value: false, label: t("partNecessity.optional") },
  ];
  return (
    <div className={cn("space-y-1", className)}>
      <div
        role="radiogroup"
        aria-label={
          partLabel ? t("partNecessity.legendFor", { part: partLabel }) : t("partNecessity.legend")
        }
        aria-describedby={hintId}
        className="flex flex-wrap gap-3"
      >
        {options.map((option) => (
          <label
            key={String(option.value)}
            className={cn(
              "flex min-h-[36px] items-center gap-1.5 text-sm",
              disabled ? "cursor-not-allowed opacity-60" : "cursor-pointer",
            )}
          >
            <input
              type="radio"
              name={name}
              className="h-4 w-4 accent-primary"
              checked={value === option.value}
              disabled={disabled}
              onChange={() => onChange(option.value)}
            />
            {option.label}
          </label>
        ))}
      </div>
      <p
        id={hintId}
        className={cn("text-xs", value === null ? "text-tone-warning-fg" : "text-text-muted")}
      >
        {value === null
          ? t("partNecessity.choose")
          : value
            ? t("partNecessity.requiredHint")
            : t("partNecessity.optionalHint")}
      </p>
    </div>
  );
}

/** Marker of an optional part line (a required line carries none). */
export function PartOptionalChip({ className }: { className?: string }) {
  const { t } = useTranslation("common");
  return (
    <StatusChip
      tone="neutral"
      label={t("partNecessity.optional")}
      title={t("partNecessity.optionalHint")}
      {...(className ? { className } : {})}
    />
  );
}
