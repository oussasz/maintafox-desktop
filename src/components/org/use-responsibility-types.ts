/**
 * Governed responsibility types (lookup domain `org.responsibility_type`: maintenance owner,
 * production owner, HSE owner, planner, approver, plus tenant additions).
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { responsibilityTypeLabel } from "@/components/org/org-display";
import { getLookupValues } from "@/services/lookup-service";
import type { LookupValueOption } from "@shared/ipc-types";

export const ORG_RESPONSIBILITY_TYPE_DOMAIN = "org.responsibility_type";

export function useResponsibilityTypes() {
  const { i18n } = useTranslation("org");
  const [types, setTypes] = useState<LookupValueOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const values = await getLookupValues(ORG_RESPONSIBILITY_TYPE_DOMAIN);
      setTypes(values.filter((v) => v.is_active !== 0));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const labelOf = useCallback(
    (code: string) => responsibilityTypeLabel(code, types, i18n.language ?? "fr"),
    [types, i18n.language],
  );

  return { types, loading, error, reload: load, labelOf };
}
