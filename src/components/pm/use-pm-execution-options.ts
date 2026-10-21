/**
 * Reasons (deferral / miss / cancel) and finding severities of the PM dialogs, from the governed
 * lists `PM.DEFERRAL_REASONS` (the plant's) and `PM.FINDING_SEVERITY` (Maintafox's).
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type PmExecutionOptions,
  type PmOption,
  listPmExecutionOptions,
} from "@/services/pm-service";

/** Label of a list value in the UI language (a shipped value's English label in English). */
export function pmOptionLabel(option: PmOption, language: string): string {
  return language.toLowerCase().startsWith("en") && option.label_en
    ? option.label_en
    : option.label;
}

export function usePmExecutionOptions(enabled = true) {
  const { i18n } = useTranslation();
  const [options, setOptions] = useState<PmExecutionOptions | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setError(null);
    listPmExecutionOptions()
      .then((o) => {
        if (!cancelled) setOptions(o);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, nonce]);

  const reload = useCallback(() => setNonce((n) => n + 1), []);
  const label = useCallback(
    (option: PmOption) => pmOptionLabel(option, i18n.resolvedLanguage || i18n.language),
    [i18n.language, i18n.resolvedLanguage],
  );

  return {
    reasons: options?.reasons ?? [],
    severities: options?.severities ?? [],
    loaded: options != null,
    error,
    reload,
    label,
  };
}
