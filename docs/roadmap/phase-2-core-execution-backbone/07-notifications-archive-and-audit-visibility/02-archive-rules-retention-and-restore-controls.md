# Archive Rules Retention And Restore Controls

**PRD:** §6.12

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Archive schema** — `archive_*` migrations under `src-tauri/src/migrations/`, writer in `src-tauri/src/archive/`.
- **Commands** — `src-tauri/src/commands/archive.rs`, capabilities for restore/purge where needed.
- **Explorer UI** — `src/components/archive/`, `src/pages/ArchivePage.tsx`, services/stores.
- **Retention policies** — Admin UI for policies; legal hold; integration with activity/audit.
- **Tests** — Archive flows + `cargo check` / `pnpm typecheck`.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
