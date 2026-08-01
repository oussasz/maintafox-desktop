# Personnel Governance Imports And Validation

**PRD:** §6.6 (HRMS import field rules, reporting, audit)

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md`.

## Tasks

- **Import engine** — `src-tauri/src/personnel/import.rs`: CSV/XLSX parse, preview vs apply, CreateAndUpdate / CreateOnly / Protected / Mapped field rules, transactional apply + step-up on `commands/personnel.rs`.
- **Wizard UI** — `PersonnelImportWizard.tsx`, templates, `PersonnelExportMenu`, `WorkforceReportPanel` on `PersonnelPage.tsx`.
- **Reports** — Workforce summary, skills gap, KPIs, CSV exports in `src-tauri/src/personnel/reports.rs`; `per.report` gates.
- **Tests** — Permission, concurrency, import protection, cross-module FK checks in `src-tauri/src/personnel/tests.rs` + Vitest for UI gates.
- **Audit** — Activity/audit emits for bulk import rows; no duplicate types outside `shared/ipc-types.ts`.

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
