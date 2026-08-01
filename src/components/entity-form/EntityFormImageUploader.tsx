import { ImagePlus, Loader2, Star, Trash2, Upload } from "lucide-react";
import { useCallback, useRef, type DragEvent, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormImageItem {
  /** Stable client key (staged) or server id string. */
  id: string;
  name: string;
  /** Absolute filesystem path when available (Tauri pick / drop). */
  path?: string | null;
  previewUrl?: string | null;
  isPrimary?: boolean;
}

export interface EntityFormImageUploaderProps {
  items: EntityFormImageItem[];
  onChange: (items: EntityFormImageItem[]) => void;
  /** Pick files via Tauri dialog; return staged items (caller owns path resolution). */
  onPickFiles: () => Promise<EntityFormImageItem[] | void>;
  disabled?: boolean;
  uploading?: boolean;
  emptyLabel?: ReactNode;
  hintLabel?: ReactNode;
  primaryLabel?: ReactNode;
  removeLabel?: ReactNode;
  addLabel?: ReactNode;
  className?: string;
  /**
   * Desktop constraint: WebView2 drag/drop often has no usable filesystem path.
   * When drop yields no path, we fall back to onPickFiles.
   */
  extractDroppedPaths?: (event: DragEvent) => string[];
  /** When set, caps staged images (e.g. 1 for personnel photo). */
  maxItems?: number;
}

function fileNameFromPath(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || path;
}

/**
 * Multi-image picker for entity create/edit dialogs.
 * Create mode stages local paths; Edit mode may mix staged + existing previews.
 */
export function EntityFormImageUploader({
  items,
  onChange,
  onPickFiles,
  disabled = false,
  uploading = false,
  emptyLabel = "No images yet",
  hintLabel = "Pick images. Drag-and-drop may fall back to the file dialog on desktop.",
  primaryLabel = "Primary",
  removeLabel = "Remove",
  addLabel = "Add images",
  className,
  extractDroppedPaths,
  maxItems,
}: EntityFormImageUploaderProps) {
  const busy = disabled || uploading;
  const inputGuard = useRef(false);
  const atCap = maxItems != null && items.length >= maxItems;

  const pick = useCallback(async () => {
    if (busy || inputGuard.current || atCap) return;
    inputGuard.current = true;
    try {
      const next = await onPickFiles();
      if (next && next.length > 0) {
        let merged = [...items, ...next];
        if (maxItems != null) {
          merged = merged.slice(0, maxItems);
        }
        if (!merged.some((i) => i.isPrimary) && merged[0]) {
          merged[0] = { ...merged[0], isPrimary: true };
        }
        onChange(merged);
      }
    } finally {
      inputGuard.current = false;
    }
  }, [atCap, busy, items, maxItems, onChange, onPickFiles]);

  const removeAt = (id: string) => {
    const next = items.filter((i) => i.id !== id);
    if (next.length > 0 && !next.some((i) => i.isPrimary) && next[0]) {
      next[0] = { ...next[0], isPrimary: true };
    }
    onChange(next);
  };

  const setPrimary = (id: string) => {
    onChange(items.map((i) => ({ ...i, isPrimary: i.id === id })));
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (busy || atCap) return;

    const paths =
      extractDroppedPaths?.(e) ??
      Array.from(e.dataTransfer.files ?? [])
        .map((f) => {
          const withPath = f as File & { path?: string };
          return typeof withPath.path === "string" ? withPath.path : "";
        })
        .filter(Boolean);

    if (paths.length === 0) {
      void pick();
      return;
    }

    const room = maxItems != null ? Math.max(0, maxItems - items.length) : paths.length;
    const limitedPaths = paths.slice(0, room);
    if (limitedPaths.length === 0) return;

    const staged: EntityFormImageItem[] = limitedPaths.map((path, idx) => ({
      id: `drop-${Date.now()}-${idx}`,
      name: fileNameFromPath(path),
      path,
      isPrimary: false,
    }));
    let merged = [...items, ...staged];
    if (maxItems != null) {
      merged = merged.slice(0, maxItems);
    }
    if (!merged.some((i) => i.isPrimary) && merged[0]) {
      merged[0] = { ...merged[0], isPrimary: true };
    }
    onChange(merged);
  };

  return (
    <div className={cn("space-y-3", className)}>
      {!atCap ? (
        <div
          role="button"
          tabIndex={busy ? -1 : 0}
          className={cn(mfEntityForm.mediaDropzone, busy && "pointer-events-none opacity-60")}
          onClick={() => void pick()}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              void pick();
            }
          }}
          onDragOver={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
          onDrop={onDrop}
          aria-disabled={busy}
        >
          {uploading ? (
            <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
          ) : (
            <ImagePlus className="h-6 w-6 text-text-muted" />
          )}
          <span className="font-medium text-text-primary">{addLabel}</span>
          <span className="max-w-sm text-xs">{hintLabel}</span>
        </div>
      ) : null}

      {items.length === 0 ? (
        <p className="text-xs text-text-muted">{emptyLabel}</p>
      ) : (
        <ul className={mfEntityForm.mediaGrid}>
          {items.map((item) => (
            <li key={item.id} className={mfEntityForm.mediaTile}>
              {item.previewUrl ? (
                <img src={item.previewUrl} alt={item.name} className="h-full w-full object-cover" />
              ) : (
                <div className="flex h-full w-full flex-col items-center justify-center gap-1 p-2 text-center text-[10px] text-text-muted">
                  <Upload className="h-4 w-4" />
                  <span className="line-clamp-3 break-all">{item.name}</span>
                </div>
              )}
              {item.isPrimary && (
                <span className="absolute left-1 top-1 rounded bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
                  {primaryLabel}
                </span>
              )}
              <div className="absolute inset-x-0 bottom-0 flex justify-end gap-1 bg-gradient-to-t from-black/60 to-transparent p-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                {!item.isPrimary && (
                  <Button
                    type="button"
                    size="icon"
                    variant="secondary"
                    className="h-7 w-7"
                    disabled={busy}
                    title={String(primaryLabel)}
                    onClick={(e) => {
                      e.stopPropagation();
                      setPrimary(item.id);
                    }}
                  >
                    <Star className="h-3.5 w-3.5" />
                    <span className="sr-only">{primaryLabel}</span>
                  </Button>
                )}
                <Button
                  type="button"
                  size="icon"
                  variant="destructive"
                  className="h-7 w-7"
                  disabled={busy}
                  title={String(removeLabel)}
                  onClick={(e) => {
                    e.stopPropagation();
                    removeAt(item.id);
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  <span className="sr-only">{removeLabel}</span>
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
