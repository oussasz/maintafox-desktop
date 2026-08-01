# Localization Signoff And Release Quality Gates

**PRD:** §2.3

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Signoff** — Checklist for i18n release.
- **Gates** — Block release on missing keys.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 06-localization-completion-bilingual-qa-and-future-locale-readiness — Sprint 04 — Localization Signoff And Release Quality Gates

**Read Only:**
- @src/
- locales/
- package.json

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **Signoff** — Checklist for i18n release.
3. **Gates** — Block release on missing keys.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
