/**
 * Pictures placed in the document header (pasted, dropped or the logo) are stored inline in the
 * header text as `data:` URLs, and the engine refuses a header over `DOCUMENT_HEADER_MAX_BYTES`.
 * A picture is therefore re-encoded smaller before it is inserted, and refused (null) when it
 * cannot fit.
 */
import { DOCUMENT_HEADER_MAX_BYTES } from "@/lib/document-header-sanitize";

/** One picture may use at most this share of the header limit, so text and a table still fit. */
export const HEADER_IMAGE_MAX_BYTES = Math.floor(DOCUMENT_HEADER_MAX_BYTES / 3);

/** Printed letterhead pictures never need more width than this (pixels). */
const MAX_WIDTH_PX = 600;
const SCALES = [1, 0.75, 0.5, 0.35, 0.25];
const QUALITIES = [0.85, 0.7, 0.5];

/** Size in bytes of the file a `data:` URL holds. */
export function dataUrlBytes(dataUrl: string): number {
  const comma = dataUrl.indexOf(",");
  if (comma < 0) return dataUrl.length;
  const base64 = dataUrl.slice(comma + 1);
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor((base64.length * 3) / 4) - padding;
}

/** The (width, height) the picture is drawn at for a scale step, never wider than the print width. */
export function scaledSize(width: number, height: number, scale: number): [number, number] {
  const ratio = Math.min(1, MAX_WIDTH_PX / width) * scale;
  return [Math.max(1, Math.round(width * ratio)), Math.max(1, Math.round(height * ratio))];
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new window.Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("image unreadable"));
    img.src = src;
  });
}

function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error("file unreadable"));
    reader.readAsDataURL(blob);
  });
}

/**
 * Re-encode a picture (file or `data:` URL) as WebP, smaller step by step (width, then quality),
 * until it fits `HEADER_IMAGE_MAX_BYTES`. Null when it cannot fit or cannot be read.
 */
export async function shrinkImageForHeader(source: Blob | string): Promise<string | null> {
  try {
    const original = typeof source === "string" ? source : await readAsDataUrl(source);
    const img = await loadImage(original);
    if (!img.naturalWidth || !img.naturalHeight) return null;
    for (const scale of SCALES) {
      const [w, h] = scaledSize(img.naturalWidth, img.naturalHeight, scale);
      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) return null;
      ctx.drawImage(img, 0, 0, w, h);
      for (const quality of QUALITIES) {
        const encoded = canvas.toDataURL("image/webp", quality);
        if (
          encoded.startsWith("data:image/webp") &&
          dataUrlBytes(encoded) <= HEADER_IMAGE_MAX_BYTES
        ) {
          return encoded;
        }
      }
    }
    return null;
  } catch {
    return null;
  }
}
