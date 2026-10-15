// ADR-003: all invoke() calls live exclusively in src/services/.
//
// Shared DI / OT urgency scale (`WORK.URGENCY`): five levels fixed by Maintafox, with the
// plant's labels and target times (`reference::urgency_scale`).

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { UpdateUrgencyLevelInput, UrgencyLevel } from "@shared/ipc-types";

export const UrgencyCodeSchema = z.enum(["emergency", "urgent", "high", "normal", "low"]);

export const UrgencyLevelSchema = z.object({
  id: z.number().int(),
  code: UrgencyCodeSchema,
  rank: z.number().int(),
  label_fr: z.string(),
  label_en: z.string(),
  description: z.string().nullable(),
  color_hex: z.string().nullable(),
  response_target_hours: z.number().int().nullable(),
  completion_target_hours: z.number().int().nullable(),
  escalation_threshold_hours: z.number().int().nullable(),
  targets_source: z.string(),
});

export async function listUrgencyLevels(): Promise<UrgencyLevel[]> {
  const raw = await invoke<unknown>("list_urgency_levels");
  return z.array(UrgencyLevelSchema).parse(raw) as UrgencyLevel[];
}

export async function updateUrgencyLevel(
  code: string,
  input: UpdateUrgencyLevelInput,
): Promise<UrgencyLevel> {
  const raw = await invoke<unknown>("update_urgency_level", { code, input });
  return UrgencyLevelSchema.parse(raw) as UrgencyLevel;
}
