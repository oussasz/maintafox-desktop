# PM Strategy Models And Trigger Logic

**PRD:** §6.9

**Execution:** See [Roadmap execution](../../../README.md#roadmap-execution) in `README.md` (no long prompt blocks; task list + direct implementation in `maintafox-desktop/`).

## Delivery Slices

### S1 - PM Backend Bootstrap And Contract Foundations
- Create missing PM backend surface first: `src-tauri/src/pm/` module + `src-tauri/src/commands/pm.rs` + command registration in `src-tauri/src/commands/mod.rs` and `src-tauri/src/lib.rs` invoke handler.
- Add first PM migration backbone: `pm_plans`, `pm_plan_versions`, `pm_occurrences`, `pm_trigger_events`, `pm_executions`, `pm_findings` with FK integrity, `row_version` where mutable, timestamps, and non-destructive lifecycle semantics.
- Keep meter/counter strategy realistic with current codebase: use existing asset-meter sources (`asset_meters` and `meter_readings`) as trigger inputs before introducing duplicate counter truth.
- Enforce permission boundary using existing seeded catalog (`pm.view` / `pm.create` / `pm.edit` / `pm.delete`) on every PM command.
- Add baseline typed errors for overlap, invalid lifecycle transition, stale row version, missing asset scope, and invalid trigger definition.

**Cursor Prompt (S1)**
```text
Bootstrap the PM backend from zero by adding a dedicated Rust pm module, first PM migrations, and Tauri command registration. Enforce pm.* permissions and optimistic concurrency, and reuse existing asset meter readings as the initial counter source.
```

### S2 - Versioned Strategy Governance And Reference Safety
- Implement PM plan lifecycle (`draft`, `proposed`, `approved`, `active`, `suspended`, `retired`) and PM plan-version effective dating with no overlapping active ranges for the same plan.
- Validate strategy payload structure (`fixed`, `floating`, `meter`, `event`, `condition`) and reject free-form fields that should be governed by lookups/reference domains.
- Include strategy resource contracts per version: required skills, parts envelope, tools, expected labor duration, and permit/shutdown flags aligned with PRD/research.
- Add compatibility hooks to existing reference-governance workflow: PM-related governed values must be publish-safe (remove current "unavailable module" behavior for `pm_plans` impact once PM tables exist).
- Keep all PM writes in Rust service/query layer transactions (no direct command-layer SQL scattered across files).

**Cursor Prompt (S2)**
```text
Implement versioned PM strategy governance with strict lifecycle rules, effective dating, and trigger-definition validation. Ensure PM strategy fields that depend on governed domains are validated consistently with the reference publish model.
```

### S3 - IPC Contracts, PM UX Replacement, And Regression Safety
- Add PM IPC contracts to `shared/ipc-types.ts` and frontend PM service calls with runtime decoding/validation patterns consistent with other modules.
- Replace `src/pages/PmPage.tsx` placeholder with real list/detail/editor flows bound to PM commands (no mock rows, no static datasets).
- Populate PM i18n namespaces (`src/i18n/locale-data/en/pm.json`, `src/i18n/locale-data/fr/pm.json`) with actual keys used by the new PM UI.
- Add migration/unit tests for lifecycle transitions, active-version exclusivity, permission gating, and stale row-version rejection.
- Add smoke coverage that PM navigation + permission route works end-to-end against real IPC (not placeholder-only rendering).

**Cursor Prompt (S3)**
```text
Finalize PM strategy delivery end-to-end: typed IPC contracts, real PM page CRUD flows, localized strings, and tests for lifecycle governance, permissions, and concurrency edge cases with no placeholder data paths.
```

---

*Completion: date, verifier, `cargo check` / `pnpm typecheck` notes.*
