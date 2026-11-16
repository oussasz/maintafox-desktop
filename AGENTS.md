# Agent instructions

## Rust build cache (disk space)

- Always build into the default `src-tauri/target`. Do **not** set `CARGO_TARGET_DIR`
  or create `target-*` / `target/<something>-check` directories — each one is a full
  2–15 GB copy of the build and they were never cleaned up (they once filled ~90 GB).
- If cargo reports "Blocking waiting for file lock on build directory", wait for the
  other build; do not work around it with a separate target dir.
- To trim the cache: `npm run clean:cache` (`--dry-run` to preview). It only removes
  build output cargo regenerates; it refuses to run while cargo is running.

## Project rules (read before changing code)

General working rules: [`docs/agent-rules/project-rules.md`](docs/agent-rules/project-rules.md).
Read each rule below before a change in its area; "every change" rules apply to all work.

- [`github-git-strategy`](docs/agent-rules/github-git-strategy.md) (every change): branches, conventional commits, release and tag policy.
- [`kpi-catalog`](docs/agent-rules/kpi-catalog.md) (every change): before schema, workflow or UI work, detect KPIs and update `docs/engineering/KPI_CATALOG.md`.
- [`no-internal-ids-in-ui`](docs/agent-rules/no-internal-ids-in-ui.md) (every change): never show raw database or foreign-key IDs to users; resolve them to business labels.
- [`rbac-definition-of-done`](docs/agent-rules/rbac-definition-of-done.md) (every change): permissions are part of the definition of done for every feature.
- [`rbac-permissions`](docs/agent-rules/rbac-permissions.md) (every change): one permission registry; no permission-name string literals.
- [`reference-combobox`](docs/agent-rules/reference-combobox.md) (every change): fields backed by Données de référence use `ReferenceCombobox` and the A/B/C governance.
- [`resource-timeline-horizon-colors`](docs/agent-rules/resource-timeline-horizon-colors.md) (timeline, personnel availability): horizon, zoom and colour standards.
- [`schema-precision-zero-ambiguity`](docs/agent-rules/schema-precision-zero-ambiguity.md) (every change): schema, DTO and IPC field names match the target entity; no ambiguous or duplicate foreign keys.
- [`smart-filter-bar`](docs/agent-rules/smart-filter-bar.md) (every change): list-screen search and filters use `SmartFilterBar`.
- [`tenant-currency`](docs/agent-rules/tenant-currency.md) (every change): money uses the tenant currency from Settings; never hard-code EUR or USD.
- [`unified-interface-primitives`](docs/agent-rules/unified-interface-primitives.md) (every change): one shared design-system renderer for unified interface patterns.
- [`universal-entity-sync`](docs/agent-rules/universal-entity-sync.md) (every change): every primary domain entity syncs across trusted devices.

Domain review agent: [`.claude/agents/reliability-maintenance-expert.md`](.claude/agents/reliability-maintenance-expert.md)
checks maintenance, CMMS and reliability correctness of a feature (it does not write code).
