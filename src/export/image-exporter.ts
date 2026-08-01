/**
 * ImageExporter — canvas-based PNG composition + native Save dialog.
 */

import { saveBlobWithDialog } from "@/export/download";
import type { ExportableImageSpec } from "@/export/types";

/**
 * Ask the user where to save, then render and write the PNG.
 * @returns Absolute path written.
 */
export async function exportImage(spec: ExportableImageSpec): Promise<string> {
  // Dialog first — only generate bytes after the user confirms a destination.
  // We still need the blob after path selection; order: dialog → render → write.
  // Implementation: saveBlobWithDialog opens dialog then writeFile; we render
  // before write by generating the blob then calling save (dialog → write).
  // Preferred UX: dialog first, then generate. Achieved by wrapping:

  const path = await saveBlobWithDeferredRender(spec);
  return path;
}

async function saveBlobWithDeferredRender(spec: ExportableImageSpec): Promise<string> {
  if (!isTauriRuntime()) {
    const blob = await spec.render();
    return saveBlobWithDialog(blob, spec.filename, {
      ...(spec.saveDialogTitle ? { title: spec.saveDialogTitle } : {}),
      filters: mimeToFilters(spec.mimeType),
    });
  }

  const { save } = await import("@tauri-apps/plugin-dialog");
  const { writeFile } = await import("@tauri-apps/plugin-fs");
  const { downloadDir, join } = await import("@tauri-apps/api/path");
  const { ExportCancelledError } = await import("@/export/types");

  let defaultPath = spec.filename;
  try {
    const downloads = await downloadDir();
    defaultPath = await join(downloads, spec.filename);
  } catch {
    // keep filename
  }

  const selected = await save({
    ...(spec.saveDialogTitle ? { title: spec.saveDialogTitle } : {}),
    defaultPath,
    filters: mimeToFilters(spec.mimeType),
  });

  if (!selected) {
    throw new ExportCancelledError();
  }

  const blob = await spec.render();
  const bytes = new Uint8Array(await blob.arrayBuffer());
  await writeFile(selected, bytes);
  return selected;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function mimeToFilters(
  mimeType: string | undefined,
): Array<{ name: string; extensions: string[] }> {
  if (mimeType === "application/pdf") {
    return [{ name: "PDF", extensions: ["pdf"] }];
  }
  return [{ name: "PNG", extensions: ["png"] }];
}

export function canvasToPngBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) {
        reject(new Error("ImageExporter: canvas.toBlob returned null."));
        return;
      }
      resolve(blob);
    }, "image/png");
  });
}

export function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`ImageExporter: failed to load image (${src}).`));
    img.src = src;
  });
}
