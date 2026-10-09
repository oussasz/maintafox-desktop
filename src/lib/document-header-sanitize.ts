import DOMPurify from "dompurify";

/** Allowed tags/attrs for tenant document letterhead HTML (Settings + print inject). */
const LETTERHEAD_CONFIG: Parameters<typeof DOMPurify.sanitize>[1] = {
  ALLOWED_TAGS: [
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
  ],
  ALLOWED_ATTR: ["class", "style", "src", "alt", "width", "height", "align", "colspan", "rowspan"],
  ALLOW_DATA_ATTR: false,
  ADD_DATA_URI_TAGS: ["img"],
};

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
