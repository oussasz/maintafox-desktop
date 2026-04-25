# Observability Permissions And Cross Module Validation

**PRD:** §6.14 / §6.17

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Permission matrix** — Validate `log.view`, `arc.*`, notification prefs across scoped RBAC in `src-tauri/src/commands/`.
- **Cross-module hooks** — Ensure DI/WO/RBAC emit activity + audit consistently.
- **Observability docs** — Short operator checklist in-repo or settings diagnostics link.
- **Regression suite** — Vitest/Rust tests for permission edges; `cargo check`, `pnpm typecheck`.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
