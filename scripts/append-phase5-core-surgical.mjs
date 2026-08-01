/**
 * Phase 5 core: dual-channel VPS vs codebase prompts (Stage 2 = desktop only).
 * Run: node scripts/append-phase5-core-surgical.mjs
 */
import fs from "fs";
import path from "path";

const ROOT = "docs/roadmap/phase-5-advanced-reliability-and-launch-hardening";
const HEADER = "## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical";

const TRACKS = [
  "01-reliability-engine-core-and-reproducible-snapshots",
  "02-advanced-rams-methods",
  "03-analytics-dashboards-exports-and-reporting-finalization",
  "04-performance-resilience-migration-and-recovery-hardening",
  "05-security-review-code-signing-packaging-and-installer-quality",
  "06-localization-completion-bilingual-qa-and-future-locale-readiness",
  "07-pilot-rollout-support-runbooks-and-go-live-readiness",
];

function readPaths(track) {
  const base = ["@docs/PRD.md", "@src-tauri/src/sync/domain.rs"];
  if (track.startsWith("01-")) return [...base, "@src-tauri/src/migrations/", "@src-tauri/src/commands/"];
  if (track.startsWith("02-")) return [...base, "@docs/research/", "@src-tauri/src/"];
  if (track.startsWith("03-")) return ["@docs/PRD.md", "@src/pages/", "@src-tauri/src/sync/domain.rs"];
  if (track.startsWith("04-")) return ["@src-tauri/src/migrations/", "@src-tauri/src/sync/domain.rs", "@src-tauri/src/"];
  if (track.startsWith("05-")) return [".github/workflows/", "src-tauri/tauri.conf.json", "@docs/RELEASE_CONTROL_COMPLIANCE_REPORT.md"];
  if (track.startsWith("06-")) return ["@src/", "locales/", "package.json"];
  if (track.startsWith("07-")) return ["@docs/RELEASE_CONTROL_COMPLIANCE_REPORT.md", "@docs/VERSIONING_POLICY.md", "docs/runbooks/"];
  return base;
}

/** Stage 1 bullets — infra/release; not for codebase agent */
function vpsStage1(track, fileBase) {
  if (track.startsWith("07-pilot-rollout"))
    return [
      "- Provision pilot tenant + license; empty tenant mirror schema; validate sync exchange for that tenant.",
      "- Coordinate `protocol_version` + deploy order (API/edge before desktop when bumping).",
    ];
  if (track.startsWith("05-security") && fileBase.startsWith("02-"))
    return [
      "- Host signed update manifest over HTTPS; CDN/cache headers per release ops.",
      "- Rotate signing keys per key-management runbook (not in desktop repo).",
    ];
  if (track.startsWith("05-security") && fileBase.startsWith("01-"))
    return [
      "- Staging API: rate limits + WAF scope for pen-test (ops ticket).",
      "- Optional: central audit log sink for fleet diagnostics (if product uses it).",
    ];
  return ["- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here)."];
}

function buildBlock(track, h1, tasks, sn, fileBase) {
  const base = path.basename(track);
  const paths = readPaths(track);
  const s1 = vpsStage1(track, fileBase);

  const lines = [
    HEADER,
    "",
    "🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)",
    "",
    "**Instructions for VPS Agent:**",
    "",
    ...s1.map((b) => (b.startsWith("-") ? b : `- ${b}`)),
    "",
    "🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)",
    "",
    `**[STATUS]:** Phase 5 ${base} — Sprint ${sn} — ${h1}`,
    "",
    "**Read Only:**",
    ...paths.map((p) => `- ${p}`),
    "",
    "**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.",
    "",
    "**Actions:**",
    "1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).",
  ];

  if (tasks.length) {
    tasks.slice(0, 4).forEach((t, i) => {
      const line = t.replace(/^[-*]\s+/, "").trim();
      lines.push(`${i + 2}. ${line}`);
    });
  } else {
    lines.push("2. Implement per PRD line and headings above in the desktop repo.");
  }

  lines.push("");
  lines.push(
    "**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A."
  );
  lines.push("");
  lines.push("**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).");
  lines.push("");
  return lines.join("\n");
}

function extractTasks(raw) {
  const m = raw.match(/## Tasks\s*\n+([\s\S]*?)(?=\n---|\n## Ready-to-Execute|\n## [^#]|\Z)/);
  if (!m) return [];
  return m[1]
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("- ") || l.startsWith("* "));
}

function processFile(fp) {
  let raw = fs.readFileSync(fp, "utf8");
  raw = raw.replace(
    /\n## Ready-to-Execute AI Prompt \(Oussama Protocol\)[^\n]*\n[\s\S]*?(?=\n---\s*\r?\n\s*\*Completion:)/,
    ""
  );
  const track = path.basename(path.dirname(fp));
  const fn = path.basename(fp, ".md");
  const sn = /^\d{2}/.test(fn) ? fn.slice(0, 2) : "?";
  const h1 = (raw.match(/^#\s+(.+)/m) || ["", ""])[1].trim();
  const tasks = extractTasks(raw);
  const block = buildBlock(track, h1, tasks, sn, fn);
  const completionRe = /\n---\s*\r?\n(\s*\*Completion:[^\n]*)/;
  if (!completionRe.test(raw)) {
    console.warn("SKIP (no *Completion footer):", fp);
    return false;
  }
  raw = raw.replace(completionRe, "\n\n" + block + "\n---\n\n$1");
  fs.writeFileSync(fp, raw, "utf8");
  return true;
}

let n = 0;
for (const tr of TRACKS) {
  const dir = path.join(ROOT, tr);
  if (!fs.existsSync(dir)) continue;
  for (const name of fs.readdirSync(dir)) {
    if (!name.endsWith(".md")) continue;
    if (processFile(path.join(dir, name))) n++;
  }
}
console.log("Phase 5 core dual-channel prompts updated:", n);
