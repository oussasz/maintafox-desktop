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

*Completion: Availability tab redesigned to People × Days matrix per `docs/engineering/AVAILABILITY_TAB_FUNCTIONAL_SPEC.md` (filters, summary, contextual blocks, edit/end APIs). Verified: `pnpm typecheck`, `cargo check`.*

*Completion: Teams tab — paginated DataTable (`pageSize` 20) + centered `TeamDetailDialog` (Overview/Roster/Capacity/Skills/Succession); `list_team_members` / `get_team` IPC. Membership mutations deferred. Verified: `pnpm typecheck`, `cargo check`.*

*Completion: ResourceTimeline DS (`react-calendar-timeline`); Availability chronology + split detail + WO/DI header icon toggles; Teams split `TeamDetailWorkspace` + CrewBadge crest + skill heatmap; `teams.icon_key`/`accent_color`. Spec updated. Verified: `pnpm typecheck`, `cargo check`.*
