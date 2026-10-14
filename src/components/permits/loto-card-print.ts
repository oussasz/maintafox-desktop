/**
 * Printable LOTO (lockout / tagout) tag for one verified isolation point.
 *
 * Uses the company letterhead and the same iframe print path as the WO fiche
 * (`printHtmlDocument`). The caller records the print in the permit audit
 * (`record_loto_card_print`) once the print dialog has been opened.
 */

import { printHtmlDocument } from "@/components/wo/WoPrintFiche";
import {
  DOCUMENT_HEADER_PRINT_CSS,
  loadDocumentHeaderHtml,
  wrapDocumentHeaderBlock,
} from "@/export/templates/document-header";
import type { LotoCardViewRecord } from "@/services/permit-service";

type TFn = (key: string) => string;

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function fmtDateTime(iso: string | null | undefined, locale: string, none: string): string {
  if (!iso) return none;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return none;
  return d.toLocaleString(locale, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function buildLotoCardHtml(
  card: LotoCardViewRecord,
  t: TFn,
  locale: string,
  letterheadHtml: string,
): string {
  const none = t("detail.none");
  const rows: [string, string][] = [
    [t("card.permit"), card.permit_code],
    [t("card.equipment"), card.equipment_label],
    [t("card.energy"), card.energy_type],
    [t("card.point"), card.isolation_point],
    [t("card.lock"), card.lock_number ?? none],
    [t("card.verifier"), card.verifier_signature ?? none],
    [t("card.expiry"), fmtDateTime(card.expires_at, locale, none)],
  ];
  const body = rows
    .map(([label, value]) => `<tr><th>${esc(label)}</th><td>${esc(value)}</td></tr>`)
    .join("");

  return `<!DOCTYPE html>
<html lang="${esc(locale)}">
<head>
  <meta charset="utf-8" />
  <title>${esc(t("card.title"))} — ${esc(card.permit_code)}</title>
  <style>
    ${DOCUMENT_HEADER_PRINT_CSS}
    * { box-sizing: border-box; }
    body { font-family: Arial, Helvetica, sans-serif; color: #0B1F3A; margin: 0; padding: 16px; }
    .tag { border: 3px solid #B42318; border-radius: 8px; padding: 16px; max-width: 120mm; margin: 0 auto; }
    .tag h1 { margin: 0 0 4px; font-size: 22px; color: #B42318; text-transform: uppercase; letter-spacing: 0.04em; }
    .tag .warning { font-size: 12px; font-weight: bold; margin-bottom: 12px; }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    th { text-align: left; width: 40%; padding: 6px 8px; border-bottom: 1px solid #D0D5DD; color: #475467; font-weight: 600; }
    td { padding: 6px 8px; border-bottom: 1px solid #D0D5DD; font-weight: 600; }
    .sign { margin-top: 18px; font-size: 12px; }
    .sign div { border-top: 1px solid #0B1F3A; margin-top: 28px; padding-top: 4px; }
    @media print { body { padding: 0; } }
  </style>
</head>
<body>
  ${letterheadHtml ? wrapDocumentHeaderBlock(letterheadHtml) : ""}
  <div class="tag">
    <h1>${esc(t("card.title"))}</h1>
    <div class="warning">${esc(t("card.doNotOperate"))}</div>
    <table>${body}</table>
    <div class="sign"><div>${esc(t("card.signature"))}</div></div>
  </div>
</body>
</html>`;
}

/** Opens the print dialog for a LOTO tag (no letterhead when none is configured). */
export async function printLotoCard(
  card: LotoCardViewRecord,
  t: TFn,
  locale: string,
): Promise<void> {
  const letterhead = await loadDocumentHeaderHtml();
  printHtmlDocument(buildLotoCardHtml(card, t, locale, letterhead));
}
