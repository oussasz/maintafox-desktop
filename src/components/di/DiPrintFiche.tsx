/**
 * DiPrintFiche.tsx
 *
 * Print-ready A4 DI fiche. Strings and dates follow the current i18n language.
 */

import type { TFunction } from "i18next";

import { i18n } from "@/i18n";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Helpers ─────────────────────────────────────────────────────────────────

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

type DiStatusKey =
  | "new"
  | "inReview"
  | "approved"
  | "rejected"
  | "inProgress"
  | "resolved"
  | "closed"
  | "cancelled";

function statusToI18nKey(s: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    submitted: "new",
    pending_review: "inReview",
    returned_for_clarification: "inReview",
    rejected: "rejected",
    screened: "inReview",
    awaiting_approval: "inReview",
    approved_for_planning: "approved",
    deferred: "inReview",
    converted_to_work_order: "inProgress",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[s] ?? "new";
}

// ── Build HTML ──────────────────────────────────────────────────────────────

function buildHtml(di: InterventionRequest, t: TFunction<"di">, locale: string): string {
  const fmt = (iso: string | null | undefined) => {
    if (!iso) return "—";
    try {
      return new Date(iso).toLocaleString(locale, {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return String(iso);
    }
  };

  const now = new Date().toLocaleDateString(locale, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  });

  const sk = statusToI18nKey(di.status);
  const statusText = t(`status.${sk}` as "status.new");
  const urgencyR = t(`priority.${di.reported_urgency}` as "priority.low");
  const urgencyV = di.validated_urgency
    ? t(`priority.${di.validated_urgency}` as "priority.low")
    : null;
  const originText = t(`form.origin.${di.origin_type}` as "form.origin.operator", {
    defaultValue: di.origin_type,
  });
  const impactText = t(`form.impact.${di.impact_level}` as "form.impact.unknown", {
    defaultValue: di.impact_level,
  });

  const flags: string[] = [];
  if (di.safety_flag) flags.push(t("print.flagSafety"));
  if (di.environmental_flag) flags.push(t("print.flagEnv"));
  if (di.quality_flag) flags.push(t("print.flagQuality"));
  if (di.production_impact) flags.push(t("print.flagProd"));

  return `<!DOCTYPE html>
<html lang="${t("print.htmlLang")}">
<head>
  <meta charset="utf-8" />
  <title>${t("print.docTitle", { code: esc(di.code) })}</title>
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
    th { background: #f5f5f5; width: 35%; }
    .desc { white-space: pre-wrap; font-size: 10px; padding: 6px 8px; border: 1px solid #ddd; background: #fafafa; }
    .flags { display: flex; gap: 8px; margin-bottom: 8px; }
    .flag { background: #fee2e2; color: #991b1b; padding: 2px 8px; border-radius: 3px; font-size: 10px; font-weight: bold; }
    .signatures { display: flex; gap: 24px; margin-top: 24px; page-break-inside: avoid; }
    .sig-box { flex: 1; border: 1px solid #ccc; padding: 10px; text-align: center; min-height: 80px; }
    .sig-box p { margin: 2px 0; font-size: 10px; }
    .footer { margin-top: 24px; font-size: 9px; color: #999; text-align: center; border-top: 1px solid #eee; padding-top: 6px; }
    @media print { body { margin: 0; padding: 10px 20px; } }
  </style>
</head>
<body>
  <div class="header">
    <div class="company">Maintafox</div>
    <h1>${t("print.ficheTitle")}</h1>
    <div class="subtitle">${t("print.headerSubtitle")}</div>
  </div>

  <div class="ref-strip">
    ${t("print.refStrip", { code: esc(di.code), date: now })}
  </div>

  <div class="section">
    <h2>${t("print.identification")}</h2>
    <table>
      <tr><th>${t("print.codeDi")}</th><td>${esc(di.code)}</td></tr>
      <tr><th>${t("print.title")}</th><td>${esc(di.title)}</td></tr>
      <tr><th>${t("print.status")}</th><td>${esc(statusText)}</td></tr>
      <tr><th>${t("print.urgencyReported")}</th><td>${esc(urgencyR)}</td></tr>
      ${urgencyV != null ? `<tr><th>${t("print.urgencyValidated")}</th><td>${esc(urgencyV)}</td></tr>` : ""}
      <tr><th>${t("print.origin")}</th><td>${esc(originText)}</td></tr>
      <tr><th>${t("print.impactLevel")}</th><td>${esc(impactText)}</td></tr>
      <tr><th>${t("print.createdAt")}</th><td>${fmt(di.created_at)}</td></tr>
      <tr><th>${t("print.submittedAt")}</th><td>${fmt(di.submitted_at)}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>${t("print.equipment")}</h2>
    <table>
      <tr><th>${t("print.asset")}</th><td>#${di.asset_id}${
        di.sub_asset_ref ? ` — ${t("print.subAsset")} : ${esc(di.sub_asset_ref)}` : ""
      }</td></tr>
      <tr><th>${t("print.orgNode")}</th><td>#${di.org_node_id}</td></tr>
    </table>
  </div>

  ${
    flags.length > 0
      ? `<div class="section">
    <h2>${t("print.flags")}</h2>
    <div class="flags">
      ${flags.map((f) => `<span class="flag">${f}</span>`).join("")}
    </div>
  </div>`
      : ""
  }

  <div class="section">
    <h2>${t("print.description")}</h2>
    ${
      di.observed_at
        ? `<p style="font-size:10px;margin-bottom:4px;">${t("print.observedOn")} <strong>${fmt(
            di.observed_at,
          )}</strong></p>`
        : ""
    }
    <p style="font-size:10px;margin-bottom:4px;">${t("print.declarant")} <strong>#${di.submitter_id}</strong></p>
    <div class="desc">${esc(di.description)}</div>
  </div>

  ${
    di.reviewer_note
      ? `<div class="section">
    <h2>${t("print.reviewNotes")}</h2>
    <div class="desc">${esc(di.reviewer_note)}</div>
  </div>`
      : ""
  }

  ${
    di.converted_to_wo_id
      ? `<div class="section">
    <h2>${t("print.conversion")}</h2>
    <table>
      <tr><th>${t("print.convertedToWo")}</th><td>#${di.converted_to_wo_id}</td></tr>
      <tr><th>${t("print.convertedAt")}</th><td>${fmt(di.converted_at)}</td></tr>
    </table>
  </div>`
      : ""
  }

  <div class="signatures">
    <div class="sig-box">
      <p><strong>${t("print.sigReporter")}</strong></p>
      <br/><br/>
      <p>${t("print.signatureLine")}</p>
      <p>${t("print.dateLine")}</p>
    </div>
    <div class="sig-box">
      <p><strong>${t("print.sigChecker")}</strong></p>
      <br/><br/>
      <p>${t("print.signatureLine")}</p>
      <p>${t("print.dateLine")}</p>
    </div>
    <div class="sig-box">
      <p><strong>${t("print.sigMaint")}</strong></p>
      <br/><br/>
      <p>${t("print.signatureLine")}</p>
      <p>${t("print.dateLine")}</p>
    </div>
  </div>

  <div class="footer">
    ${t("print.footer", { code: esc(di.code), product: "Maintafox", date: now })}
  </div>
</body>
</html>`;
}

// ── Public API ──────────────────────────────────────────────────────────────

export function printDiFiche(di: InterventionRequest): void {
  let html = "";
  try {
    const t = i18n.getFixedT(i18n.language, "di");
    const locale = intlLocaleForLanguage(i18n.language);
    html = buildHtml(di, t, locale);
  } catch {
    // Last-resort fallback so the print action still works even if i18n/template generation fails.
    html = `<!doctype html><html><head><meta charset="utf-8"><title>${esc(di.code)}</title></head><body><h1>${esc(di.code)}</h1><p>${esc(di.title)}</p><pre>${esc(di.description)}</pre></body></html>`;
  }

  // Tauri/webview can block popup windows depending on platform settings.
  // Use an iframe fallback so print preview always opens from current window.
  const iframe = document.createElement("iframe");
  iframe.style.position = "fixed";
  iframe.style.right = "0";
  iframe.style.bottom = "0";
  iframe.style.width = "0";
  iframe.style.height = "0";
  iframe.style.border = "0";
  iframe.setAttribute("aria-hidden", "true");

  iframe.onload = () => {
    const w = iframe.contentWindow;
    if (!w) return;
    w.focus();
    w.print();
    setTimeout(() => {
      iframe.remove();
    }, 1000);
  };

  document.body.appendChild(iframe);
  const doc = iframe.contentDocument;
  if (!doc) return;
  doc.open();
  doc.write(html);
  doc.close();
}
