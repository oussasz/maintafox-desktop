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
import { formatDateTime as formatLocalDateTime } from "@/utils/formatters";

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
  const text = formatLocalDateTime(iso, locale);
  return text === "—" ? none : text;
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
    // The zero-energy test recorded at verification (L4-13); "not recorded" before that version.
    [
      t("card.zeroEnergy"),
      card.zero_energy_note ??
        (card.verifier_signature ? t("isolation.zeroEnergyNotRecorded") : none),
    ],
    [t("card.expiry"), fmtDateTime(card.expires_at, locale, none)],
  ];
  // A lock taken off by a forced removal (applier absent): who authorised it, when and why.
  if (card.forced_removal) {
    rows.push(
      [t("card.forcedRemoval"), t("card.forcedRemovalYes")],
      [t("card.forcedAuthorisedBy"), card.forced_authorized_by ?? none],
      [t("card.forcedAt"), fmtDateTime(card.forced_removed_at, locale, none)],
      [t("card.forcedReason"), card.forced_reason ?? none],
    );
  }
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

/**
 * L4-17: one printable LOTO sheet for the whole permit — every verified isolation point of the
 * permit in one table (point, energy, lock, verifier, zero-energy test), instead of one tag per
 * point. `cards` are the points' card views, in the permit's order.
 */
export function buildPermitLotoSheetHtml(
  cards: LotoCardViewRecord[],
  t: TFn,
  locale: string,
  letterheadHtml: string,
): string {
  const none = t("detail.none");
  const first = cards[0];
  const head: [string, string][] = first
    ? [
        [t("card.permit"), first.permit_code],
        [t("card.equipment"), first.equipment_label],
        [t("card.expiry"), fmtDateTime(first.expires_at, locale, none)],
      ]
    : [];
  const headRows = head
    .map(([label, value]) => `<tr><th>${esc(label)}</th><td>${esc(value)}</td></tr>`)
    .join("");
  const pointRows = cards
    .map((card, i) => {
      const zero =
        card.zero_energy_note ??
        (card.verifier_signature ? t("isolation.zeroEnergyNotRecorded") : none);
      const forced = card.forced_removal
        ? `<div class="forced">${esc(t("card.forcedRemoval"))} — ${esc(card.forced_authorized_by ?? none)}, ${esc(fmtDateTime(card.forced_removed_at, locale, none))} : ${esc(card.forced_reason ?? none)}</div>`
        : "";
      return `<tr><td>${i + 1}</td><td>${esc(card.isolation_point)}${forced}</td><td>${esc(card.energy_type)}</td><td>${esc(card.lock_number ?? none)}</td><td>${esc(card.verifier_signature ?? none)}</td><td>${esc(zero)}</td><td class="sig"></td></tr>`;
    })
    .join("");
  const code = first?.permit_code ?? "";
  return `<!DOCTYPE html>
<html lang="${esc(locale)}">
<head>
  <meta charset="utf-8" />
  <title>${esc(t("card.title"))} — ${esc(code)}</title>
  <style>
    ${DOCUMENT_HEADER_PRINT_CSS}
    * { box-sizing: border-box; }
    body { font-family: Arial, Helvetica, sans-serif; color: #0B1F3A; margin: 0; padding: 16px; }
    .sheet { border: 3px solid #B42318; border-radius: 8px; padding: 16px; }
    .sheet h1 { margin: 0 0 4px; font-size: 22px; color: #B42318; text-transform: uppercase; letter-spacing: 0.04em; }
    .warning { font-size: 12px; font-weight: bold; margin-bottom: 12px; }
    table { width: 100%; border-collapse: collapse; font-size: 12px; margin-bottom: 12px; }
    th, td { text-align: left; padding: 6px 8px; border: 1px solid #D0D5DD; vertical-align: top; }
    th { color: #475467; font-weight: 600; background: #F9FAFB; }
    table.head th { width: 30%; }
    td.sig { width: 22%; }
    .forced { margin-top: 4px; color: #B54708; font-weight: 600; }
    @media print { body { padding: 0; } }
  </style>
</head>
<body>
  ${letterheadHtml ? wrapDocumentHeaderBlock(letterheadHtml) : ""}
  <div class="sheet">
    <h1>${esc(t("card.title"))}</h1>
    <div class="warning">${esc(t("card.doNotOperate"))}</div>
    <table class="head">${headRows}</table>
    <table>
      <thead><tr><th>#</th><th>${esc(t("card.point"))}</th><th>${esc(t("card.energy"))}</th><th>${esc(t("card.lock"))}</th><th>${esc(t("card.verifier"))}</th><th>${esc(t("card.zeroEnergy"))}</th><th>${esc(t("card.signature"))}</th></tr></thead>
      <tbody>${pointRows}</tbody>
    </table>
  </div>
</body>
</html>`;
}

/** Opens the print dialog for the permit's LOTO sheet (L4-17). */
export async function printPermitLotoSheet(
  cards: LotoCardViewRecord[],
  t: TFn,
  locale: string,
): Promise<void> {
  const letterhead = await loadDocumentHeaderHtml();
  printHtmlDocument(buildPermitLotoSheetHtml(cards, t, locale, letterhead));
}
