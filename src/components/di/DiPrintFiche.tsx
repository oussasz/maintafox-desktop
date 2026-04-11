/**
 * DiPrintFiche.tsx
 *
 * Print-ready A4 DI (Demande d'Intervention) fiche. Opens a new browser
 * window with optimized HTML layout: company header, identification,
 * equipment, flags, description, review notes, signatures, footer.
 * Follows the same pattern as WoPrintFiche.tsx.
 */

import type { InterventionRequest } from "@shared/ipc-types";

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

function esc(v: string | null | undefined): string {
  if (!v) return "";
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function urgencyLabel(u: string): string {
  const map: Record<string, string> = {
    low: "Basse",
    medium: "Moyenne",
    high: "Haute",
    critical: "Critique",
  };
  return map[u] ?? u;
}

function impactLabel(i: string): string {
  const map: Record<string, string> = {
    unknown: "Inconnu",
    none: "Aucun",
    minor: "Mineur",
    major: "Majeur",
    critical: "Critique",
  };
  return map[i] ?? i;
}

function originLabel(o: string): string {
  const map: Record<string, string> = {
    operator: "Opérateur",
    technician: "Technicien",
    inspection: "Inspection",
    pm: "Maintenance préventive",
    iot: "IoT / Capteur",
    quality: "Qualité",
    hse: "HSE",
    production: "Production",
    external: "Externe",
  };
  return map[o] ?? o;
}

function statusLabel(s: string): string {
  const map: Record<string, string> = {
    submitted: "Soumise",
    pending_review: "En revue",
    returned_for_clarification: "Retournée",
    rejected: "Rejetée",
    screened: "Triée",
    awaiting_approval: "En attente d'approbation",
    approved_for_planning: "Approuvée",
    deferred: "Reportée",
    converted_to_work_order: "Convertie en OT",
    closed_as_non_executable: "Fermée (non exécutable)",
    archived: "Archivée",
  };
  return map[s] ?? s;
}

// ── Build HTML ──────────────────────────────────────────────────────────────

function buildHtml(di: InterventionRequest): string {
  const now = new Date().toLocaleDateString("fr-FR");

  const flags: string[] = [];
  if (di.safety_flag) flags.push("Sécurité");
  if (di.environmental_flag) flags.push("Environnement");
  if (di.quality_flag) flags.push("Qualité");
  if (di.production_impact) flags.push("Impact production");

  return `<!DOCTYPE html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>Fiche DI — ${esc(di.code)}</title>
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
    <h1>Fiche de Demande d'Intervention</h1>
    <div class="subtitle">Document confidentiel</div>
  </div>

  <div class="ref-strip">
    <span>Référence : <strong>${esc(di.code)}</strong></span>
    <span>Date d'émission : ${now}</span>
  </div>

  <div class="section">
    <h2>IDENTIFICATION</h2>
    <table>
      <tr><th>Code DI</th><td>${esc(di.code)}</td></tr>
      <tr><th>Titre</th><td>${esc(di.title)}</td></tr>
      <tr><th>Statut</th><td>${statusLabel(di.status)}</td></tr>
      <tr><th>Urgence déclarée</th><td>${urgencyLabel(di.reported_urgency)}</td></tr>
      ${di.validated_urgency ? `<tr><th>Urgence validée</th><td>${urgencyLabel(di.validated_urgency)}</td></tr>` : ""}
      <tr><th>Origine</th><td>${originLabel(di.origin_type)}</td></tr>
      <tr><th>Niveau d'impact</th><td>${impactLabel(di.impact_level)}</td></tr>
      <tr><th>Créée le</th><td>${fmtDate(di.created_at)}</td></tr>
      <tr><th>Soumise le</th><td>${fmtDate(di.submitted_at)}</td></tr>
    </table>
  </div>

  <div class="section">
    <h2>ÉQUIPEMENT CONCERNÉ</h2>
    <table>
      <tr><th>Équipement</th><td>#${di.asset_id}${di.sub_asset_ref ? ` — Sous-ensemble : ${esc(di.sub_asset_ref)}` : ""}</td></tr>
      <tr><th>Nœud organisationnel</th><td>#${di.org_node_id}</td></tr>
    </table>
  </div>

  ${
    flags.length > 0
      ? `<div class="section">
    <h2>DRAPEAUX</h2>
    <div class="flags">
      ${flags.map((f) => `<span class="flag">${f}</span>`).join("")}
    </div>
  </div>`
      : ""
  }

  <div class="section">
    <h2>DESCRIPTION</h2>
    ${di.observed_at ? `<p style="font-size:10px;margin-bottom:4px;">Observée le : <strong>${fmtDate(di.observed_at)}</strong></p>` : ""}
    <p style="font-size:10px;margin-bottom:4px;">Déclarant : <strong>#${di.submitter_id}</strong></p>
    <div class="desc">${esc(di.description)}</div>
  </div>

  ${
    di.reviewer_note
      ? `<div class="section">
    <h2>NOTES DE REVUE</h2>
    <div class="desc">${esc(di.reviewer_note)}</div>
  </div>`
      : ""
  }

  ${
    di.converted_to_wo_id
      ? `<div class="section">
    <h2>CONVERSION</h2>
    <table>
      <tr><th>Convertie en OT</th><td>#${di.converted_to_wo_id}</td></tr>
      <tr><th>Date de conversion</th><td>${fmtDate(di.converted_at)}</td></tr>
    </table>
  </div>`
      : ""
  }

  <div class="signatures">
    <div class="sig-box">
      <p><strong>Déclarant</strong></p>
      <br/><br/>
      <p>Signature : _______________</p>
      <p>Date : ___/___/______</p>
    </div>
    <div class="sig-box">
      <p><strong>Vérificateur</strong></p>
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
    Réf : ${esc(di.code)} | Maintafox | ${now} — Document confidentiel
  </div>
</body>
</html>`;
}

// ── Public API ──────────────────────────────────────────────────────────────

export function printDiFiche(di: InterventionRequest): void {
  const html = buildHtml(di);
  const w = window.open("", "_blank");
  if (w) {
    w.document.write(html);
    w.document.close();
    w.print();
  }
}
