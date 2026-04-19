# Pilot Environment Preparation And Customer Onboarding

**PRD:** §15

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Pilot kit** — Install guide, data migration, training agenda.
- **Environment** — Hardware/network prerequisites.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- Provision pilot tenant + license; empty tenant mirror schema; validate sync exchange for that tenant.
- Coordinate `protocol_version` + deploy order (API/edge before desktop when bumping).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 07-pilot-rollout-support-runbooks-and-go-live-readiness — Sprint 01 — Pilot Environment Preparation And Customer Onboarding

**Read Only:**
- @docs/RELEASE_CONTROL_COMPLIANCE_REPORT.md
- @docs/VERSIONING_POLICY.md
- docs/runbooks/

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **Pilot kit** — Install guide, data migration, training agenda.
3. **Environment** — Hardware/network prerequisites.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
