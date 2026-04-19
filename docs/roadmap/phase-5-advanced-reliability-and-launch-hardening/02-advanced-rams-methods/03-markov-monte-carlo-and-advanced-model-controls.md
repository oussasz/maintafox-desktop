# Markov Monte Carlo And Advanced Model Controls

**PRD:** §6.10

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **MC** — Configurable trials, seed for reproducibility.
- **Markov** — State graph solver.
- **Guardrails** — Tenant flags for advanced methods.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 02-advanced-rams-methods — Sprint 03 — Markov Monte Carlo And Advanced Model Controls

**Read Only:**
- @docs/PRD.md
- @src-tauri/src/sync/domain.rs
- @docs/research/
- @src-tauri/src/

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **MC** — Configurable trials, seed for reproducibility.
3. **Markov** — State graph solver.
4. **Guardrails** — Tenant flags for advanced methods.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
