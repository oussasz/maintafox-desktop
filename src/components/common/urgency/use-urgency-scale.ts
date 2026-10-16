/**
 * Shared DI / OT urgency scale for display: localized label, tone and target times of a level.
 * The levels, labels and targets come from the backend (`list_urgency_levels`); the frontend
 * only maps a level code to a tone of the design system.
 */

import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";

import { type Tone, URGENCY_TONE, toneOf } from "@/design-system/tone";
import { useUrgencyScaleStore } from "@/stores/urgency-scale-store";
import type { UrgencyCode, UrgencyLevel } from "@shared/ipc-types";

/** Label of a level in the UI language. */
export function urgencyLevelLabel(level: UrgencyLevel, language: string): string {
  return language.toLowerCase().startsWith("fr") ? level.label_fr : level.label_en;
}

export function urgencyTone(code: string | null | undefined): Tone {
  return toneOf(URGENCY_TONE, code ?? null);
}

export function useUrgencyScale() {
  const { i18n } = useTranslation();
  const levels = useUrgencyScaleStore((s) => s.levels);
  const loading = useUrgencyScaleStore((s) => s.loading);
  const error = useUrgencyScaleStore((s) => s.error);
  const ensureLoaded = useUrgencyScaleStore((s) => s.ensureLoaded);

  useEffect(() => {
    void ensureLoaded();
  }, [ensureLoaded]);

  const levelOf = useCallback(
    (code: string | null | undefined): UrgencyLevel | undefined =>
      code ? levels.find((l) => l.code === code) : undefined,
    [levels],
  );

  /** Localized label; the raw code while the scale loads or for an unknown code. */
  const labelOf = useCallback(
    (code: string | null | undefined): string => {
      if (!code) return "";
      const level = levels.find((l) => l.code === code);
      return level ? urgencyLevelLabel(level, i18n.language) : code;
    },
    [levels, i18n.language],
  );

  /** 1 = most urgent … 5 = least urgent; unknown codes sort last. */
  const rankOf = useCallback(
    (code: string | null | undefined): number => levelOf(code)?.rank ?? Number.MAX_SAFE_INTEGER,
    [levelOf],
  );

  return { levels, loading, error, levelOf, labelOf, rankOf, language: i18n.language };
}

/** Left border of calendar chips per level (tone colours of the design system). */
const URGENCY_BORDER: Record<UrgencyCode, string> = {
  emergency: "border-l-tone-danger-fg",
  urgent: "border-l-tone-attention-fg",
  high: "border-l-tone-warning-fg",
  normal: "border-l-tone-info-fg",
  low: "border-l-tone-neutral-fg",
};

export function urgencyBorderClass(code: string | null | undefined): string {
  return (code ? URGENCY_BORDER[code as UrgencyCode] : undefined) ?? "border-l-tone-neutral-border";
}

/**
 * Label of a level outside React (print sheets): loads the scale if needed, then reads it.
 * Returns the raw code only when the level is unknown to the backend.
 */
export async function urgencyLabelFor(
  code: string | null | undefined,
  language: string,
): Promise<string> {
  if (!code) return "";
  await useUrgencyScaleStore.getState().ensureLoaded();
  const level = useUrgencyScaleStore.getState().levels.find((l) => l.code === code);
  return level ? urgencyLevelLabel(level, language) : code;
}
