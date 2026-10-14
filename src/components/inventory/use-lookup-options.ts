/**
 * Active values of one reference-data domain (lookup) with labels in the app language.
 *
 * Inventory codes that the backend validates against a lookup domain (equivalence type,
 * document purpose, movement reason, variance reason, purchase priority, demand source) are
 * offered and displayed from that domain, so the screen shows the same FR/EN labels as the
 * reference-data manager and never a raw code.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { getLookupValues } from "@/services/lookup-service";
import type { LookupValueOption } from "@shared/ipc-types";

export interface LookupOptions {
  /** Active values, in backend order. */
  options: LookupValueOption[];
  /** Label of a code in the app language; the code itself only if the domain does not know it. */
  labelOf: (code: string | null | undefined) => string;
  loading: boolean;
  error: unknown;
  reload: () => void;
}

export function lookupLabel(option: LookupValueOption, language: string): string {
  if (language.startsWith("fr") && option.fr_label) return option.fr_label;
  if (language.startsWith("en") && option.en_label) return option.en_label;
  return option.label || option.code;
}

export function useLookupOptions(domainKey: string): LookupOptions {
  const { i18n } = useTranslation();
  const [all, setAll] = useState<LookupValueOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    getLookupValues(domainKey)
      .then((rows) => {
        if (!cancelled) setAll(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [domainKey, version]);

  const language = i18n.language ?? "fr";
  const options = useMemo(() => all.filter((o) => o.is_active !== 0), [all]);
  const labelOf = useCallback(
    (code: string | null | undefined) => {
      if (!code) return "—";
      const match = all.find((o) => o.code === code);
      return match ? lookupLabel(match, language) : code;
    },
    [all, language],
  );
  const reload = useCallback(() => setVersion((v) => v + 1), []);

  return { options, labelOf, loading, error, reload };
}
