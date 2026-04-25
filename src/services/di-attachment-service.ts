/**
 * di-attachment-service.ts
 *
 * IPC wrappers for DI attachment commands.
 * Phase 2 â€“ Sub-phase 04 â€“ File 03 â€“ Sprint S3.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type { DiAttachment, DiAttachmentUploadInput } from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const DiAttachmentSchema = z.object({
  id: z.number(),
  di_id: z.number(),
  file_name: z.string(),
  relative_path: z.string(),
  mime_type: z.string(),
  size_bytes: z.number(),
  attachment_type: z.string(),
  uploaded_by_id: z.number().nullable(),
  uploaded_at: z.string(),
  notes: z.string().nullable(),
});

// â”€â”€ Commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Upload an attachment to a DI.
 *
 * The caller is responsible for converting a `File` to `number[]` before
 * calling this function. Use `fileToNumberArray()` below for convenience.
 */
export async function uploadDiAttachment(input: DiAttachmentUploadInput): Promise<DiAttachment> {
  const raw = await invoke<unknown>("upload_di_attachment", {
    diId: input.diId,
    fileName: input.fileName,
    fileBytes: input.fileBytes,
    mimeType: input.mimeType,
    attachmentType: input.attachmentType,
    notes: input.notes ?? null,
  });
  return DiAttachmentSchema.parse(raw) as DiAttachment;
}

export async function listDiAttachments(diId: number): Promise<DiAttachment[]> {
  const raw = await invoke<unknown>("list_di_attachments", { diId });
  return z.array(DiAttachmentSchema).parse(raw) as DiAttachment[];
}

export async function deleteDiAttachment(attachmentId: number): Promise<void> {
  await invoke<unknown>("delete_di_attachment", { attachmentId });
}

// â”€â”€ Helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/** Maximum file size accepted by the backend (20 MB). */
export const MAX_ATTACHMENT_SIZE_BYTES = 20 * 1024 * 1024;

/**
 * Convert a `File` to `number[]` for Tauri IPC transport.
 *
 * Usage:
 * ```ts
 * const bytes = await fileToNumberArray(file);
 * await uploadDiAttachment({ diId, fileName: file.name, fileBytes: bytes, ... });
 * ```
 */
export async function fileToNumberArray(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}
