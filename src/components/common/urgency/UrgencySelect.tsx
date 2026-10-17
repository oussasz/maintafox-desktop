/**
 * UrgencySelect — picks a level of the shared DI / OT urgency scale (by code), most urgent
 * first, each option showing the plant's label and target times.
 */

import { useTranslation } from "react-i18next";

import { urgencyLevelLabel, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { UrgencyCode } from "@shared/ipc-types";

export interface UrgencySelectProps {
  value: string | null;
  onChange: (code: UrgencyCode) => void;
  id?: string;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  "aria-invalid"?: boolean;
  "aria-describedby"?: string;
}

export function UrgencySelect({
  value,
  onChange,
  id,
  placeholder,
  disabled = false,
  className,
  "aria-invalid": ariaInvalid,
  "aria-describedby": ariaDescribedBy,
}: UrgencySelectProps) {
  const { t } = useTranslation("common");
  const { levels, loading, error, language } = useUrgencyScale();

  return (
    <>
      <Select
        {...(value ? { value } : {})}
        onValueChange={(v) => onChange(v as UrgencyCode)}
        disabled={disabled || (loading && levels.length === 0)}
      >
        <SelectTrigger
          {...(id ? { id } : {})}
          className={className ?? "h-10"}
          aria-invalid={ariaInvalid}
          {...(ariaDescribedBy ? { "aria-describedby": ariaDescribedBy } : {})}
        >
          <SelectValue
            placeholder={loading ? t("urgency.loading") : (placeholder ?? t("urgency.placeholder"))}
          />
        </SelectTrigger>
        <SelectContent>
          {levels.map((level) => (
            <SelectItem key={level.code} value={level.code}>
              <span className="flex flex-col">
                <span>{urgencyLevelLabel(level, language)}</span>
                {level.response_target_hours != null && level.completion_target_hours != null ? (
                  <span className="text-[11px] text-text-muted">
                    {t("urgency.targetsShort", {
                      response: level.response_target_hours,
                      completion: level.completion_target_hours,
                    })}
                  </span>
                ) : null}
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {error ? (
        <p className="mt-1 text-xs text-status-danger" role="alert">
          {t("urgency.loadError")}
        </p>
      ) : null}
    </>
  );
}
