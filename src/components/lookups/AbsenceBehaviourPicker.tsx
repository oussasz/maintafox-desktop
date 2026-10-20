/**
 * Behaviour of an absence type (PERSONNEL.ABSENCE_TYPES): the program behaviour the value names,
 * stored in `semantic_tag` and required (checked on the server, `personnel::reference_lists`).
 * `critical` makes every absence of the type critical for planning (the neutral tag "Absent —
 * critical for planning" a planner without the sensitive-data right sees, decision register
 * 2026-10-19).
 *
 * The three tags are behaviours fixed in the program (server `ABSENCE_TAGS`), not a list a plant
 * configures; each plant's absence types are the values of the list and name one of them.
 */

import { useTranslation } from "react-i18next";

import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export const ABSENCE_TYPES_DOMAIN = "PERSONNEL.ABSENCE_TYPES";

/** Program behaviours of an absence type (server `personnel::reference_lists::ABSENCE_TAGS`). */
export const ABSENCE_BEHAVIOUR_TAGS = ["leave", "critical", "training"] as const;

export function isAbsenceTypesDomain(code: string | null | undefined): boolean {
  return (code ?? "").trim().toUpperCase() === ABSENCE_TYPES_DOMAIN;
}

/**
 * Removing the `critical` tag does not change the absences already recorded: they were stored
 * critical, and stay so (the warning says it).
 */
export function removesCriticalTag(
  before: string | null | undefined,
  after: string | null,
): boolean {
  return before === "critical" && after !== "critical";
}

interface AbsenceBehaviourSelectProps {
  value: string | null;
  onChange: (tag: string | null) => void;
  disabled?: boolean;
}

export function AbsenceBehaviourSelect({ value, onChange, disabled }: AbsenceBehaviourSelectProps) {
  const { t } = useTranslation("reference");
  return (
    <select
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
      aria-label={t("absenceBehaviour.column")}
      title={t("absenceBehaviour.hint")}
      disabled={disabled}
      className={cn(mfInput.nativeSelect, "h-9")}
    >
      {/* Required, no default: an absence type without a behaviour is refused on save. */}
      <option value="" disabled>
        {t("absenceBehaviour.placeholder")}
      </option>
      {ABSENCE_BEHAVIOUR_TAGS.map((tag) => (
        <option key={tag} value={tag}>
          {t(`absenceBehaviour.tags.${tag}`)}
        </option>
      ))}
    </select>
  );
}

/** Read-only cell: the behaviour's name (a stored tag the program does not know shows as is). */
export function AbsenceBehaviourText({ value }: { value: string | null }) {
  const { t } = useTranslation("reference");
  if (!value) return <span className="text-xs text-text-muted">—</span>;
  const known = (ABSENCE_BEHAVIOUR_TAGS as readonly string[]).includes(value);
  return <span className="text-sm">{known ? t(`absenceBehaviour.tags.${value}`) : value}</span>;
}

/** Shown while editing a type that loses the `critical` tag. */
export function CriticalTagRemovalWarning() {
  const { t } = useTranslation("reference");
  return (
    <p role="alert" className="mt-1 text-xs text-status-warning">
      {t("absenceBehaviour.criticalRemovedWarning")}
    </p>
  );
}
