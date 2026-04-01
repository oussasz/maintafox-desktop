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

---

## Version History

*No released versions yet. Version 1.0.0 targets Phase 5 completion.*
