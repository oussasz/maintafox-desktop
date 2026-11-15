import DOMPurify from "dompurify";

/**
 * Allowed tags/attrs for tenant document letterhead HTML (Settings + print inject). The engine
 * checks the same lists (`HEADER_TAGS` / `HEADER_ATTRIBUTES` in `src-tauri/src/settings/catalog.rs`);
 * a test compares both so they cannot drift apart.
 */
export const DOCUMENT_HEADER_ALLOWED_TAGS = [
  "p",
  "br",
  "strong",
  "b",
  "em",
  "i",
  "u",
  "s",
  "h1",
  "h2",
  "h3",
  "ul",
  "ol",
  "li",
  "span",
  "div",
  "img",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
];
export const DOCUMENT_HEADER_ALLOWED_ATTRIBUTES = [
  "class",
  "style",
  "src",
  "alt",
  "width",
  "height",
  "align",
  "colspan",
  "rowspan",
];

/** Largest header the engine stores, in bytes (`HEADER_MAX_BYTES` in `catalog.rs`). */
export const DOCUMENT_HEADER_MAX_BYTES = 200_000;

const LETTERHEAD_CONFIG: Parameters<typeof DOMPurify.sanitize>[1] = {
  ALLOWED_TAGS: DOCUMENT_HEADER_ALLOWED_TAGS,
  ALLOWED_ATTR: DOCUMENT_HEADER_ALLOWED_ATTRIBUTES,
  ALLOW_DATA_ATTR: false,
  ADD_DATA_URI_TAGS: ["img"],
};

/** A header the engine would refuse: the reason code of `errors:gateError.*` and its parameters. */
export interface DocumentHeaderIssue {
  code: "GATE_SET_HEADER_TOO_LARGE";
  params: { size_kb: string; max_kb: string };
}

/**
 * The refusal the engine would give this (sanitized) header, found before saving; null when it
 * fits. The sanitizer already removes forbidden tags, attributes and comments, so only the size
 * can still be over the limit (a picture pasted into the text is stored inline).
 */
export function documentHeaderIssue(sanitizedHtml: string): DocumentHeaderIssue | null {
  const bytes = new TextEncoder().encode(sanitizedHtml).length;
  if (bytes <= DOCUMENT_HEADER_MAX_BYTES) return null;
  return {
    code: "GATE_SET_HEADER_TOO_LARGE",
    params: {
      size_kb: String(Math.ceil(bytes / 1000)),
      max_kb: String(DOCUMENT_HEADER_MAX_BYTES / 1000),
    },
  };
}

/** Sanitize letterhead HTML before persist and before print injection. */
export function sanitizeDocumentHeaderHtml(html: string): string {
  if (!html || !html.trim()) return "";
  return DOMPurify.sanitize(html, LETTERHEAD_CONFIG).trim();
}

/** True when sanitized HTML has no meaningful visible content. */
export function isDocumentHeaderHtmlEmpty(html: string): boolean {
  const sanitized = sanitizeDocumentHeaderHtml(html);
  const text = sanitized
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/gi, " ")
    .trim();
  return text.length === 0 && !/<img\b/i.test(sanitized);
}
