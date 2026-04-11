/**
 * WoPrintFiche.tsx
 *
 * Print-ready A4 WO fiche. Opens a new browser window with optimized
 * HTML layout: company header, identification, equipment, planning,
 * description, tasks, interveners, parts, closeout, signatures, footer.
 * Phase 2 – Sub-phase 05 – File 03 – Sprint S4.
 */

import {
  getWoAnalyticsSnapshot,
  type WoFailureDetail,
  type WoVerification,
} from "@/services/wo-closeout-service";
import { listLabor, listParts, listTasks } from "@/services/wo-execution-service";
import type { WoExecPart, WoExecTask, WoIntervener, WorkOrder } from "@shared/ipc-types";

// ── i18n keys for the print template ────────────────────────────────────────

type TFn = (key: string) => string;

const DEFAULT_T: TFn = (key) => key;

// ── Helpers ─────────────────────────────────────────────────────────────────

function fmtDate(iso: string | null | undefined, locale: string): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString(locale, {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ── Build HTML ──────────────────────────────────────────────────────────────

function buildHtml(
  wo: WorkOrder,
  tasks: WoExecTask[],
  labor: WoIntervener[],
  parts: WoExecPart[],
  failureDetails: WoFailureDetail[],
  verifications: WoVerification[],
  costTotals: {
    labor_cost: number;
    parts_cost: number;
    service_cost: number;
    total_cost: number;
  } | null,
  t: TFn,
  locale: string,
): string {
  const now = new Date().toLocaleDateString(locale);

  const taskRows = tasks
    .map(
      (tk) =>
        `<tr><td>${tk.sequence_order}</td><td>${esc(tk.task_description)}</td><td>${tk.is_completed ? "✓ OK" : "☐"}</td></tr>`,
    )
    .join("");

  const laborRows = labor
    .map(
      (l) =>
        `<tr><td>#${l.intervener_id}</td><td>${l.skill_id != null ? `#${l.skill_id}` : "—"}</td><td>${l.hours_worked != null ? `${l.hours_worked}h` : "—"}</td></tr>`,
    )
    .join("");

  const partRows = parts
    .map(
      (p) =>
        `<tr><td>${esc(p.article_ref ?? `#${p.article_id}`)}</td><td>${p.quantity_used ?? p.quantity_planned ?? "—"}</td><td>${p.unit_cost != null ? `${p.unit_cost.toFixed(2)}` : "—"}</td></tr>`,
    )
    .join("");

  const shiftDisplay = wo.shift ? esc(t(`shift.${wo.shift}`)) : "—";

  return `<!DOCTYPE html>
<html lang="${locale}">
<head>
  <meta charset="utf-8" />
  <title>${esc(t("print.pageTitle"))} — ${esc(wo.code)}</title>
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
    <h1>${esc(t("print.heading"))}</h1>
    <div class="subtitle">${esc(t("print.confidential"))}</div>
  </div>

  <div class="ref-strip">
    <span>${esc(t("print.reference"))} : <strong>${esc(wo.code)}</strong></span>
    <span>${esc(t("print.issueDate"))} : ${now}</span>
  </div>

  <div class="section">
    <h2>${esc(t("print.identification"))}</h2>
    <table>
      <tr><th>${esc(t("print.woCode"))}</th><td>${esc(wo.code)}</td></tr>
      <tr><th>${esc(t("print.woTitle"))}</th><td>${esc(wo.title)}</td></tr>
      <tr><th>${esc(t("print.woType"))}</th><td>${esc(wo.type_label ?? "—")}</td></tr>
      <tr><th>${esc(t("print.woUrgency"))}</th><td>${esc(wo.urgency_label ?? "—")}</td></tr>
      <tr><th>${esc(t("print.woStatus"))}</th><td>${esc(wo.status_code ?? "—")}</td></tr>
      <tr><th>${esc(t("print.createdAt"))}</th><td>${fmtDate(wo.created_at, locale)}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>${esc(t("print.equipment"))}</h2>
    <table>
      <tr><th>${esc(t("print.designation"))}</th><td>${esc(wo.asset_label ?? "—")}${wo.asset_code ? ` (${esc(wo.asset_code)})` : ""}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>${esc(t("print.planning"))}</h2>
    <table>
      <tr><th>${esc(t("print.plannedStart"))}</th><td>${fmtDate(wo.planned_start, locale)}</td></tr>
      <tr><th>${esc(t("print.plannedEnd"))}</th><td>${fmtDate(wo.planned_end, locale)}</td></tr>
      <tr><th>${esc(t("print.shift"))}</th><td>${shiftDisplay}</td></tr>
      <tr><th>${esc(t("print.estimatedDuration"))}</th><td>${wo.expected_duration_hours != null ? `${wo.expected_duration_hours}h` : "—"}</td></tr>
      <tr><th>${esc(t("print.assignedTo"))}</th><td>${esc(wo.responsible_username ?? "—")}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>${esc(t("print.description"))}</h2>
    ${wo.source_di_id ? `<p style="font-size:10px;margin-bottom:4px;">${esc(t("print.sourceDi"))} : <strong>DI-${wo.source_di_id}</strong></p>` : ""}
    <div class="desc">${esc(wo.description ?? "—")}</div>
  </div>

  ${
    tasks.length > 0
      ? `<div class="section">
    <h2>${esc(t("print.tasks"))}</h2>
    <table>
      <tr><th style="width:8%">N°</th><th>${esc(t("print.taskDescription"))}</th><th style="width:12%">${esc(t("print.taskStatus"))}</th></tr>
      ${taskRows}
    </table>
  </div>`
      : ""
  }

  ${
    labor.length > 0
      ? `<div class="section">
    <h2>${esc(t("print.interveners"))}</h2>
    <table>
      <tr><th>${esc(t("print.name"))}</th><th>${esc(t("print.skill"))}</th><th style="width:15%">${esc(t("print.hours"))}</th></tr>
      ${laborRows}
    </table>
  </div>`
      : ""
  }

  ${
    parts.length > 0
      ? `<div class="section">
    <h2>${esc(t("print.partsUsed"))}</h2>
    <table>
      <tr><th>${esc(t("print.designation"))}</th><th style="width:12%">${esc(t("print.quantity"))}</th><th style="width:15%">${esc(t("print.unitCost"))}</th></tr>
      ${partRows}
    </table>
  </div>`
      : ""
  }

  ${
    failureDetails.length > 0
      ? `<div class="section">
    <h2>${esc(t("print.failureAnalysis"))}</h2>
    <table>
      ${failureDetails
        .map(
          (fd) => `
        <tr><th style="width:35%">${esc(t("print.failureMode"))}</th><td>${fd.failure_mode_id != null ? `#${fd.failure_mode_id}` : "—"}</td></tr>
        <tr><th>${esc(t("print.failureCause"))}</th><td>${fd.failure_cause_id != null ? `#${fd.failure_cause_id}` : "—"}</td></tr>
        <tr><th>${esc(t("print.failureEffect"))}</th><td>${fd.failure_effect_id != null ? `#${fd.failure_effect_id}` : "—"}</td></tr>
        <tr><th>${esc(t("print.repairType"))}</th><td>${fd.is_temporary_repair ? esc(t("print.temporary")) : fd.is_permanent_repair ? esc(t("print.permanent")) : "—"}</td></tr>
        ${fd.notes ? `<tr><th>${esc(t("print.notes"))}</th><td>${esc(fd.notes)}</td></tr>` : ""}
      `,
        )
        .join("")}
    </table>
    ${wo.root_cause_summary ? `<p><strong>${esc(t("print.rootCause"))}:</strong> ${esc(wo.root_cause_summary)}</p>` : ""}
    ${wo.corrective_action_summary ? `<p><strong>${esc(t("print.correctiveAction"))}:</strong> ${esc(wo.corrective_action_summary)}</p>` : ""}
  </div>`
      : ""
  }

  ${
    verifications.length > 0
      ? `<div class="section">
    <h2>${esc(t("print.verification"))}</h2>
    <table>
      ${verifications
        .map(
          (v) => `
        <tr><th style="width:35%">${esc(t("print.verificationResult"))}</th><td>${esc(v.result)}</td></tr>
        <tr><th>${esc(t("print.returnToService"))}</th><td>${v.return_to_service_confirmed ? "✓" : "✗"}</td></tr>
        ${v.notes ? `<tr><th>${esc(t("print.notes"))}</th><td>${esc(v.notes)}</td></tr>` : ""}
      `,
        )
        .join("")}
    </table>
  </div>`
      : ""
  }

  ${
    costTotals && costTotals.total_cost > 0
      ? `<div class="section">
    <h2>${esc(t("print.costSummary"))}</h2>
    <table>
      <tr><th style="width:35%">${esc(t("print.laborCost"))}</th><td>${costTotals.labor_cost.toFixed(2)}</td></tr>
      <tr><th>${esc(t("print.partsCost"))}</th><td>${costTotals.parts_cost.toFixed(2)}</td></tr>
      <tr><th>${esc(t("print.serviceCost"))}</th><td>${costTotals.service_cost.toFixed(2)}</td></tr>
      <tr><th><strong>${esc(t("print.totalCost"))}</strong></th><td><strong>${costTotals.total_cost.toFixed(2)}</strong></td></tr>
    </table>
  </div>`
      : ""
  }

  <div class="signatures">
    <div class="sig-box">
      <p><strong>${esc(t("print.requester"))}</strong></p>
      <br/><br/>
      <p>${esc(t("print.signature"))} : _______________</p>
      <p>${esc(t("print.date"))} : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>${esc(t("print.technician"))}</strong></p>
      <br/><br/>
      <p>${esc(t("print.signature"))} : _______________</p>
      <p>${esc(t("print.date"))} : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>${esc(t("print.maintenanceManager"))}</strong></p>
      <br/><br/>
      <p>${esc(t("print.signature"))} : _______________</p>
      <p>${esc(t("print.date"))} : ___/___/______</p>
    </div>
  </div>

  <div class="footer">
    ${esc(t("print.reference"))} : ${esc(wo.code)} | Maintafox | ${now} — ${esc(t("print.confidential"))}
  </div>
</body>
</html>`;
}

// ── Public API ──────────────────────────────────────────────────────────────

export async function printWoFiche(
  wo: WorkOrder,
  t: TFn = DEFAULT_T,
  locale = "fr",
): Promise<void> {
  const [tasks, labor, parts, snapshot] = await Promise.all([
    listTasks(wo.id).catch(() => [] as WoExecTask[]),
    listLabor(wo.id).catch(() => [] as WoIntervener[]),
    listParts(wo.id).catch(() => [] as WoExecPart[]),
    getWoAnalyticsSnapshot(wo.id).catch(() => null),
  ]);

  const failureDetails = snapshot?.failure_details ?? [];
  const verifications = snapshot?.verifications ?? [];
  const costTotals = snapshot
    ? {
        labor_cost: snapshot.labor_cost,
        parts_cost: snapshot.parts_cost,
        service_cost: snapshot.service_cost,
        total_cost: snapshot.total_cost,
      }
    : null;

  const html = buildHtml(
    wo,
    tasks,
    labor,
    parts,
    failureDetails,
    verifications,
    costTotals,
    t,
    locale,
  );

  // Tauri/webview can block popup windows depending on platform settings.
  // Use an iframe fallback so print preview still opens from the current window.
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
