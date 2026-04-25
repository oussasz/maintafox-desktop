# Skills Availability And Team Capacity

**PRD:** §6.6 (skills matrix, team assignments, availability blocks, capacity vs WO/PM)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md`.

## Tasks

- **Schema** — `personnel_skills`, `personnel_team_assignments`, `personnel_availability_blocks` + skill seeds in SP03 `reference_values` domain in `src-tauri/src/migrations/`.
- **Computation** — Availability windows (schedule minus blocks), skills matrix query, team capacity summary in `src-tauri/src/personnel/{skills,availability,teams}.rs`.
- **IPC** — Extend `commands/personnel.rs`; align `shared/ipc-types.ts`.
- **UI** — `SkillsMatrixPanel`, `AvailabilityCalendar`, `TeamCapacityBoard` under `src/components/personnel/`; wire tabs on `PersonnelPage.tsx`.
- **Notifications** — Optional `notify::emit_event` for critical blocks (medical/restriction) via existing SP07 stack.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
