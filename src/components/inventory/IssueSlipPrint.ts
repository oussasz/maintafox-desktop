/**
 * Printable stock issue slip ("bon de sortie", UX L3-16): what left the store, for which work
 * order, at what cost, with signature boxes for the storekeeper and the receiver.
 */

import { printHtmlDocument } from "@/components/inventory/PoPrintFiche";
import {
  DOCUMENT_HEADER_PRINT_CSS,
  loadDocumentHeaderHtml,
  wrapDocumentHeaderBlock,
} from "@/export/templates/document-header";
import type { IssueOutcome } from "@/services/inventory-service";

type TFn = (key: string, options?: Record<string, unknown>) => string;

export interface IssueSlipData {
  outcome: IssueOutcome;
  articleCode: string;
  articleName: string | null;
  unit: string;
  locationLabel: string | null;
}

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function buildIssueSlipHtml(
  data: IssueSlipData,
  t: TFn,
  locale: string,
  currency: string,
  letterheadHtml: string,
  now: Date = new Date(),
): string {
  const qty = (n: number) => new Intl.NumberFormat(locale, { maximumFractionDigits: 3 }).format(n);
  const money = (n: number | null) =>
    n == null ? "—" : new Intl.NumberFormat(locale, { style: "currency", currency }).format(n);
  const when = new Intl.DateTimeFormat(locale, { dateStyle: "short", timeStyle: "short" }).format(
    now,
  );
  const article = data.articleName ? `${data.articleCode} — ${data.articleName}` : data.articleCode;
  const rows = data.outcome.lines
    .map(
      (line) => `<tr>
        <td>${esc(article)}</td>
        <td>${esc(line.serial_number ?? line.lot_number ?? "—")}</td>
        <td class="num">${esc(qty(line.quantity))} ${esc(data.unit)}</td>
        <td class="num">${esc(money(line.unit_cost))}</td>
      </tr>`,
    )
    .join("");
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8" />
  <title>${esc(t("issueDesk.slip.title"))}</title>
  <style>
    @page { size: A5; margin: 12mm; }
    body { font-family: Arial, sans-serif; margin: 0; padding: 12px 20px; font-size: 11px; color: #333; }
    h1 { font-size: 15px; text-align: center; margin: 0 0 10px; }
    .ref { display: flex; justify-content: space-between; gap: 12px; padding: 6px 10px; background: #f0f0f0; border: 1px solid #ccc; margin-bottom: 10px; }
    table { width: 100%; border-collapse: collapse; margin-bottom: 6px; }
    td, th { border: 1px solid #ccc; padding: 4px 6px; text-align: left; font-size: 10px; }
    th { background: #f5f5f5; }
    .num { text-align: right; }
    .total { font-weight: bold; text-align: right; margin: 4px 0 0; }
    .signatures { display: flex; gap: 16px; margin-top: 20px; page-break-inside: avoid; }
    .sig-box { flex: 1; border: 1px solid #ccc; padding: 8px; text-align: center; min-height: 64px; }
    ${DOCUMENT_HEADER_PRINT_CSS}
  </style>
</head>
<body>
  ${wrapDocumentHeaderBlock(letterheadHtml)}
  <h1>${esc(t("issueDesk.slip.title"))}</h1>
  <div class="ref">
    <span>${esc(t("issueDesk.slip.workOrder"))} : <strong>${esc(data.outcome.work_order_code ?? "—")}</strong></span>
    <span>${esc(t("issueDesk.slip.location"))} : ${esc(data.locationLabel ?? "—")}</span>
    <span>${esc(t("issueDesk.slip.date"))} : ${esc(when)}</span>
  </div>
  <table>
    <thead><tr>
      <th>${esc(t("issueDesk.slip.article"))}</th>
      <th>${esc(t("issueDesk.slip.lot"))}</th>
      <th class="num">${esc(t("issueDesk.slip.quantity"))}</th>
      <th class="num">${esc(t("issueDesk.slip.unitCost"))}</th>
    </tr></thead>
    <tbody>${rows}</tbody>
  </table>
  <p class="total">${esc(t("issueDesk.slip.total"))} : ${esc(money(data.outcome.total_cost))}</p>
  <div class="signatures">
    <div class="sig-box">${esc(t("issueDesk.slip.issuedBy"))}</div>
    <div class="sig-box">${esc(t("issueDesk.slip.receivedBy"))}</div>
  </div>
</body>
</html>`;
}

export async function printIssueSlip(
  data: IssueSlipData,
  t: TFn,
  locale: string,
  currency: string,
): Promise<void> {
  const letterheadHtml = await loadDocumentHeaderHtml();
  printHtmlDocument(buildIssueSlipHtml(data, t, locale, currency, letterheadHtml));
}
