/**
 * EntityPortrait — compact identity photo for Entity Detail Workspace headers
 * (same header slot / footprint as EntityQrCode).
 *
 * Presentation only: callers supply a `data:` URL (or loading/error/empty state).
 * Do not load filesystem paths via `convertFileSrc` here.
 */

import { Loader2, UserRound } from "lucide-react";

export interface EntityPortraitProps {
  /** `data:` URL when loaded successfully. */
  src: string | null;
  alt: string;
  loading?: boolean;
  error?: string | null;
  /** When no photo is configured (not an error). */
  empty?: boolean;
  /** Match EntityQrCode compact footprint. */
  sizePx?: number;
}

const DEFAULT_SIZE_PX = 90;

export function EntityPortrait({
  src,
  alt,
  loading = false,
  error = null,
  empty = false,
  sizePx = DEFAULT_SIZE_PX,
}: EntityPortraitProps) {
  return (
    <div
      className="flex shrink-0 items-center justify-center overflow-hidden rounded-md border border-surface-border bg-surface-2"
      style={{ width: sizePx, height: sizePx }}
      role="img"
      aria-label={alt}
    >
      {loading ? (
        <Loader2 className="h-6 w-6 animate-spin text-text-muted" aria-hidden />
      ) : error ? (
        <span className="px-1.5 text-center text-[10px] leading-tight text-status-danger">
          {error}
        </span>
      ) : src ? (
        <img src={src} alt={alt} className="h-full w-full object-cover" />
      ) : empty ? (
        <UserRound className="h-10 w-10 text-text-muted" aria-hidden />
      ) : (
        <UserRound className="h-10 w-10 text-text-muted" aria-hidden />
      )}
    </div>
  );
}
