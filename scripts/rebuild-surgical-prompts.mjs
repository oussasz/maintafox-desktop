/**
 * Rebuild gaps/ Oussama blocks: dual-channel VPS vs codebase (no server terms in Stage 2).
 * Run: node scripts/rebuild-surgical-prompts.mjs
 */
import fs from "fs";
import path from "path";

const GAPS_ROOT = "docs/roadmap/phase-5-advanced-reliability-and-launch-hardening/gaps";
const HEADER = "## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical";

function readPathsFor(rel) {
  const p = rel.replace(/\\/g, "/");
  if (p.includes("01-loto-work-permit-system"))
    return [
      "@docs/PRD.md §6.23",
      "@docs/research/MODULES_6_23_6_25_WORK_PERMITS_AND_INSPECTION_ROUNDS.md",
      "@src-tauri/src/migrations/",
      "@src-tauri/src/sync/domain.rs",
      "@src-tauri/src/commands/",
    ];
  if (p.includes("02-training-certification-habilitation"))
    return ["@docs/PRD.md §6.20", "@src/pages/PersonnelPage.tsx", "@src-tauri/src/migrations/", "@src-tauri/src/sync/domain.rs"];
  if (p.includes("03-inspection-rounds-and-checklists"))
    return ["@docs/PRD.md §6.25", "@src-tauri/src/migrations/", "@src-tauri/src/sync/domain.rs", "@src/pages/InspectionsPage.tsx"];
  if (p.includes("04-budget-cost-center-closure"))
    return ["@docs/PRD.md §6.24", "@src/pages/BudgetPage.tsx", "@src-tauri/src/sync/domain.rs", "@src/services/sync-vps-transport-service.ts"];
  if (p.includes("05-reliability-data-foundation-iso-14224"))
    return ["@docs/PRD.md §6.10.1", "@docs/research/MODULE_6_10_RELIABILITY_ENGINE.md", "@src-tauri/src/migrations/", "@src-tauri/src/sync/domain.rs"];
  if (p.includes("06-work-order-closeout-and-data-integrity"))
    return ["@docs/PRD.md §6.5", "@src-tauri/src/", "@src-tauri/src/sync/domain.rs", "@src/pages/WorkOrdersPage.tsx"];
  if (p.includes("07-scientific-output-validation"))
    return ["@docs/research/MODULE_6_10_RELIABILITY_ENGINE.md", "@src-tauri/", "gaps/05-reliability-data-foundation-iso-14224/03-runtime-exposure-denominators-and-reliability-kpi-snapshots.md"];
  return ["@docs/PRD.md", "@src-tauri/src/sync/domain.rs"];
}

function extractBetween(src, start, endRegex) {
  const i = src.indexOf(start);
  if (i < 0) return "";
  const rest = src.slice(i + start.length);
  const m = rest.match(endRegex);
  return m ? rest.slice(0, m.index).trim() : rest.trim();
}

function syncLooksNfa(syncChunk) {
  const t = syncChunk.toUpperCase();
  return /\bN\/A\b/.test(t) && !/\| `entity_type`/.test(syncChunk);
}

function vpsFromSync(syncChunk, rel) {
  const s = syncChunk.toLowerCase();
  if (rel.includes("07-scientific-output-validation")) return false;
  if (syncLooksNfa(syncChunk)) return false;
  if (s.includes("no new entities") && (s.includes("ui only") || s.includes("ui-only"))) return false;
  if (s.includes("typically") && s.includes("no new entities")) return false;
  if (s.includes("n/a") && !s.includes("`entity_type`")) return false;
  return true;
}

/** Stage 1 only — server/tenant DB language OK here */
function vpsDispatchBullets(rel, vpsYes) {
  if (!vpsYes) {
    if (rel.includes("07-scientific-output-validation")) {
      return [
        "- None — golden/regression harness is desktop/CI; do not replicate test DB to tenant mirror.",
        "- Optional (ops): if KPI recompute runs off-device, parity-check using same fixture JSON as `rams_golden` (separate ops ticket).",
      ];
    }
    return ["- None — no mirror/API work for this sprint per Sync section (desktop/UI/tests only)."];
  }
  if (rel.includes("02-training") && rel.includes("01-qualification"))
    return [
      "- Create/update tenant mirror tables for qualifications + personnel_qualifications + profiles; PII minimization policy.",
      "- Inbound apply + validation; idempotent upsert on (tenant_id, entity_sync_id).",
    ];
  if (rel.includes("04-budget") && rel.includes("02-erp"))
    return [
      "- HTTP callback route + auth for ERP posting acknowledgements (see Phase 4 API patterns).",
      "- Mirror posted_export_batches / integration_exceptions; reconcile with desktop batches.",
    ];
  return [
    "- PostgreSQL mirror DDL aligned with desktop migrations; inbound apply + row validation.",
    "- Idempotent upsert (tenant_id, entity_sync_id); joint replay test with desktop outbox.",
  ];
}

function completionFooter(rel) {
  if (rel.includes("07-scientific-output-validation"))
    return "\n---\n\n*Completion: date, verifier, `cargo test rams_golden` / CI.*\n";
  return "\n---\n\n*Completion: date, verifier, `cargo check` / `pnpm typecheck`.*\n";
}

function syncJsonLine(syncChunk, rel) {
  if (rel.includes("07-scientific-output-validation"))
    return "N/A — golden fixtures / local CI only; no exchange payloads (see Sync section).";
  if (syncChunk.includes("`entity_type`"))
    return "Verified keys: use **Sync transport specification** table above (`entity_type` + `payload_json`). Do not invent keys.";
  return syncChunk.replace(/\|/g, " ").replace(/\s+/g, " ").slice(0, 260);
}

function buildPrompt({ titleLine, syncChunk, rel, trackSlug, sprintNum }) {
  const h1 = titleLine.replace(/^#\s*/, "").replace(/^Sprint\s+\d+\s+—\s*/i, "").trim();
  const status = `Gaps ${trackSlug} — Sprint ${sprintNum} — ${h1}`;
  const vpsYes = vpsFromSync(syncChunk, rel);
  const paths = readPathsFor(rel);
  const s1 = vpsDispatchBullets(rel, vpsYes);

  const lines = [
    HEADER,
    "",
    "🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)",
    "",
    "**Instructions for VPS Agent:**",
    "",
    ...s1,
    "",
    "🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)",
    "",
    `**[STATUS]:** ${status}`,
    "",
    "**Read Only:**",
    ...paths.map((p) => `- ${p}`),
    "",
    "**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Never** edit VPS/server repos or run infra deploys from this stage.",
    "",
    "**Actions:**",
  ];

  if (rel.includes("07-scientific-output-validation")) {
    lines.push("1. Add `rams_golden` (or equivalent) + JSON fixtures; call **only** public Rust calculator APIs used in production.");
    lines.push("2. Assert tolerances from Business rules; forbid duplicated math in tests.");
    lines.push("3. Wire `cargo test` / CI gate; keep fixtures in-repo.");
  } else {
    lines.push("1. SeaORM/SQLite migrations + entities per **Schema** / **Business rules** in this doc (local DB).");
    lines.push("2. Tauri commands, IPC, UI surfaces; stage **outbox** rows on authoritative writes.");
    lines.push("3. Register/sync `entity_type` strings + serializers in `@src-tauri/src/sync/domain.rs` (exchange payload shape only—no server config here).");
  }

  lines.push("");
  lines.push(`**Sync JSON:** ${syncJsonLine(syncChunk, rel)}`);
  lines.push("");
  lines.push("**Done:** `cargo check` + `pnpm typecheck` (+ integration tests if listed in this sprint).");
  lines.push("");
  return lines.join("\n");
}

function processFile(filePath) {
  let raw = fs.readFileSync(filePath, "utf8");
  const rel = path.relative(".", filePath).replace(/\\/g, "/");
  const parts = rel.split("/");
  const trackSlug = parts.find((p) => /^\d{2}-/.test(p)) || "gaps";
  const base = path.basename(filePath, ".md");
  const sprintNum = /^\d{2}-/.test(base) ? base.slice(0, 2) : "?";

  const syncChunk = extractBetween(raw, "## Sync transport specification", /\n---\s*\n/);
  const titleLine = (raw.match(/^#\s+.+/m) || [""])[0];

  const idx = raw.indexOf("## Ready-to-Execute AI Prompt");
  if (idx < 0) return false;
  raw = raw.slice(0, idx);
  raw += buildPrompt({ titleLine, syncChunk, rel, trackSlug, sprintNum });
  raw += completionFooter(rel);
  fs.writeFileSync(filePath, raw, "utf8");
  return true;
}

function walk(dir, out = []) {
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    if (fs.statSync(p).isDirectory()) walk(p, out);
    else if (name.endsWith(".md") && name !== "README.md") out.push(p);
  }
  return out;
}

let n = 0;
for (const f of walk(GAPS_ROOT)) {
  if (processFile(f)) n++;
}
console.log("Rebuilt gaps dual-channel prompts:", n);
