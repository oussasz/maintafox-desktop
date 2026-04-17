# Self Service Links And Operational Visibility

**PRD:** §6.6 / §6.19 (profile, cards, cross-module visibility, succession risk)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md`.

## Tasks

- **Detail surface** — `PersonnelDetailDialog` + tabs (identity, skills, availability, teams, rate cards, authorizations, work history, contractor banner) in `src/components/personnel/`.
- **Reusable widgets** — `PersonnelCard`, `PersonnelPickerCombobox`, barrel `src/components/personnel/index.ts` for WO/PM/permit consumers.
- **Queries** — Work history union (WO/DI roles), workload summary, succession risk scan in `src-tauri/src/personnel/` + commands.
- **Self-service** — `ProfilePage.tsx`: link session `personnel_id`, `declare_own_skill` path (self-declared only), hide rate/cost for non-admin.
- **RBAC** — Own-record read without `per.view` where specified; keep `per.manage` for edits.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
