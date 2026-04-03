import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type { IntegrityReport } from "@shared/ipc-types";

// ── Zod schemas for runtime shape validation ──────────────────────────────

const IntegrityIssueSchema = z.object({
  code: z.string(),
  description: z.string(),
  is_auto_repairable: z.boolean(),
  subject: z.string(),
});

export const IntegrityReportSchema = z.object({
  is_healthy: z.boolean(),
  is_recoverable: z.boolean(),
  issues: z.array(IntegrityIssueSchema),
  seed_schema_version: z.number().int().nullable(),
  domain_count: z.number().int(),
  value_count: z.number().int(),
});

// ── Service functions ─────────────────────────────────────────────────────

export async function runIntegrityCheck(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("run_integrity_check");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}

export async function repairSeedData(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("repair_seed_data");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}
