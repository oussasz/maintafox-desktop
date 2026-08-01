# Changelog

All notable changes to Maintafox Desktop are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Important:** The `[Unreleased]` section must contain at least one entry before a PR
> may be merged to `main`. The release workflow moves `[Unreleased]` content to a dated
> version entry and creates a Git tag.

---

## [0.4.0-dev] — 2026-04-17

Phase 4 pre-release after repository history unification: the advanced Phase 4 control-plane
and desktop baseline is merged with the official `main` line, release-control compliance
artifacts are retained, and CI/changelog alignment is restored from `origin/main`.

### Added

- Unified baseline combining Phase 4 advanced core (`a0a3785` lineage) with official
  repository structure and PR history (including release control documentation).
- `docs/RELEASE_CONTROL_COMPLIANCE_REPORT.md` and related compliance workflow alignment.

### Changed

- Version bump to `0.4.0-dev` per `docs/VERSIONING_POLICY.md` (Phase 4 — `0.4.x` range).

---

## [Unreleased]

### Added
- Org: Structure model service — versioned schema lifecycle (draft → active →
  superseded → archived) with atomic publish transition (P2-SP01-F01-S1)
- Org: Node type service — tenant-defined vocabulary with 5 capability flags
  (`can_host_assets`, `can_own_work`, `can_carry_cost_center`, `can_aggregate_kpis`,
  `can_receive_permits`), root-type uniqueness, draft-model guard (P2-SP01-F01-S2)
- Org: Relationship rules service — parent-child type pairing rules with duplicate
  prevention, draft-model guard, and JOINed label denormalization (P2-SP01-F01-S2)
- Org: 11 IPC commands (`list_org_structure_models`, `get_active_org_structure_model`,
  `create_org_structure_model`, `publish_org_structure_model`,
  `archive_org_structure_model`, `list_org_node_types`, `create_org_node_type`,
  `deactivate_org_node_type`, `list_org_relationship_rules`,
  `create_org_relationship_rule`, `delete_org_relationship_rule`) with `org.view` /
  `org.admin` permission gates (P2-SP01-F01-S3)
- Org: `org-service.ts` — frontend IPC wrappers for all 11 org commands (P2-SP01-F01-S3)
- Org: `org-store.ts` — Zustand store caching active model, node types, and relationship
  rules with `loadActiveModelConfig()` action (P2-SP01-F01-S3)
- Org: `org.view`, `org.manage`, `org.admin` permission seeds; `org.admin` marked
  dangerous with step-up required (P2-SP01-F01-S3)
- Org: 29 integration tests covering structure model lifecycle, node type CRUD,
  capability flags, root-type uniqueness, relationship rules, and draft-model
  guards (P2-SP01-F01)
- Shared: `OrgStructureModel`, `OrgNodeType`, `OrgRelationshipRule`,
  `CreateStructureModelPayload`, `CreateOrgNodeTypePayload`,
  `CreateRelationshipRulePayload` types added to `shared/ipc-types.ts` (P2-SP01-F01-S3)
- Shell: Role-scoped sidebar — nav items filtered by `usePermissions().can()` with
  `requiredPermission` field on 24 of 27 items; 3 always-visible (dashboard,
  notifications, profile); empty groups auto-hidden (P2-SP00-F03-S1)
- Shell: Command palette (`⌘K` / `Ctrl+K`) — Dialog-based search over nav-registry
  with bilingual matching, permission filtering, and keyboard navigation (P2-SP00-F03-S2)
- Shell: `useNotificationCount` polling hook — queries `get_unread_notification_count`
  IPC every 30 s with silent fallback to 0 when backend unavailable (P2-SP00-F03-S3)
- Shell: `useDeviceTrustStatus` hook — fetches device trust status from Rust backend
  with silent fallback to `unknown` (P2-SP00-F03-S3)
- Shell: User menu enhancements — session time-remaining indicator and device trust
  badge (`SessionTimeIndicator`, `DeviceTrustBadge`) in TopBar dropdown (P2-SP00-F03-S3)
- Shell: Sidebar verification tests — 4 tests covering admin/non-admin/always-visible/
  empty-group scenarios (P2-SP00-F03-S1)
- i18n: Added `commandPalette.title`, `session.timeRemaining`, `session.expired`,
  `session.active`, `device.trusted`, `device.untrusted` keys to FR and EN shell
  namespaces (P2-SP00-F03)
- Monorepo scaffold: Tauri 2.x + React 18 + TypeScript 5 workspace structure
- Rust application core with `AppError` typed error system and `AppResult<T>` alias
- `health_check` IPC command with typed `HealthCheckResponse` contract in `shared/ipc-types.ts`
- SQLite local data plane with WAL mode and initial system schema migrations
  (system_config, trusted_devices, audit_events, app_sessions, user_accounts, roles,
  permissions, role_permissions, user_scope_assignments)
- Database developer tooling: `db:seed` and `db:reset` scripts with baseline seed data
  (6 system roles, admin account, system config entries)
- pnpm workspace with full TypeScript strict configuration
- ESLint 9 flat config, Prettier, Biome, and lint-staged pre-commit hooks
- Vitest test setup with Tauri IPC mock and jsdom environment
- GitHub Actions CI pipeline: lint, Rust quality, frontend tests, Tauri build check,
  security audit jobs
- Staging promotion workflow (`staging-promote.yml`)
- Git branching strategy: feature/fix/hotfix/chore branches with protection rules reference
- PR template with Supervisor Acceptance block and Definition of Done checklist
- Issue templates: Bug Report and Supervisor Feedback
- One-command development environment setup scripts (PowerShell and Bash)
- Environment preflight checker (`check-env.ts`) with PASS/FAIL table output
- Secrets management policy and `.env.example` with security prohibitions documented
- `.gitleaks.toml` secret scanning configuration with allowlisted placeholder patterns
- GitHub environments reference (`.github/environments.md`) for staging and production
- Architecture Decision Records (ADR-001 through ADR-005)
- Coding standards documents: frontend (CODING_STANDARDS_FRONTEND.md) and
  Rust (CODING_STANDARDS_RUST.md)
- Code review guide with 25-item checklist
- French-first i18n architecture with `fr/` and `en/` locale namespaces (`common.json`)
- Rust toolchain pinned to stable with MSRV 1.78 via `rust-toolchain.toml`
- Changelog and versioning governance (VERSIONING_POLICY.md, RELEASE_CHECKLIST.md)
- IPC Command Registry (`IPC_COMMAND_REGISTRY.md`) with `health_check` as first entry
- Team working agreements defining roles, sprint protocol, Definition of Done, change
  request protocol, and escalation conditions (WORKING_AGREEMENTS.md)
- Agent onboarding guide with environment setup, branch naming convention, sprint
  execution rules, and "What You Must Never Do" constraints (ONBOARDING_GUIDE.md)
- Desktop shell: Tauri window configuration with minimum 1024×600, hidden at startup
- Desktop shell: Single-instance enforcement (second launch focuses existing window)
- Desktop shell: Startup sequence (DB ready → migrations → entitlement cache → ready)
- Desktop shell: System tray with Show/Hide/Quit (French labels)
- Desktop shell: Window state persistence (size, position, maximized state)
- Desktop shell: Minimize-to-tray on OS close-button
- Tests: WindowState serialization, StartupEvent serialization (8 unit tests)
- Scripts: audit-tauri-conf.ts for CSP, capability, and window constraint verification
- Rust core: `AppState` struct with db pool, config cache, session stub, and task supervisor (`state.rs`)
- Rust core: `BackgroundTaskSupervisor` with spawn, cancel, status, and graceful shutdown (`background/mod.rs`)
- Rust core: Graceful shutdown hooked into Tauri `WindowEvent::Destroyed`
- Rust core: IPC commands `get_app_info` and `get_task_status` (`commands/app.rs`)
- Frontend: `src/services/app.service.ts` — typed service wrappers for `health_check`, `get_app_info`, `get_task_status`
- Frontend: `src/assets/logo/` — SVG and PNG logo assets with color and white variants; barrel export via `index.ts`
- Frontend: `src/vite-env.d.ts` — Vite client type reference for asset imports
- Docs: `IPC_COMMAND_REGISTRY.md` updated to 3 commands with summary table and rules
- Shared: `AppInfoResponse` and `TaskStatusEntry` types added to `shared/ipc-types.ts`

---

## Version History

*No released versions yet. Version 1.0.0 targets Phase 5 completion.*
