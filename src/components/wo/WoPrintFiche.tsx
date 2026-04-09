/**
 * WoPrintFiche.tsx
 *
 * Print-ready A4 WO fiche. Opens a new browser window with optimized
 * HTML layout: company header, identification, equipment, planning,
 * description, tasks, interveners, parts, signatures, footer.
 * Phase 2 – Sub-phase 05 – File 03 – Sprint S4.
 */

import { listLabor, listParts, listTasks } from "@/services/wo-service";
import type { WoLaborEntry, WoPartUsage, WoTask, WorkOrder } from "@shared/ipc-types";

// ── Helpers ─────────────────────────────────────────────────────────────────

function fmtDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleDateString("fr-FR", {
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

function shiftLabel(s: string | null | undefined): string {
  const map: Record<string, string> = {
    morning: "Matin",
    afternoon: "Après-midi",
    night: "Nuit",
    full_day: "Journée complète",
  };
  return s ? (map[s] ?? s) : "—";
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
  tasks: WoTask[],
  labor: WoLaborEntry[],
  parts: WoPartUsage[],
): string {
  const now = new Date().toLocaleDateString("fr-FR");

  const taskRows = tasks
    .map(
      (t) =>
        `<tr><td>${t.sequence}</td><td>${esc(t.description)}</td><td>${t.is_completed ? "✓ OK" : "☐"}</td></tr>`,
    )
    .join("");

  const laborRows = labor
    .map(
      (l) =>
        `<tr><td>${esc(l.intervener_name ?? `#${l.intervener_id}`)}</td><td>${esc(l.skill ?? "—")}</td><td>${l.hours_worked != null ? `${l.hours_worked}h` : "—"}</td></tr>`,
    )
    .join("");

  const partRows = parts
    .map(
      (p) =>
        `<tr><td>${esc(p.part_label ?? `#${p.part_id}`)}</td><td>${p.quantity_actual ?? p.quantity_planned ?? "—"}</td><td>${p.unit_cost != null ? `${p.unit_cost.toFixed(2)}` : "—"}</td></tr>`,
    )
    .join("");

  return `<!DOCTYPE html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>Fiche OT — ${esc(wo.code)}</title>
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
    <h1>Fiche d'Ordre de Travail</h1>
    <div class="subtitle">Document confidentiel</div>
  </div>

  <div class="ref-strip">
    <span>Référence : <strong>${esc(wo.code)}</strong></span>
    <span>Date d'émission : ${now}</span>
  </div>

  <div class="section">
    <h2>IDENTIFICATION</h2>
    <table>
      <tr><th>Code OT</th><td>${esc(wo.code)}</td></tr>
      <tr><th>Titre</th><td>${esc(wo.title)}</td></tr>
      <tr><th>Type</th><td>${esc(wo.type_label ?? "—")}</td></tr>
      <tr><th>Urgence</th><td>${esc(wo.urgency_label ?? "—")}</td></tr>
      <tr><th>Statut</th><td>${esc(wo.status)}</td></tr>
      <tr><th>Créé le</th><td>${fmtDate(wo.created_at)}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>ÉQUIPEMENT CONCERNÉ</h2>
    <table>
      <tr><th>Désignation</th><td>${esc(wo.equipment_name ?? "—")}${wo.equipment_code ? ` (${esc(wo.equipment_code)})` : ""}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>PLANIFICATION</h2>
    <table>
      <tr><th>Début prévu</th><td>${fmtDate(wo.planned_start)}</td></tr>
      <tr><th>Fin prévue</th><td>${fmtDate(wo.planned_end)}</td></tr>
      <tr><th>Poste</th><td>${shiftLabel(wo.shift)}</td></tr>
      <tr><th>Durée estimée</th><td>${wo.expected_duration_hours != null ? `${wo.expected_duration_hours}h` : "—"}</td></tr>
      <tr><th>Assigné à</th><td>${esc(wo.assigned_to_name ?? "—")}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>DESCRIPTION</h2>
    ${wo.source_di_code ? `<p style="font-size:10px;margin-bottom:4px;">Source DI : <strong>${esc(wo.source_di_code)}</strong></p>` : ""}
    <div class="desc">${esc(wo.description ?? "—")}</div>
  </div>

  ${
    tasks.length > 0
      ? `<div class="section">
    <h2>TÂCHES</h2>
    <table>
      <tr><th style="width:8%">N°</th><th>Description</th><th style="width:12%">Statut</th></tr>
      ${taskRows}
    </table>
  </div>`
      : ""
  }

  ${
    labor.length > 0
      ? `<div class="section">
    <h2>INTERVENANTS</h2>
    <table>
      <tr><th>Nom</th><th>Compétence</th><th style="width:15%">Heures</th></tr>
      ${laborRows}
    </table>
  </div>`
      : ""
  }

  ${
    parts.length > 0
      ? `<div class="section">
    <h2>PIÈCES UTILISÉES</h2>
    <table>
      <tr><th>Désignation</th><th style="width:12%">Qté</th><th style="width:15%">Coût unit.</th></tr>
      ${partRows}
    </table>
  </div>`
      : ""
  }

  <div class="signatures">
    <div class="sig-box">
      <p><strong>Demandeur</strong></p>
      <br/><br/>
      <p>Signature : _______________</p>
      <p>Date : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>Exécutant</strong></p>
      <br/><br/>
      <p>Signature : _______________</p>
      <p>Date : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>Resp. Maintenance</strong></p>
      <br/><br/>
      <p>Signature : _______________</p>
      <p>Date : ___/___/______</p>
    </div>
  </div>

  <div class="footer">
    Réf : ${esc(wo.code)} | Maintafox | ${now} — Document confidentiel
  </div>
</body>
</html>`;
}

// ── Public API ──────────────────────────────────────────────────────────────

export async function printWoFiche(wo: WorkOrder): Promise<void> {
  const [tasks, labor, parts] = await Promise.all([
    listTasks(wo.id).catch(() => [] as WoTask[]),
    listLabor(wo.id).catch(() => [] as WoLaborEntry[]),
    listParts(wo.id).catch(() => [] as WoPartUsage[]),
  ]);

  const html = buildHtml(wo, tasks, labor, parts);
  const w = window.open("", "_blank");
  if (w) {
    w.document.write(html);
    w.document.close();
    w.print();
  }
}
