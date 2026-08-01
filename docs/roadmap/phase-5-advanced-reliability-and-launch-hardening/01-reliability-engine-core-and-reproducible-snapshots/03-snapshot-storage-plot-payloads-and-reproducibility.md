# Snapshot Storage Plot Payloads And Reproducibility

**PRD:** §9

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Payloads** — Store plot JSON + inputs hash.
- **Reopen** — Reload same snapshot.
- **Export** — PNG/SVG export.






## Ready-to-Execute AI Prompt (Oussama Protocol) — Surgical

🌐 [STAGE 1]: VPS AGENT DISPATCH (DO NOT COPY TO CODEBASE)

**Instructions for VPS Agent:**

- None — no VPS dispatch required for this sprint document (infra-only work is out of scope here).

🖥️ [STAGE 2]: CODEBASE AGENT EXECUTION (COPY FROM HERE)

**[STATUS]:** Phase 5 01-reliability-engine-core-and-reproducible-snapshots — Sprint 03 — Snapshot Storage Plot Payloads And Reproducibility


2. **Payloads** — Store plot JSON + inputs hash.
3. **Reopen** — Reload same snapshot.
4. **Export** — PNG/SVG export.

**Sync JSON:** If this file defines `entity_type` / `payload_json`, implement serializers + outbox staging in desktop; verified keys must match tables in doc. Else N/A.

**Done:** `cargo check` + `pnpm typecheck` (and tests listed in this file if any).

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
