/**
 * Shared helpers for entity-form image staging (Tauri desktop).
 */

import type { EntityFormImageItem } from "@/components/entity-form";

function fileNameFromPath(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

/** Open the native multi-select image dialog and return staged items with filesystem paths. */
export async function pickEntityFormImages(): Promise<EntityFormImageItem[]> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: true,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
  });
  if (!selected) return [];

  const paths = Array.isArray(selected)
    ? selected
        .map((s) => (typeof s === "string" ? s : (s as { path?: string }).path))
        .filter(Boolean)
    : [typeof selected === "string" ? selected : (selected as { path?: string }).path].filter(
        Boolean,
      );

  const now = Date.now();
  return (paths as string[]).map((path, idx) => ({
    id: `pick-${now}-${idx}`,
    name: fileNameFromPath(path),
    path,
    isPrimary: false,
  }));
}

export interface EntityFormFileItem {
  id: string;
  name: string;
  path: string;
}

/** Open the native multi-select document dialog for non-image attachments. */
export async function pickEntityFormFiles(): Promise<EntityFormFileItem[]> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "Documents",
        extensions: ["pdf", "doc", "docx", "xls", "xlsx", "txt", "csv", "zip"],
      },
      { name: "All", extensions: ["*"] },
    ],
  });
  if (!selected) return [];

  const paths = Array.isArray(selected)
    ? selected
        .map((s) => (typeof s === "string" ? s : (s as { path?: string }).path))
        .filter(Boolean)
    : [typeof selected === "string" ? selected : (selected as { path?: string }).path].filter(
        Boolean,
      );

  const now = Date.now();
  return (paths as string[]).map((path, idx) => ({
    id: `file-${now}-${idx}`,
    name: fileNameFromPath(path),
    path,
  }));
}

/** Primary first, then remaining in list order. */
export function orderImagesForUpload(items: EntityFormImageItem[]): EntityFormImageItem[] {
  const primary = items.find((i) => i.isPrimary);
  const rest = items.filter((i) => i !== primary);
  return primary ? [primary, ...rest] : items;
}
