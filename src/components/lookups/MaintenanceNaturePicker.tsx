/**
 * Nature picker of a company request type (DI.REQUEST_TYPE): the EN 13306 nature of the work
 * order a request of this type becomes. The value is stored in `semantic_tag` and checked on the
 * server (`reference::values::checked_semantic_tag`); the options are the published values of the
 * SYSTEM list WORK.MAINTENANCE_NATURES, so the frontend keeps no list of its own.
 *
 * Empty stays allowed: the DI → OT conversion then creates corrective work, immediate at the top
 * urgency level and deferred otherwise. The empty option says so.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { ReferenceValue } from "@shared/ipc-types";

export const MAINTENANCE_NATURES_DOMAIN = "WORK.MAINTENANCE_NATURES";

/** Label of a nature value in the UI language (`metadata_json.label_en` in English). */
export function natureValueLabel(value: ReferenceValue, language: string): string {
  if (language.toLowerCase().startsWith("fr") || !value.metadata_json) return value.label;
  try {
    const meta = JSON.parse(value.metadata_json) as { label_en?: unknown };
    return typeof meta.label_en === "string" && meta.label_en.trim() !== ""
      ? meta.label_en
      : value.label;
  } catch {
    return value.label;
  }
}

/** Published maintenance natures; loaded only when `enabled`. */
export function useMaintenanceNatures(enabled: boolean): ReferenceValue[] {
  const [natures, setNatures] = useState<ReferenceValue[]>([]);
  useEffect(() => {
    if (!enabled) {
      setNatures([]);
      return;
    }
    let cancelled = false;
    void listPublishedReferenceValuesByDomainCode(MAINTENANCE_NATURES_DOMAIN)
      .then((rows) => {
        if (!cancelled) setNatures(rows);
      })
      .catch(() => {
        // The global toast reports the failure; the column then shows the stored codes.
        if (!cancelled) setNatures([]);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);
  return natures;
}

interface MaintenanceNatureSelectProps {
  value: string | null;
  natures: ReferenceValue[];
  onChange: (code: string | null) => void;
  disabled?: boolean;
}

export function MaintenanceNatureSelect({
  value,
  natures,
  onChange,
  disabled,
}: MaintenanceNatureSelectProps) {
  const { t, i18n } = useTranslation("reference");
  const known = value == null || natures.some((n) => n.code === value);
  return (
    <select
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
      aria-label={t("maintenanceNature.column")}
      title={t("maintenanceNature.fallbackHint")}
      disabled={disabled}
      className={cn(mfInput.nativeSelect, "h-9")}
    >
      <option value="">{t("maintenanceNature.fallback")}</option>
      {natures.map((n) => (
        <option key={n.id} value={n.code}>
          {natureValueLabel(n, i18n.language)}
        </option>
      ))}
      {known ? null : <option value={value ?? ""}>{value}</option>}
    </select>
  );
}

/** Read-only cell: the nature label, or the corrective fallback in muted text. */
export function MaintenanceNatureText({
  value,
  natures,
}: {
  value: string | null;
  natures: ReferenceValue[];
}) {
  const { t, i18n } = useTranslation("reference");
  if (!value) {
    return (
      <span className="text-xs text-text-muted" title={t("maintenanceNature.fallbackHint")}>
        {t("maintenanceNature.fallback")}
      </span>
    );
  }
  const nature = natures.find((n) => n.code === value);
  return (
    <span className="text-sm">{nature ? natureValueLabel(nature, i18n.language) : value}</span>
  );
}
