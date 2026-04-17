# Notification Routing And Delivery Model

**PRD:** §6.14

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Notification schema + seed** — Add or align `notification_*` tables and default categories in `src-tauri/src/migrations/`, register in `migrations/mod.rs`.
- **Emitter + router + scheduler** — Implement `src-tauri/src/notifications/{emitter,router,scheduler,delivery}.rs` and wire scheduler on startup.
- **IPC + shared types** — `src-tauri/src/commands/notifications.rs`, `generate_handler!`, `shared/ipc-types.ts`.
- **Inbox UI** — `src/components/notifications/*`, shell header bell, preferences panel.
- **Integration smoke** — Emit from DI/WO paths where applicable; `cargo check`, `pnpm typecheck`.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
