# Code Signing Notarization And Supply Chain Controls

**PRD:** §12

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Signing** — Windows/macOS signing in CI.
- **Notarize** — Apple notarization stapling.
- **SBOM** — Dependency SBOM export.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- Host signed update manifest over HTTPS; CDN/cache headers per release ops.
- Rotate signing keys per key-management runbook (not in desktop repo).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 05-security-review-code-signing-packaging-and-installer-quality — Sprint 02 — Code Signing Notarization And Supply Chain Controls

**Read Only:**
- .github/workflows/
- src-tauri/tauri.conf.json
- @docs/RELEASE_CONTROL_COMPLIANCE_REPORT.md

**Agent Rules:** Only tagged `@` paths. No whole-file rewrites—output **full code for changed blocks** only. No explanations or confirmations. Minimal comments. One pass per file where possible. **Do not** reference or configure PostgreSQL, Nginx, or non-desktop hosts from this stage.

**Actions:**
1. Execute **Tasks** in this file using SQLite/Tauri/React only (local app + migrations).
2. **Signing** — Windows/macOS signing in CI.
3. **Notarize** — Apple notarization stapling.
4. **SBOM** — Dependency SBOM export.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
