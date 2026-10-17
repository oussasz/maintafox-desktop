import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";

import { PM_WO_FINDING_TYPES } from "./pm-labels";

/** Finding types are the request symptom catalogue (the backend maps them to DI symptoms). */
export const PM_FINDING_TYPE_DOMAIN = "WORK.SYMPTOMS";

const WO_TYPES = new Set<string>(PM_WO_FINDING_TYPES);

/**
 * Label of a stored finding type: translated for the codes the OT close-out writes, the symptom
 * catalogue label for catalogue codes (stored upper-case), else the text the user typed.
 */
export function usePmFindingTypeLabel(): (code: string) => string {
  const { t } = useTranslation("pm");
  const [labels, setLabels] = useState<Map<string, string>>(() => new Map());

  useEffect(() => {
    let cancelled = false;
    listPublishedReferenceValuesByDomainCode(PM_FINDING_TYPE_DOMAIN)
      .then((values) => {
        if (cancelled) return;
        setLabels(new Map(values.map((v) => [v.code.trim().toUpperCase(), v.label])));
      })
      .catch(() => {
        // The global IPC toast reports the failure; stored types then show as recorded.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return useCallback(
    (code: string) => {
      const key = code.trim().toUpperCase();
      if (WO_TYPES.has(key)) return t(`findingType.${key}`);
      return labels.get(key) ?? code;
    },
    [labels, t],
  );
}
