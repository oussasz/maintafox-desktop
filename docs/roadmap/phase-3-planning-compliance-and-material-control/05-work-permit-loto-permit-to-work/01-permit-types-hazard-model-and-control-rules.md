# Permit Types Hazard Model And Control Rules

**PRD:** §6.23

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Schema** — `permit_types`, hazard JSON, PPE refs.
- **State machine** — Permit status enum + guards in Rust.
- **Commands** — `ptw.*` IPC; `shared/ipc-types.ts`.
- **UI** — Permit type admin, `PermitsPage` shell.
- **Role-to-permit eligibility** — Define required qualification/authorization links per permit type and hazard class for assignee validation.
- **Assignment integration** — Expose permit eligibility flags to WO scheduling so non-eligible personnel cannot be committed to gated work.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
