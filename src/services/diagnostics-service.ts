// ADR-003 compliant: all IPC calls for diagnostics go through this file only.
// Components and hooks MUST NOT import from @tauri-apps/api/core directly
// for diagnostics operations.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { DiagnosticsAppInfo, IntegrityReport, SupportBundle } from "@shared/ipc-types";

// â”€â”€ Zod schemas for runtime shape validation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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

// â”€â”€ SP06-F03 Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DiagnosticsAppInfoSchema = z.object({
  app_version: z.string(),
  os_name: z.string(),
  os_version: z.string(),
  arch: z.string(),
  db_schema_version: z.number(),
  active_locale: z.string(),
  sync_status: z.string(),
  uptime_seconds: z.number(),
});

const SupportBundleSchema = z.object({
  generated_at: z.string(),
  app_info: DiagnosticsAppInfoSchema,
  log_lines: z.array(z.string()),
  collection_warnings: z.array(z.string()),
  runbook_links: z.array(z.string()).optional(),
});

// â”€â”€ Service functions â€” Integrity â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function runIntegrityCheck(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("run_integrity_check");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}

export async function repairSeedData(): Promise<IntegrityReport> {
  const raw = await invoke<unknown>("repair_seed_data");
  return IntegrityReportSchema.parse(raw) as IntegrityReport;
}

// â”€â”€ Service functions â€” SP06-F03 Diagnostics & Support Bundle â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Return rich application metadata (session-gated).
 * Richer than the pre-auth `get_app_info` â€” includes DB schema version,
 * locale from settings, and process uptime.
 */
export async function getDiagnosticsInfo(): Promise<DiagnosticsAppInfo> {
  const raw = await invoke<unknown>("get_diagnostics_info");
  return DiagnosticsAppInfoSchema.parse(raw) as DiagnosticsAppInfo;
}

/**
 * Generate and return a sanitized support bundle (session-gated).
 * Contains the last 500 log lines (sanitized), app info, and any
 * non-fatal collection warnings.
 */
export async function generateSupportBundle(): Promise<SupportBundle> {
  const raw = await invoke<unknown>("generate_support_bundle");
  return SupportBundleSchema.parse(raw) as SupportBundle;
}
