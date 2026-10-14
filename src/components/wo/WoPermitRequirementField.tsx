/**
 * WoPermitRequirementField.tsx
 *
 * "Permit / LOTO required" control for a work order. When checked, the start gate
 * (`permit/wo_gate.rs`) refuses to put the WO in progress until a linked permit is
 * active. Used on the create form (value sent with the create input) and in the
 * planning panel (saved immediately while the WO has not started).
 */

import { HardHat } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

interface WoPermitRequirementFieldProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  /** Visible explanation when the control is disabled (e.g. execution started). */
  disabledReason?: string | null;
  /** Visible origin hint (e.g. "set because the request was flagged as a safety risk"). */
  originHint?: string | null;
  className?: string;
}

export function WoPermitRequirementField({
  checked,
  onCheckedChange,
  disabled = false,
  disabledReason = null,
  originHint = null,
  className,
}: WoPermitRequirementFieldProps) {
  const { t } = useTranslation("ot");
  const id = useId();
  const helpId = `${id}-help`;

  return (
    <div
      className={cn(
        "rounded-md border px-3 py-2",
        checked ? mfTone.warning.panel : "border-surface-border",
        className,
      )}
      data-testid="wo-permit-requirement"
    >
      <label
        htmlFor={id}
        className={cn(
          "flex min-h-[44px] items-center gap-3",
          disabled ? "cursor-not-allowed" : "cursor-pointer",
        )}
      >
        <input
          id={id}
          type="checkbox"
          className="h-5 w-5 shrink-0 rounded border border-input accent-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
          checked={checked}
          disabled={disabled}
          aria-describedby={helpId}
          onChange={(e) => onCheckedChange(e.target.checked)}
        />
        <HardHat className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
        <span className="text-sm font-medium text-text-primary">
          {t("permitRequirement.label")}
        </span>
      </label>
      <div id={helpId} className="space-y-1 pl-8 text-xs text-text-muted">
        <p>{t("permitRequirement.help")}</p>
        {originHint ? <p className="font-medium text-text-primary">{originHint}</p> : null}
        {disabled && disabledReason ? <p>{disabledReason}</p> : null}
      </div>
    </div>
  );
}
