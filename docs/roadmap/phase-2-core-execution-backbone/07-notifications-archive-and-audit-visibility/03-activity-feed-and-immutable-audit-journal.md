# Activity Feed And Immutable Audit Journal

**PRD:** §6.17

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Activity + audit tables** — Migrations for `activity_events`, `audit_events` (or aligned names), writers in `src-tauri/src/activity/`, `src-tauri/src/audit/`.
- **Commands** — `commands/activity_feed.rs`, `commands/audit_log.rs`; fix entity-scoped `log.view` if still needed.
- **UI** — `src/components/activity/*`, `src/pages/ActivityPage.tsx`, filters + chain expansion.
- **Emitters** — Wire emits from admin, archive, RBAC mutations per handoff docs.
- **Tests** — Permission + duplicate-chain fixes; `pnpm test` targeted.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
