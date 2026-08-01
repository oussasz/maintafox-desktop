# Qualification Profiles And Certification Model

**PRD:** §6.20

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Tasks

- **Schema** — `certification_types`, `personnel_certifications`, requirement profiles.
- **Link personnel** — FK from training module to `personnel` / `user_accounts`.
- **Commands** — `trn.*` CRUD certifications.
- **UI** — `TrainingPage` matrix shell.
- **WO skill mapping** — Map certification profiles to WO/PM required-skill sets for assignment-time readiness scoring.
- **Costing metadata** — Add training/qualification cost attributes (course cost, renewal cost, vendor) for workforce investment analysis.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
