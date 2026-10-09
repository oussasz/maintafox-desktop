import {
  isDocumentHeaderHtmlEmpty,
  sanitizeDocumentHeaderHtml,
} from "@/lib/document-header-sanitize";
import {
  DOCUMENT_HEADER_SETTING_KEY,
  getSetting,
  parseDocumentHeaderValue,
} from "@/services/settings-service";

/**
 * Load sanitized default document letterhead HTML for print fiches.
 * Never throws — returns "" when unset, invalid, or on IPC failure
 * (no magic product-name fallback).
 */
export async function loadDocumentHeaderHtml(): Promise<string> {
  try {
    const row = await getSetting(DOCUMENT_HEADER_SETTING_KEY, "tenant");
    const parsed = parseDocumentHeaderValue(row?.setting_value_json ?? null);
    const sanitized = sanitizeDocumentHeaderHtml(parsed.html);
    if (isDocumentHeaderHtmlEmpty(sanitized)) return "";
    return sanitized;
  } catch (err) {
    console.error("[document-header] failed to load letterhead:", err);
    return "";
  }
}

/** Shared CSS for the letterhead block inside A4 print fiches. */
export const DOCUMENT_HEADER_PRINT_CSS = `
    .doc-letterhead { margin-bottom: 14px; text-align: left; }
    .doc-letterhead img { max-height: 64px; max-width: 220px; object-fit: contain; }
    .doc-letterhead p { margin: 0 0 4px; }
    .doc-letterhead h1, .doc-letterhead h2, .doc-letterhead h3 { margin: 0 0 4px; }
`;

/**
 * Wrap letterhead HTML for injection, or return empty string when blank.
 * Callers must pass already-sanitized HTML from `loadDocumentHeaderHtml`.
 */
export function wrapDocumentHeaderBlock(sanitizedHtml: string): string {
  if (!sanitizedHtml.trim()) return "";
  return `<div class="doc-letterhead">${sanitizedHtml}</div>`;
}
