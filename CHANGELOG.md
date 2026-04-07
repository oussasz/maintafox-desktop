# Changelog

All notable changes to Maintafox Desktop are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **Important:** The `[Unreleased]` section must contain at least one entry before a PR
> may be merged to `main`. The release workflow moves `[Unreleased]` content to a dated
> version entry and creates a Git tag.

---

## [Unreleased]

### Added
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
- DI domain model: 11-state machine with guard_transition, code generation, and
  immutability checks (`di/domain.rs`, migration 017)
- DI review workflow: screen, return, reject, approve, defer, reactivate commands
  with review events and state transition logging (`di/review.rs`, migration 018)
- DI attachments: upload, list, delete with binary IPC transport and size validation
  (`di/attachments.rs`, migration 019)
- DI SLA engine: rule resolution by urgency+origin, breach detection, status computation
  (`di/sla.rs`, migration 019)
- DI WO conversion: approved DI to work_order_stubs with step-up guard and immutability
  enforcement (`di/conversion.rs`, migration 019)
- DI audit trail: append-only `di_change_events` ledger with fire-and-log writer;
  records both successful and blocked dangerous actions (`di/audit.rs`, migration 020)
- DI permission domain: 7 canonical `di.*` permissions seeded via migration 020
  (di.view, di.create, di.create.own, di.review, di.approve, di.convert, di.admin)
- DI test suite: 12 tests covering state machine, SLA, optimistic locking, full lifecycle,
  return/resubmit, and rejection paths (`di/tests.rs`)
- DI frontend services: di-service.ts, di-review-service.ts, di-attachment-service.ts,
  di-conversion-service.ts, di-audit-service.ts — all Zod-validated IPC wrappers
- DI frontend components: DiAttachmentPanel (drag-and-drop upload), WoConversionModal
  (step-up guarded conversion), DiAuditTimeline (read-only vertical timeline with
  action icons, apply_result badges, step-up badges), DiDetailPanel (tabbed detail
  with Attachments and Audit Trail tabs)
- 20 DI IPC commands registered in invoke_handler and IPC_COMMAND_REGISTRY.md

---

## Version History

*No released versions yet. Version 1.0.0 targets Phase 5 completion.*
