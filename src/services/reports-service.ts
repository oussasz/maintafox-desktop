import { z } from "zod";

import { saveBlobWithDialog } from "@/export/download";
import { invoke } from "@/lib/ipc-invoke";
import type {
  ExportedBinaryDocument,
  ExportReportInput,
  ReportRun,
  ReportSchedule,
  ReportTemplate,
  UpsertReportScheduleInput,
} from "@shared/ipc-types";

const ReportTemplateSchema = z.object({
  id: z.number(),
  code: z.string(),
  title: z.string(),
  description: z.string(),
  default_format: z.string(),
  spec_json: z.string(),
  is_active: z.boolean(),
});

const ReportScheduleSchema = z.object({
  id: z.number(),
  user_id: z.number(),
  template_id: z.number(),
  cron_expr: z.string(),
  export_format: z.string(),
  enabled: z.boolean(),
  next_run_at: z.string(),
  last_run_at: z.string().nullable(),
});

const ReportRunSchema = z.object({
  id: z.number(),
  schedule_id: z.number().nullable(),
  template_id: z.number(),
  user_id: z.number(),
  status: z.string(),
  export_format: z.string(),
  artifact_path: z.string().nullable(),
  byte_size: z.number().nullable(),
  error_message: z.string().nullable(),
  started_at: z.string(),
  finished_at: z.string().nullable(),
  has_downloadable_artifact: z.boolean(),
});

const ExportedBinaryDocumentSchema = z.object({
  file_name: z.string(),
  mime_type: z.string(),
  bytes: z.array(z.number().int().min(0).max(255)),
});

export async function listReportTemplates(): Promise<ReportTemplate[]> {
  const raw = await invoke<unknown>("list_report_templates");
  return z.array(ReportTemplateSchema).parse(raw);
}

export async function listMyReportSchedules(): Promise<ReportSchedule[]> {
  const raw = await invoke<unknown>("list_my_report_schedules");
  return z.array(ReportScheduleSchema).parse(raw);
}

export async function upsertMyReportSchedule(input: UpsertReportScheduleInput): Promise<number> {
  const raw = await invoke<unknown>("upsert_my_report_schedule", { input });
  return z.number().parse(raw);
}

export async function deleteMyReportSchedule(scheduleId: number): Promise<void> {
  await invoke("delete_my_report_schedule", { scheduleId });
}

export async function listMyReportRuns(limit?: number): Promise<ReportRun[]> {
  const raw = await invoke<unknown>("list_my_report_runs", { limit: limit ?? null });
  return z.array(ReportRunSchema).parse(raw);
}

/** Report language: titles, headers and labels of the generated PDF / XLSX. */
export type ReportLocale = "fr" | "en";

export function toReportLocale(appLocale: string): ReportLocale {
  return appLocale.toLowerCase().startsWith("en") ? "en" : "fr";
}

export async function exportReportNow(
  input: ExportReportInput & { locale: ReportLocale },
): Promise<ExportedBinaryDocument> {
  const raw = await invoke<unknown>("export_report_now", { input });
  return ExportedBinaryDocumentSchema.parse(raw);
}

export async function downloadReportRunArtifact(runId: number): Promise<ExportedBinaryDocument> {
  const raw = await invoke<unknown>("download_report_run_artifact", { runId });
  return ExportedBinaryDocumentSchema.parse(raw);
}

const SAVE_FILTERS: Record<string, { name: string; extensions: string[] }> = {
  pdf: { name: "PDF", extensions: ["pdf"] },
  xlsx: { name: "Excel", extensions: ["xlsx"] },
};

/**
 * Writes an exported report through the native Save dialog (an anchor download does nothing in
 * the packaged app). Resolves with the absolute path written; rejects with
 * `ExportCancelledError` when the user closes the dialog (check with `isExportCancelled`).
 */
export async function saveExportedDocument(
  doc: ExportedBinaryDocument,
  dialogTitle?: string,
): Promise<string> {
  const blob = new Blob([new Uint8Array(doc.bytes)], { type: doc.mime_type });
  const ext = doc.file_name.split(".").pop()?.toLowerCase() ?? "";
  const filter = SAVE_FILTERS[ext];
  return saveBlobWithDialog(blob, doc.file_name, {
    ...(dialogTitle ? { title: dialogTitle } : {}),
    ...(filter ? { filters: [filter] } : {}),
  });
}

/**
 * @deprecated Anchor download: does nothing in the packaged desktop app. Use
 * {@link saveExportedDocument} (native Save dialog). Kept for the reliability export panel until
 * it moves over.
 */
export function downloadExportedDocument(doc: ExportedBinaryDocument): void {
  const u8 = new Uint8Array(doc.bytes);
  const blob = new Blob([u8], { type: doc.mime_type });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = doc.file_name;
  a.click();
  URL.revokeObjectURL(url);
}
