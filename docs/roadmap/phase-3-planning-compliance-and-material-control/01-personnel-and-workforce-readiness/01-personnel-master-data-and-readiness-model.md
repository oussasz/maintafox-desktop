# Personnel Master Data And Readiness Model

**PRD:** §6.6 (personnel master, positions, schedules, rate cards, authorizations, external companies)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md`.

## Tasks

- **Personnel schema + migration** — `positions`, `schedule_classes`, `schedule_details`, `personnel`, `personnel_rate_cards`, `personnel_authorizations`, `external_companies`, `external_company_contacts` in `src-tauri/src/migrations/` (next ordinal), register in `migrations/mod.rs` and `db/migration_integrity.rs`.
- **Rust domain + queries** — `src-tauri/src/personnel/` (`domain.rs`, `queries.rs`), `generate_handler!` in `lib.rs`.
- **IPC + contracts** — `src-tauri/src/commands/personnel.rs`, `shared/ipc-types.ts`, `per.*` gates.
- **Frontend** — `src/services/personnel-service.ts`, `src/stores/personnel-store.ts`, `src/pages/PersonnelPage.tsx` (list/cards), `src/i18n/locale-data/{fr,en}/personnel.json`.
- **Cross-module hooks** — FK targets for `user_accounts.personnel_id` and WO interveners; activity events on create/update/deactivate.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
