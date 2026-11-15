/**
 * bulk-import-service.ts
 *
 * IPC wrappers for the "Imports de données" wizard (organisation tree and the other plant data
 * that has no dedicated importer). Invoke names match the Rust `#[tauri::command]` ids.
 */

import { invoke } from "@/lib/ipc-invoke";

export interface BulkImportColumn {
  key: string;
  header: string;
  required: boolean;
  hint: string;
}

export interface BulkImportEntity {
  code: string;
  label: string;
  order: number;
  needs: string;
  columns: BulkImportColumn[];
}

export interface BulkImportBatch {
  id: number;
  entity: string;
  source_filename: string;
  status: string;
  total_rows: number;
  valid_rows: number;
  warning_rows: number;
  error_rows: number;
  ignored_columns: string[];
  created_at: string;
}

export interface BulkImportMessage {
  severity: "error" | "warning";
  code: string;
  message: string;
}

export interface BulkImportPreviewRow {
  row_no: number;
  status: "ok" | "warning" | "error";
  action: "create" | "update" | "skip";
  raw: Record<string, string>;
  messages: BulkImportMessage[];
  apply_status: string | null;
  apply_message: string | null;
}

export interface BulkImportPreview {
  batch: BulkImportBatch;
  rows: BulkImportPreviewRow[];
}

export interface BulkImportApplyResult {
  batch: BulkImportBatch;
  created: number;
  updated: number;
  skipped: number;
  failed: number;
}

export interface BulkImportTable {
  header: string[];
  rows: string[][];
}

export function listBulkImportEntities(): Promise<BulkImportEntity[]> {
  return invoke<BulkImportEntity[]>("list_bulk_import_entities");
}

export function createBulkImportBatch(input: {
  entity: string;
  filename: string;
  source_sha256: string;
  source_kind: "csv" | "xlsx";
  file_content: number[];
}): Promise<BulkImportBatch> {
  return invoke<BulkImportBatch>("create_bulk_import_batch", { input });
}

export function getBulkImportPreview(batchId: number): Promise<BulkImportPreview> {
  return invoke<BulkImportPreview>("get_bulk_import_preview", { batchId });
}

export function applyBulkImportBatch(batchId: number): Promise<BulkImportApplyResult> {
  return invoke<BulkImportApplyResult>("apply_bulk_import_batch", { batchId });
}

export function getBulkImportTemplate(entity: string): Promise<BulkImportTable> {
  return invoke<BulkImportTable>("get_bulk_import_template", { entity });
}

export function exportBulkImportData(entity: string): Promise<BulkImportTable> {
  return invoke<BulkImportTable>("export_bulk_import_data", { entity });
}
