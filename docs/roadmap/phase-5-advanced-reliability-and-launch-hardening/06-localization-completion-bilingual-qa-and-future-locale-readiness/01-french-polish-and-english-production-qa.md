# French Polish And English Production QA

**PRD:** §2.3 / §6.11

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **QA pass** — FR primary + EN: UI sweep.
- **Fixes** — Typography, truncation, locale formats.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 06-localization-completion-bilingual-qa-and-future-locale-readiness — Sprint 01 — French Polish And English Production QA

**Read Only:**
- @src/
- locales/
- package.json

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **QA pass** — FR primary + EN: UI sweep.
3. **Fixes** — Typography, truncation, locale formats.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
