/**
 * asset-import-service.ts
 *
 * IPC wrappers for SP02-F04 asset import pipeline commands.
 * All invoke() calls for the import workflow are isolated here.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  ApplyPolicy,
  ApplyResult,
  ImportBatchSummary,
  ImportPreview,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const ValidationMessageSchema = z.object({
  category: z.string(),
  severity: z.string(),
  message: z.string(),
});

const ImportBatchSummarySchema = z.object({
  id: z.number(),
  source_filename: z.string(),
  source_sha256: z.string(),
  initiated_by_id: z.number().nullable(),
  status: z.string(),
  total_rows: z.number(),
  valid_rows: z.number(),
  warning_rows: z.number(),
  error_rows: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ImportPreviewRowSchema = z.object({
  id: z.number(),
  row_no: z.number(),
  normalized_asset_code: z.string().nullable(),
  normalized_external_key: z.string().nullable(),
  validation_status: z.string(),
  validation_messages: z.array(ValidationMessageSchema),
  proposed_action: z.string().nullable(),
  raw_json: z.string(),
});

const ImportPreviewSchema = z.object({
  batch: ImportBatchSummarySchema,
  rows: z.array(ImportPreviewRowSchema),
});

const ApplyResultSchema = z.object({
  batch: ImportBatchSummarySchema,
  created: z.number(),
  updated: z.number(),
  skipped: z.number(),
  errored: z.number(),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Upload a CSV file, create the import batch, parse and stage rows.
 * The IPC command combines create + stage in a single call.
 */
export async function createAssetImportBatch(
  filename: string,
  fileSha256: string,
  csvContent: number[],
): Promise<ImportBatchSummary> {
  const raw = await invoke<unknown>("create_asset_import_batch", {
    filename,
    fileSha256,
    csvContent,
  });
  return ImportBatchSummarySchema.parse(raw) as ImportBatchSummary;
}

/** Run governance validation on all staging rows of a batch. */
export async function validateAssetImportBatch(batchId: number): Promise<ImportBatchSummary> {
  const raw = await invoke<unknown>("validate_asset_import_batch", {
    batchId,
  });
  return ImportBatchSummarySchema.parse(raw) as ImportBatchSummary;
}

/** Get the full validation preview (batch + staging rows). */
export async function getAssetImportPreview(batchId: number): Promise<ImportPreview> {
  const raw = await invoke<unknown>("get_asset_import_preview", { batchId });
  return ImportPreviewSchema.parse(raw) as ImportPreview;
}

/** Apply validated staging rows to the equipment registry. */
export async function applyAssetImportBatch(
  batchId: number,
  policy: ApplyPolicy,
): Promise<ApplyResult> {
  const raw = await invoke<unknown>("apply_asset_import_batch", {
    batchId,
    policy,
  });
  return ApplyResultSchema.parse(raw) as ApplyResult;
}

/** List import batches, optionally filtered by status. */
export async function listAssetImportBatches(
  statusFilter?: string,
  limit?: number,
): Promise<ImportBatchSummary[]> {
  const raw = await invoke<unknown>("list_asset_import_batches", {
    statusFilter: statusFilter ?? null,
    limit: limit ?? null,
  });
  return z.array(ImportBatchSummarySchema).parse(raw) as ImportBatchSummary[];
}
