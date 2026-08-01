/**
 * Print-ready A4 purchase order fiche.
 * Follows WoPrintFiche pattern (iframe + print).
 */

import { formatAssetLabel, formatEntityCode, formatOrDash } from "@/lib/display";
import type { PurchaseOrderDetail } from "@shared/ipc-types";

type TFn = (key: string, options?: Record<string, unknown>) => string;

const DEFAULT_T: TFn = (key) => key;

function fmtDate(iso: string | null | undefined, locale: string): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString(locale, {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}

function fmtMoney(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  return n.toFixed(2);
}

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function buildHtml(detail: PurchaseOrderDetail, t: TFn, locale: string): string {
  const { order, lines, grand_total, grand_total_partial } = detail;
  const now = new Date().toLocaleDateString(locale);
  const supplier =
    order.supplier_name?.trim() ||
    order.supplier_company_name?.trim() ||
    t("procurement.poWorkspace.noSupplier");

  const lineRows = lines
    .map((line) => {
      const article = formatAssetLabel(line.article_code, line.article_name);
      return `<tr>
        <td>${esc(article)}</td>
        <td style="text-align:right">${line.ordered_qty}</td>
        <td style="text-align:right">${fmtMoney(line.unit_price)}</td>
        <td style="text-align:right">${fmtMoney(line.line_total)}</td>
      </tr>`;
    })
    .join("");

  const grandLabel = grand_total_partial
    ? `${fmtMoney(grand_total)} ${esc(t("procurement.poWorkspace.grandTotalPartial"))}`
    : fmtMoney(grand_total);

  return `<!DOCTYPE html>
<html lang="${locale}">
<head>
  <meta charset="utf-8" />
  <title>${esc(t("procurement.poWorkspace.print.pageTitle"))} — ${esc(order.po_number)}</title>
  <style>
    @page { size: A4; margin: 15mm; }
    body { font-family: Arial, sans-serif; margin: 0; padding: 20px 30px; font-size: 11px; color: #333; }
    h1 { font-size: 16px; text-align: center; margin: 0 0 2px; }
    .header { text-align: center; margin-bottom: 16px; }
    .header .company { font-size: 14px; font-weight: bold; }
    .header .subtitle { color: #666; font-size: 10px; }
    .ref-strip { display: flex; justify-content: space-between; padding: 6px 10px; background: #f0f0f0; border: 1px solid #ccc; margin-bottom: 12px; font-size: 11px; }
    .section { margin-bottom: 12px; }
    .section h2 { font-size: 12px; background: #e8e8e8; padding: 4px 8px; margin: 0 0 6px; border-left: 3px solid #555; }
    table { width: 100%; border-collapse: collapse; margin-bottom: 6px; }
    td, th { border: 1px solid #ccc; padding: 4px 6px; text-align: left; font-size: 10px; }
    th { background: #f5f5f5; }
    .signatures { display: flex; gap: 24px; margin-top: 24px; page-break-inside: avoid; }
    .sig-box { flex: 1; border: 1px solid #ccc; padding: 10px; text-align: center; min-height: 80px; }
    .sig-box p { margin: 2px 0; font-size: 10px; }
    .footer { margin-top: 24px; font-size: 9px; color: #999; text-align: center; border-top: 1px solid #eee; padding-top: 6px; }
    .total-row { font-weight: bold; }
    @media print { body { margin: 0; padding: 10px 20px; } }
  </style>
</head>
<body>
  <div class="header">
    <div class="company">Maintafox</div>
    <h1>${esc(t("procurement.poWorkspace.print.heading"))}</h1>
    <div class="subtitle">${esc(t("procurement.poWorkspace.print.confidential"))}</div>
  </div>

  <div class="ref-strip">
    <span>${esc(t("procurement.poWorkspace.print.reference"))} : <strong>${esc(formatEntityCode(order.po_number))}</strong></span>
    <span>${esc(t("procurement.poWorkspace.print.issueDate"))} : ${now}</span>
  </div>

  <div class="section">
    <h2>${esc(t("procurement.poWorkspace.print.header"))}</h2>
    <table>
      <tr><th style="width:35%">${esc(t("procurement.poWorkspace.fields.poNumber"))}</th><td>${esc(order.po_number)}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.status"))}</th><td>${esc(order.status)}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.supplier"))}</th><td>${esc(String(supplier))}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.orderedAt"))}</th><td>${fmtDate(order.ordered_at, locale)}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.approvedAt"))}</th><td>${fmtDate(order.approved_at, locale)}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.expectedDelivery"))}</th><td>${fmtDate(order.expected_delivery_date, locale)}</td></tr>
      <tr><th>${esc(t("procurement.poWorkspace.fields.createdAt"))}</th><td>${fmtDate(order.created_at, locale)}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>${esc(t("procurement.poWorkspace.print.lines"))}</h2>
    <table>
      <tr>
        <th>${esc(t("procurement.poWorkspace.lines.article"))}</th>
        <th style="width:12%;text-align:right">${esc(t("procurement.poWorkspace.lines.ordered"))}</th>
        <th style="width:15%;text-align:right">${esc(t("procurement.poWorkspace.lines.unitPrice"))}</th>
        <th style="width:15%;text-align:right">${esc(t("procurement.poWorkspace.lines.lineTotal"))}</th>
      </tr>
      ${
        lineRows || `<tr><td colspan="4">${esc(t("procurement.poWorkspace.lines.empty"))}</td></tr>`
      }
      <tr class="total-row">
        <td colspan="3" style="text-align:right">${esc(t("procurement.poWorkspace.grandTotal"))}</td>
        <td style="text-align:right">${grandLabel}</td>
      </tr>
    </table>
  </div>

  <div class="signatures">
    <div class="sig-box">
      <p><strong>${esc(t("procurement.poWorkspace.print.buyer"))}</strong></p>
      <br/><br/>
      <p>${esc(t("procurement.poWorkspace.print.signature"))} : _______________</p>
      <p>${esc(t("procurement.poWorkspace.print.date"))} : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>${esc(t("procurement.poWorkspace.print.approver"))}</strong></p>
      <br/><br/>
      <p>${esc(t("procurement.poWorkspace.print.signature"))} : _______________</p>
      <p>${esc(t("procurement.poWorkspace.print.date"))} : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>${esc(t("procurement.poWorkspace.print.supplierSig"))}</strong></p>
      <br/><br/>
      <p>${esc(t("procurement.poWorkspace.print.signature"))} : _______________</p>
      <p>${esc(t("procurement.poWorkspace.print.date"))} : ___/___/______</p>
    </div>
  </div>

  <div class="footer">
    ${esc(t("procurement.poWorkspace.print.reference"))} : ${esc(formatOrDash(order.po_number))} | Maintafox | ${now} — ${esc(t("procurement.poWorkspace.print.confidential"))}
  </div>
</body>
</html>`;
}

export function printPoFiche(detail: PurchaseOrderDetail, t: TFn = DEFAULT_T, locale = "fr"): void {
  const html = buildHtml(detail, t, locale);

  const iframe = document.createElement("iframe");
  iframe.style.position = "fixed";
  iframe.style.right = "0";
  iframe.style.bottom = "0";
  iframe.style.width = "0";
  iframe.style.height = "0";
  iframe.style.border = "0";
  iframe.setAttribute("aria-hidden", "true");

  document.body.appendChild(iframe);
  const doc = iframe.contentDocument;
  if (!doc) return;
  doc.open();
  doc.write(html);
  doc.close();

  const w = iframe.contentWindow;
  if (!w) return;

  const cleanup = () => {
    setTimeout(() => {
      iframe.remove();
    }, 300);
  };

  w.requestAnimationFrame(() => {
    w.requestAnimationFrame(() => {
      w.focus();
      w.print();
    });
  });

  w.onafterprint = cleanup;
  setTimeout(cleanup, 15000);
}
