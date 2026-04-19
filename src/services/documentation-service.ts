/**
 * Document library (PRD §6.15) — IPC via Tauri. Files stored like WO/DI attachments.
 */

import { invoke } from "@/lib/ipc-invoke";
import type {
  LibraryDocument,
  LibraryDocumentCategory,
  UpdateLibraryDocumentPayload,
} from "@shared/ipc-types";

export const MAX_LIBRARY_DOCUMENT_BYTES = 25 * 1024 * 1024;

export async function listLibraryDocuments(
  category: LibraryDocumentCategory | null,
  equipmentId: number | null,
): Promise<LibraryDocument[]> {
  return invoke<LibraryDocument[]>("list_library_documents", {
    category: category ?? null,
    equipmentId: equipmentId ?? null,
  });
}

export async function uploadLibraryDocument(input: {
  category: LibraryDocumentCategory;
  equipmentId: number | null;
  title: string;
  fileName: string;
  fileBytes: number[];
  mimeType: string;
  notes: string | null;
}): Promise<LibraryDocument> {
  return invoke<LibraryDocument>("upload_library_document", {
    category: input.category,
    equipmentId: input.equipmentId,
    title: input.title,
    fileName: input.fileName,
    fileBytes: input.fileBytes,
    mimeType: input.mimeType,
    notes: input.notes,
  });
}

export async function getLibraryDocumentFile(id: number): Promise<Uint8Array> {
  const bytes = await invoke<number[]>("get_library_document_file", { id });
  return new Uint8Array(bytes);
}

export async function deleteLibraryDocument(id: number): Promise<void> {
  await invoke<void>("delete_library_document", { id });
}

export async function updateLibraryDocument(
  payload: UpdateLibraryDocumentPayload,
): Promise<LibraryDocument> {
  return invoke<LibraryDocument>("update_library_document", { input: payload });
}

export async function fileToNumberArray(file: File): Promise<number[]> {
  const buffer = await file.arrayBuffer();
  return Array.from(new Uint8Array(buffer));
}
