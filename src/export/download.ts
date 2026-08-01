/**
 * Shared download / save transport for the Maintafox export system.
 *
 * Desktop (Tauri): native Save dialog → write file → return absolute path.
 * Browser fallback: Blob object-URL download (dev / non-Tauri only).
 */

import { ExportCancelledError } from "@/export/types";

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Opens the OS Save File dialog, then writes the blob to the chosen path.
 * @returns Absolute path written, or throws ExportCancelledError if dismissed.
 */
export async function saveBlobWithDialog(
  blob: Blob,
  defaultFileName: string,
  options?: { title?: string; filters?: Array<{ name: string; extensions: string[] }> },
): Promise<string> {
  if (!isTauriRuntime()) {
    // Dev / browser fallback — still not silent: caller should toast.
    downloadBlob(blob, defaultFileName);
    return defaultFileName;
  }

  const { save } = await import("@tauri-apps/plugin-dialog");
  const { writeFile } = await import("@tauri-apps/plugin-fs");
  const { downloadDir, join } = await import("@tauri-apps/api/path");

  let defaultPath = defaultFileName;
  try {
    const downloads = await downloadDir();
    defaultPath = await join(downloads, defaultFileName);
  } catch {
    // Keep bare filename if Downloads dir is unavailable.
  }

  const selected = await save({
    ...(options?.title ? { title: options.title } : {}),
    defaultPath,
    filters: options?.filters ?? [{ name: "PNG", extensions: ["png"] }],
  });

  if (!selected) {
    throw new ExportCancelledError();
  }

  const bytes = new Uint8Array(await blob.arrayBuffer());
  await writeFile(selected, bytes);
  return selected;
}

/** Open the containing folder in the OS file manager. */
export async function revealInFolder(filePath: string): Promise<void> {
  if (!isTauriRuntime()) return;

  const { dirname } = await import("@tauri-apps/api/path");
  const { open } = await import("@tauri-apps/plugin-shell");
  const folder = await dirname(filePath);
  await open(folder);
}

/**
 * Legacy browser-style download. Prefer {@link saveBlobWithDialog} on desktop.
 */
export function downloadBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = fileName;
    a.rel = "noopener";
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    URL.revokeObjectURL(url);
  }
}

export async function downloadDataUrl(dataUrl: string, fileName: string): Promise<void> {
  const res = await fetch(dataUrl);
  const blob = await res.blob();
  downloadBlob(blob, fileName);
}
